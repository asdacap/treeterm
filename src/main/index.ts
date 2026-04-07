import { app, BrowserWindow, clipboard, dialog, shell } from 'electron'
import { execSync } from 'child_process'
import { join } from 'path'
import { randomUUID } from 'crypto'
import { GrpcDaemonClient, PtyStream } from './grpcClient'
import { IpcServer } from './ipc/ipc-server'
import { GitClient } from './git'
import { createRunActionsClient, RunActionsClient } from './runActions'
import { ConnectionManager } from './connectionManager'
import { windowManager } from './windowManager'
import type { ExecInput, ExecOutput } from '../generated/treeterm'
import type { SSHConnectionConfig, PortForwardConfig } from '../shared/types'

// Parse initial workspace and SSH target from command line
let initialWorkspacePath: string | null = null
let initialSSHTarget: string | null = null
for (const arg of process.argv) {
  if (arg.startsWith('--workspace=')) {
    initialWorkspacePath = arg.substring('--workspace='.length)
  }
  if (arg.startsWith('--ssh=')) {
    initialSSHTarget = arg.substring('--ssh='.length)
  }
}
import { loadSettings, saveSettings, addRecentDirectory } from './settings'
import { createApplicationMenu } from './menu'
import { startChatStream, cancelChatStream, completeChatCall, formatLlmError, parseLlmJson } from './llm'

let mainWindow: BrowserWindow | null = null
let loadingWindow: BrowserWindow | null = null
const closeConfirmedWindows: Set<number> = new Set()
let connectionManager: ConnectionManager | null = null
// Maps sessionId to the daemon client that owns it (populated when session watch returns initial data)
const sessionConnectionMap = new Map<string, string>()
// Simple object storage — each entry is an independent terminal's stream.
const ptyStreams = new Map<string, PtyStream>()
// Active exec streams keyed by execId for streaming output and kill support.
const execStreams = new Map<string, ReturnType<GrpcDaemonClient['execStream']>>()

// Helper: get the daemon client for a given connectionId
function getClientForConnection(connId: string): GrpcDaemonClient {
  if (!connectionManager) throw new Error('ConnectionManager not initialized')
  return connectionManager.getClient(connId)
}

// Initialize IPC server
const server = new IpcServer()

function createLoadingWindow(): BrowserWindow {
  const isTest = process.env.NODE_ENV === 'test'

  loadingWindow = new BrowserWindow({
    width: 300,
    height: 200,
    frame: false,
    show: false,
    center: true,
    alwaysOnTop: true,
    resizable: false,
    movable: false,
    skipTaskbar: true,
    webPreferences: {
      preload: join(__dirname, '../preload/index.js'),
      nodeIntegration: false,
      contextIsolation: true
    },
    transparent: true,
    backgroundColor: '#00000000'
  })

  void loadingWindow.loadFile(join(__dirname, 'loading.html'))

  loadingWindow.once('ready-to-show', () => {
    if (!isTest) {
      loadingWindow?.show()
    }
  })

  return loadingWindow
}

function createWindow(): BrowserWindow {
  const window = new BrowserWindow({
    width: 1200,
    height: 800,
    minWidth: 800,
    minHeight: 600,
    show: false,
    webPreferences: {
      preload: join(__dirname, '../preload/index.js'),
      nodeIntegration: false,
      contextIsolation: true
    },
    titleBarStyle: 'hiddenInset',
    trafficLightPosition: { x: 15, y: 15 }
  })

  window.once('ready-to-show', () => {
    window.show()
  })

  // Create a dedicated IPC server for this window
  const windowServer = new IpcServer()
  windowServer.setWindow(window)

  // Assign a unique UUID to this window for session sync deduplication
  const windowUuid = randomUUID()

  // Cleanup for session watch stream
  let unwatchSession: (() => void) | null = null

  // Forward all keyboard events including Caps Lock to renderer
  window.webContents.on('before-input-event', (_event, input) => {
    // Forward Caps Lock events to renderer via IPC
    if (input.code === 'CapsLock' || input.key === 'CapsLock') {
      windowServer.capsLockEvent({
        type: input.type, // 'keyDown' or 'keyUp'
        key: input.key,
        code: input.code
      })
    }
  })

  // Reset keyboard modifier state on window focus to prevent stuck keys
  // (Chromium can lose keyUp events when window is unfocused, corrupting input state)
  window.on('focus', () => {
    for (const keyCode of ['Shift', 'Control', 'Alt', 'Meta'] as const) {
      window.webContents.sendInputEvent({ type: 'keyUp', keyCode })
    }
  })

  // Open external links in the default browser instead of within Electron
  window.webContents.on('will-navigate', (event, url) => {
    const parsedUrl = new URL(url)
    if (
      parsedUrl.protocol === 'file:' ||
      (process.env.ELECTRON_RENDERER_URL && url.startsWith(process.env.ELECTRON_RENDERER_URL))
    ) {
      return
    }
    event.preventDefault()
    void shell.openExternal(url)
  })

  window.webContents.setWindowOpenHandler(({ url }) => {
    if (url && url !== 'about:blank') {
      void shell.openExternal(url)
    }
    return { action: 'deny' }
  })

  // Intercept close event to check for unmerged workspaces
  window.on('close', (event) => {
    if (!closeConfirmedWindows.delete(window.webContents.id)) {
      event.preventDefault()
      window.webContents.send('app:confirm-close')
    }
  })

  // Build the load URL
  const loadUrl = process.env.ELECTRON_RENDERER_URL
    ? process.env.ELECTRON_RENDERER_URL
    : `file://${join(__dirname, '../renderer/index.html')}`

  // Load the renderer
  void window.loadURL(loadUrl)

  // Signal renderer when ready to initialize with the session
  window.webContents.on('did-finish-load', () => { void (async () => {
    if (!connectionManager) {
      windowServer.appReady(null)
      return
    }

    try {
      const localClient = connectionManager.getClient('local')
      await localClient.ensureDaemonRunning()

      // Start watching the single session (cancel previous if HMR reload)
      if (unwatchSession) {
        unwatchSession()
      }
      const watch = localClient.watchSession(windowUuid, (updatedSession) => {
        console.log('[main] session sync received for window', window.id, {
          sessionId: updatedSession.id,
          workspaces: updatedSession.workspaces.map(ws => ({ path: ws.path, metadata: ws.metadata })),
        })
        windowServer.sessionSync('local', updatedSession)
      })
      unwatchSession = watch.unsubscribe

      const session = await watch.initial
      console.log('[main] loaded session:', session.id)
      sessionConnectionMap.set(session.id, 'local')
      windowServer.appReady(session)
    } catch (error) {
      console.error('[main] failed to get session:', error)
      windowServer.appReady(null)
    }
  })() })

  window.on('closed', () => {
    // Stop watching session
    if (unwatchSession) {
      unwatchSession()
      unwatchSession = null
    }
  })

  // Register with window manager (session ID updated later in did-finish-load)
  windowManager.registerWindow(window, windowServer, windowUuid)

  return window
}

// IPC Handlers
server.onPtyCreate(async (event, connectionId, handle, cwd, sandbox, startupCommand) => {
  if (!connectionManager) throw new Error('ConnectionManager not initialized')

  try {
    const client = getClientForConnection(connectionId)
    await client.ensureDaemonRunning()
    const sessionId = await client.createPtySession({ cwd, sandbox: sandbox, startupCommand })
    const ptyStream = client.openPtyStream(handle, sessionId, (evt) => {
      event.sender.send('pty:event', handle, evt)
      if (evt.type === 'end') ptyStreams.delete(handle)
    })
    ptyStreams.set(handle, ptyStream)

    return { success: true, sessionId }
  } catch (error) {
    const errorMessage = error instanceof Error ? error.message : String(error)
    console.error('[main] failed to create PTY session via daemon:', error)
    return { success: false, error: errorMessage }
  }
})

server.onPtyAttach(async (event, connectionId, handle, sessionId) => {
  if (!connectionManager) {
    return { success: false, error: 'ConnectionManager not initialized' }
  }

  try {
    const client = getClientForConnection(connectionId)
    await client.ensureDaemonRunning()
    const ptyStream = client.openPtyStream(handle, sessionId, (evt) => {
      event.sender.send('pty:event', handle, evt)
      if (evt.type === 'end') ptyStreams.delete(handle)
    })
    ptyStreams.set(handle, ptyStream)

    return { success: true }
  } catch (error) {
    const errorMessage = error instanceof Error ? error.message : 'Unknown error'
    console.error('[main] failed to attach to PTY session:', errorMessage)
    return { success: false, error: errorMessage }
  }
})

server.onPtyList(async (connectionId) => {
  const client = getClientForConnection(connectionId)
  await client.ensureDaemonRunning()
  return client.listPtySessions()
})

server.onPtyWrite((handle, data) => {
  ptyStreams.get(handle)?.write(data)
})

server.onPtyResize((handle, cols, rows) => {
  ptyStreams.get(handle)?.resize(cols, rows)
})

server.onPtyKill((connectionId, sessionId) => {
  // Close any PtyStreams for this session
  for (const [handle, stream] of ptyStreams) {
    if (stream.sessionId === sessionId) {
      stream.close()
      ptyStreams.delete(handle)
    }
  }
  try {
    const client = getClientForConnection(connectionId)
    void client.killPtySession(sessionId).catch((error: unknown) => {
      console.error('[main] failed to kill PTY:', error)
    })
  } catch (error) {
    console.error('[main] failed to kill PTY:', error)
  }
})


// Terminal analyzer — non-streaming LLM call with buffer cache
const analyzerCache: { buffer: string; result: { state: string; reason: string } }[] = []
const ANALYZER_CACHE_SIZE = 10

server.onLlmChatSend(async (event, requestId, messages, settings) => {
  await startChatStream(requestId, messages, settings, event.sender)
})

server.onLlmAnalyzeTerminal(async (buffer, cwd, settings) => {
  const cached = analyzerCache.find((entry) => entry.buffer === buffer)
  if (cached) {
    return { ...cached.result, cached: true }
  }

  const allSafePaths = [...new Set([...settings.safePaths, cwd])]
  const systemPrompt = settings.systemPrompt
    .replace(/\{\{cwd\}\}/g, cwd)
    .replace(/\{\{safe_paths\}\}/g, allSafePaths.join(', '))
  const messages: { role: 'user' | 'assistant' | 'system'; content: string }[] = [
    { role: 'system', content: systemPrompt },
    { role: 'user', content: buffer }
  ]
  try {
    const response = await completeChatCall(messages, {
      baseUrl: settings.baseUrl,
      apiKey: settings.apiKey,
      model: settings.model,
      reasoning: settings.reasoningEffort
    })
    const parsed = parseLlmJson(response)
    const result = { state: parsed.state as string, reason: parsed.reason as string }
    analyzerCache.push({ buffer, result })
    if (analyzerCache.length > ANALYZER_CACHE_SIZE) {
      analyzerCache.shift()
    }
    return { ...result, systemPrompt }
  } catch (error) {
    return { error: formatLlmError(error), systemPrompt }
  }
})

// eslint-disable-next-line @typescript-eslint/require-await
server.onLlmClearAnalyzerCache(async () => {
  analyzerCache.length = 0
})

server.onLlmGenerateTitle(async (buffer, settings) => {
  const messages: { role: 'user' | 'assistant' | 'system'; content: string }[] = [
    { role: 'system', content: settings.titleSystemPrompt },
    { role: 'user', content: buffer }
  ]
  try {
    const response = await completeChatCall(messages, {
      baseUrl: settings.baseUrl,
      apiKey: settings.apiKey,
      model: settings.model,
      reasoning: settings.reasoningEffort
    })
    const parsed = parseLlmJson(response)
    return { title: (parsed.title as string) || '', description: (parsed.description as string) || '', branchName: (parsed.branchName as string) || '', systemPrompt: settings.titleSystemPrompt }
  } catch (error) {
    return { error: formatLlmError(error), systemPrompt: settings.titleSystemPrompt }
  }
})

server.onLlmChatCancel((requestId) => {
  cancelChatStream(requestId)
})

server.onDaemonShutdown(async (connectionId) => {
  if (!connectionManager) {
    return { success: false, error: 'ConnectionManager not initialized' }
  }

  try {
    const client = connectionManager.getClient(connectionId)
    await client.shutdownDaemon()
    return { success: true }
  } catch (error) {
    const errorMessage = error instanceof Error ? error.message : 'Unknown error'
    console.error('[main] failed to shutdown daemon:', errorMessage)
    return { success: false, error: errorMessage }
  }
})

// Session IPC Handlers (workspace sessions)
server.onSessionUpdate(async (sessionId, workspaces, senderUuid, expectedVersion) => {
  if (!connectionManager) {
    return { success: false, error: 'ConnectionManager not initialized' }
  }

  // Route to the daemon that owns this session
  const connectionId = sessionConnectionMap.get(sessionId) ?? 'local'
  const client = connectionManager.getClient(connectionId)

  try {
    const result = await client.updateSession(workspaces, senderUuid, expectedVersion)
    return { success: true, session: result }
  } catch (error) {
    const errorMessage = error instanceof Error ? error.message : 'Unknown error'
    console.error('[main] failed to update session:', errorMessage)
    return { success: false, error: errorMessage }
  }
})

server.onClipboardWriteText((text) => { clipboard.writeText(text) })
server.onClipboardReadText(() => clipboard.readText())

server.onDialogSelectFolder(async () => {
  if (!mainWindow) return null
  const result = await dialog.showOpenDialog(mainWindow, {
    properties: ['openDirectory']
  })
  if (result.canceled || result.filePaths.length === 0) {
    return null
  }
  const selectedPath = result.filePaths[0]
  if (!selectedPath) return null
  // Add to recent directories
  try {
    const settings = loadSettings()
    const updatedSettings = addRecentDirectory(settings, selectedPath)
    saveSettings(updatedSettings)
  } catch (error) {
    console.error('[main] failed to save recent directory:', error)
  }
  return selectedPath
})

server.onDialogGetRecentDirectories(() => {
  const settings = loadSettings()
  return settings.recentDirectories
})

function getGitClientForConnection(connectionId: string): GitClient {
  return new GitClient(getClientForConnection(connectionId))
}

function getRunActionsClientForConnection(connectionId: string): RunActionsClient {
  return createRunActionsClient(getClientForConnection(connectionId))
}

// Git IPC Handlers - Now handled in main process via ExecStream
server.onGitGetInfo((connectionId, dirPath) => {
  return getGitClientForConnection(connectionId).getGitInfo(dirPath)
})

server.onGitCreateWorktree(async (connectionId, repoPath, name, baseBranch, operationId) => {
  try {
    const onProgress = operationId
      ? (data: string) => { server.gitOutput(operationId, data); }
      : undefined
    const result = await getGitClientForConnection(connectionId).createWorktree(repoPath, name, baseBranch, onProgress)
    return { success: true, path: result.path, branch: result.branch }
  } catch (error) {
    const errorMessage = error instanceof Error ? error.message : 'Failed to create worktree'
    console.error('[main] Failed to create worktree:', errorMessage)
    return { success: false, error: errorMessage }
  }
})

server.onGitRemoveWorktree(async (connectionId, repoPath, worktreePath, deleteBranch, operationId) => {
  const onProgress = operationId
    ? (data: string) => { server.gitOutput(operationId, data); }
    : undefined
  await getGitClientForConnection(connectionId).removeWorktree(repoPath, worktreePath, deleteBranch, onProgress)
  return { success: true }
})

server.onGitListWorktrees((connectionId, repoPath) => {
  return getGitClientForConnection(connectionId).listWorktrees(repoPath)
})

server.onGitListLocalBranches((connectionId, repoPath) => {
  return getGitClientForConnection(connectionId).listLocalBranches(repoPath)
})

server.onGitListRemoteBranches((connectionId, repoPath) => {
  return getGitClientForConnection(connectionId).listRemoteBranches(repoPath)
})

server.onGitGetBranchesInWorktrees((connectionId, repoPath) => {
  return getGitClientForConnection(connectionId).getBranchesInWorktrees(repoPath)
})

server.onGitCreateWorktreeFromBranch(async (connectionId, repoPath, branch, worktreeName, operationId) => {
  try {
    const onProgress = operationId
      ? (data: string) => { server.gitOutput(operationId, data); }
      : undefined
    const result = await getGitClientForConnection(connectionId).createWorktreeFromBranch(repoPath, branch, worktreeName, onProgress)
    return { success: true, path: result.path, branch: result.branch }
  } catch (error) {
    const errorMessage = error instanceof Error ? error.message : 'Failed to create worktree from branch'
    console.error('[main] Failed to create worktree from branch:', errorMessage)
    return { success: false, error: errorMessage }
  }
})

server.onGitCreateWorktreeFromRemote(async (connectionId, repoPath, remoteBranch, worktreeName, operationId) => {
  try {
    const onProgress = operationId
      ? (data: string) => { server.gitOutput(operationId, data); }
      : undefined
    const result = await getGitClientForConnection(connectionId).createWorktreeFromRemote(repoPath, remoteBranch, worktreeName, onProgress)
    return { success: true, path: result.path, branch: result.branch }
  } catch (error) {
    const errorMessage = error instanceof Error ? error.message : 'Failed to create worktree from remote branch'
    console.error('[main] Failed to create worktree from remote branch:', errorMessage)
    return { success: false, error: errorMessage }
  }
})

server.onGitGetDiff(async (connectionId, worktreePath, parentBranch) => {
  const result = await getGitClientForConnection(connectionId).getDiff(worktreePath, parentBranch)
  return { success: true, diff: result }
})

server.onGitGetFileDiff(async (connectionId, worktreePath, parentBranch, filePath) => {
  const diff = await getGitClientForConnection(connectionId).getFileDiff(worktreePath, parentBranch, filePath)
  return { success: true, diff }
})

server.onGitMerge(async (connectionId, targetWorktreePath, worktreeBranch, squash, operationId) => {
  const onProgress = operationId
    ? (data: string) => { server.gitOutput(operationId, data); }
    : undefined
  await getGitClientForConnection(connectionId).mergeWorktree(targetWorktreePath, worktreeBranch, squash, onProgress)
  return { success: true }
})

server.onGitCheckMergeConflicts(async (connectionId, repoPath, sourceBranch, targetBranch) => {
  const result = await getGitClientForConnection(connectionId).checkMergeConflicts(repoPath, sourceBranch, targetBranch)
  return {
    success: true,
    conflicts: {
      hasConflicts: result.hasConflicts,
      conflictedFiles: result.conflictedFiles,
      messages: result.messages
    }
  }
})

server.onGitHasUncommittedChanges((connectionId, repoPath) => {
  return getGitClientForConnection(connectionId).hasUncommittedChanges(repoPath)
})

server.onGitCommitAll(async (connectionId, repoPath, message) => {
  await getGitClientForConnection(connectionId).commitAll(repoPath, message)
  return { success: true }
})

server.onGitDeleteBranch(async (connectionId, repoPath, branchName, operationId) => {
  const onProgress = operationId
    ? (data: string) => { server.gitOutput(operationId, data); }
    : undefined
  await getGitClientForConnection(connectionId).deleteBranch(repoPath, branchName, false, onProgress)
  return { success: true }
})

server.onGitRenameBranch(async (connectionId, repoPath, oldName, newName) => {
  await getGitClientForConnection(connectionId).renameBranch(repoPath, oldName, newName)
  return { success: true }
})

server.onGitGetUncommittedChanges(async (connectionId, repoPath) => {
  const result = await getGitClientForConnection(connectionId).getUncommittedChanges(repoPath)
  return {
    success: true,
    changes: {
      files: result.files,
      totalAdditions: result.totalAdditions,
      totalDeletions: result.totalDeletions
    }
  }
})

server.onGitGetUncommittedFileDiff(async (connectionId, repoPath, filePath, staged) => {
  const diff = await getGitClientForConnection(connectionId).getUncommittedFileDiff(repoPath, filePath, staged)
  return { success: true, diff }
})

server.onGitStageFile(async (connectionId, repoPath, filePath) => {
  await getGitClientForConnection(connectionId).stageFile(repoPath, filePath)
  return { success: true }
})

server.onGitUnstageFile(async (connectionId, repoPath, filePath) => {
  await getGitClientForConnection(connectionId).unstageFile(repoPath, filePath)
  return { success: true }
})

server.onGitStageAll(async (connectionId, repoPath) => {
  await getGitClientForConnection(connectionId).stageAll(repoPath)
  return { success: true }
})

server.onGitUnstageAll(async (connectionId, repoPath) => {
  await getGitClientForConnection(connectionId).unstageAll(repoPath)
  return { success: true }
})

server.onGitCommitStaged(async (connectionId, repoPath, message) => {
  await getGitClientForConnection(connectionId).commitStaged(repoPath, message)
  return { success: true }
})

server.onGitGetFileContentsForDiff(async (connectionId, worktreePath, parentBranch, filePath) => {
  const result = await getGitClientForConnection(connectionId).getFileContentsForDiff(worktreePath, parentBranch, filePath)
  return {
    success: true,
    contents: {
      originalContent: result.originalContent,
      modifiedContent: result.modifiedContent,
      language: result.language
    }
  }
})

server.onGitGetUncommittedFileContentsForDiff(async (connectionId, repoPath, filePath, staged) => {
  const result = await getGitClientForConnection(connectionId).getUncommittedFileContentsForDiff(repoPath, filePath, staged)
  return {
    success: true,
    contents: {
      originalContent: result.originalContent,
      modifiedContent: result.modifiedContent,
      language: result.language
    }
  }
})

server.onGitGetHeadCommitHash(async (connectionId, repoPath) => {
  try {
    const hash = await getGitClientForConnection(connectionId).getHeadCommitHash(repoPath)
    return { success: true, hash }
  } catch (error) {
    return { success: false, error: error instanceof Error ? error.message : 'Unknown error' }
  }
})

server.onGitGetLog(async (connectionId, repoPath, parentBranch, skip, limit) => {
  try {
    const result = await getGitClientForConnection(connectionId).getLog(repoPath, parentBranch, skip, limit)
    return { success: true, result }
  } catch (error) {
    return { success: false, error: error instanceof Error ? error.message : 'Unknown error' }
  }
})

server.onGitGetCommitDiff(async (connectionId, repoPath, commitHash) => {
  try {
    const files = await getGitClientForConnection(connectionId).getCommitDiff(repoPath, commitHash)
    return { success: true, files }
  } catch (error) {
    return { success: false, error: error instanceof Error ? error.message : 'Unknown error' }
  }
})

server.onGitGetCommitFileDiff(async (connectionId, repoPath, commitHash, filePath) => {
  try {
    const contents = await getGitClientForConnection(connectionId).getCommitFileDiff(repoPath, commitHash, filePath)
    return { success: true, contents }
  } catch (error) {
    return { success: false, error: error instanceof Error ? error.message : 'Unknown error' }
  }
})

// Git fetch/pull IPC Handlers
server.onGitFetch(async (connectionId, repoPath) => {
  try {
    await getGitClientForConnection(connectionId).fetch(repoPath)
    return { success: true }
  } catch (error) {
    return { success: false, error: error instanceof Error ? error.message : 'Unknown error' }
  }
})

server.onGitPull(async (connectionId, repoPath) => {
  try {
    return await getGitClientForConnection(connectionId).pull(repoPath)
  } catch (error) {
    return { success: false, error: error instanceof Error ? error.message : 'Unknown error' }
  }
})

server.onGitGetBehindCount((connectionId, repoPath) => {
  return getGitClientForConnection(connectionId).getBehindCount(repoPath)
})

// GitHub IPC Handlers
server.onGitGetRemoteUrl(async (connectionId, repoPath) => {
  try {
    const url = await getGitClientForConnection(connectionId).getRemoteUrl(repoPath)
    return { success: true, url }
  } catch (error) {
    return { success: false, error: error instanceof Error ? error.message : 'Unknown error' }
  }
})

function execCommand(
  client: GrpcDaemonClient,
  cwd: string,
  command: string,
  args: string[]
): Promise<{ exitCode: number; stdout: string; stderr: string }> {
  return new Promise((resolve, reject) => {
    const stdout: Buffer[] = []
    const stderr: Buffer[] = []
    let resultReceived = false
    try {
      const stream = client.execStream()
      const startInput: ExecInput = {
        start: { cwd, command, args, env: {}, timeoutMs: 10000 }
      }
      stream.write(startInput)
      stream.end()
      stream.on('data', (output: ExecOutput) => {
        if (output.stdout) stdout.push(output.stdout.data)
        else if (output.stderr) stderr.push(output.stderr.data)
        else if (output.result) {
          resultReceived = true
          resolve({
            exitCode: output.result.exitCode,
            stdout: Buffer.concat(stdout).toString('utf-8'),
            stderr: Buffer.concat(stderr).toString('utf-8')
          })
        }
      })
      stream.on('error', (error: Error) => { reject(error); })
      stream.on('end', () => {
        if (!resultReceived) resolve({ exitCode: -1, stdout: '', stderr: 'Stream ended unexpectedly' })
      })
    } catch (error) {
      reject(error instanceof Error ? error : new Error(String(error)))
    }
  })
}

function parseGitHubOwnerRepo(remoteUrl: string): { owner: string; repo: string } | null {
  // Handle SSH: git@github.com:owner/repo.git
  const sshMatch = remoteUrl.match(/github\.com[:/]([^/]+)\/([^/]+?)(?:\.git)?$/)
  if (sshMatch?.[1] && sshMatch[2]) return { owner: sshMatch[1], repo: sshMatch[2] }
  // Handle HTTPS: https://github.com/owner/repo.git
  const httpsMatch = remoteUrl.match(/github\.com\/([^/]+)\/([^/]+?)(?:\.git)?$/)
  if (httpsMatch?.[1] && httpsMatch[2]) return { owner: httpsMatch[1], repo: httpsMatch[2] }
  return null
}

server.onGithubGetPrInfo(async (connectionId, repoPath, head, base) => {
  try {
    // Get GitHub token
    const settings = loadSettings()
    let token: string
    if (settings.github.autodetectViaGh) {
      const result = await execCommand(getClientForConnection(connectionId), repoPath, 'gh', ['auth', 'token'])
      if (result.exitCode !== 0) {
        return { error: 'Failed to get token from gh CLI. Is gh installed and authenticated?' }
      }
      token = result.stdout.trim()
    } else {
      token = settings.github.pat || ''
      if (!token) return { error: 'No GitHub PAT configured. Set one in Settings > GitHub.' }
    }

    // Get remote URL and parse owner/repo
    const remoteUrl = await getGitClientForConnection(connectionId).getRemoteUrl(repoPath)
    const parsed = parseGitHubOwnerRepo(remoteUrl)
    if (!parsed) return { error: `Could not parse GitHub owner/repo from remote URL: ${remoteUrl}` }
    const { owner, repo } = parsed

    // Search for existing PR via REST
    const { net } = await import('electron')
    const prResponse = await net.fetch(
      `https://api.github.com/repos/${owner}/${repo}/pulls?head=${owner}:${head}&base=${base}&state=open`,
      { headers: { Authorization: `Bearer ${token}`, Accept: 'application/vnd.github+json' } }
    )
    if (!prResponse.ok) {
      return { error: `GitHub API error: ${String(prResponse.status)} ${prResponse.statusText}` }
    }
    const prs = await prResponse.json() as Array<{ number: number; title: string }>

    if (prs.length === 0) {
      return { noPr: true as const, createUrl: `https://github.com/${owner}/${repo}/compare/${base}...${head}?expand=1` }
    }

    // eslint-disable-next-line @typescript-eslint/no-non-null-assertion -- length checked above
    const pr = prs[0]!
    const prUrl = `https://github.com/${owner}/${repo}/pull/${String(pr.number)}`

    // Fetch rich PR info via GraphQL
    const graphqlQuery = `query($owner: String!, $repo: String!, $prNumber: Int!) {
      repository(owner: $owner, name: $repo) {
        pullRequest(number: $prNumber) {
          state
          reviewThreads(first: 100) {
            nodes {
              isResolved
              comments(first: 1) {
                nodes {
                  body
                  path
                  line
                  author { login }
                }
              }
            }
          }
          latestReviews(first: 20) {
            nodes {
              author { login }
              state
            }
          }
          commits(last: 1) {
            nodes {
              commit {
                statusCheckRollup {
                  contexts(first: 50) {
                    nodes {
                      ... on CheckRun {
                        __typename
                        name
                        status
                        conclusion
                      }
                      ... on StatusContext {
                        __typename
                        context
                        state
                      }
                    }
                  }
                }
              }
            }
          }
        }
      }
    }`

    try {
      const graphqlResponse = await net.fetch('https://api.github.com/graphql', {
        method: 'POST',
        headers: {
          Authorization: `Bearer ${token}`,
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({
          query: graphqlQuery,
          variables: { owner, repo, prNumber: pr.number }
        })
      })

      if (!graphqlResponse.ok) {
        // Graceful degradation — return basic PR info
        return { prInfo: { number: pr.number, url: prUrl, title: pr.title, state: 'OPEN' as const, reviews: [], checkRuns: [], unresolvedThreads: [], unresolvedCount: 0 } }
      }

      const graphqlData = await graphqlResponse.json() as {
        data?: {
          repository?: {
            pullRequest?: {
              state?: string
              reviewThreads?: {
                nodes?: Array<{
                  isResolved: boolean
                  comments?: { nodes?: Array<{ body: string; path: string; line: number | null; author?: { login: string } }> }
                }>
              }
              latestReviews?: {
                nodes?: Array<{ author?: { login: string }; state: string }>
              }
              commits?: {
                nodes?: Array<{
                  commit?: {
                    statusCheckRollup?: {
                      contexts?: {
                        nodes?: Array<{
                          __typename: string
                          name?: string
                          status?: string
                          conclusion?: string | null
                          context?: string
                          state?: string
                        }>
                      }
                    }
                  }
                }>
              }
            }
          }
        }
      }

      const prData = graphqlData.data?.repository?.pullRequest
      const prState = (prData?.state ?? 'OPEN') as 'OPEN' | 'CLOSED' | 'MERGED'

      // Parse review threads
      const threads = prData?.reviewThreads?.nodes ?? []
      const unresolvedThreads = threads
        .filter(t => !t.isResolved)
        .map(t => {
          const firstComment = t.comments?.nodes?.[0]
          return {
            isResolved: false,
            path: firstComment?.path ?? '',
            body: firstComment?.body ?? '',
            author: firstComment?.author?.login ?? '',
            line: firstComment?.line ?? null,
          }
        })

      // Parse reviews
      const reviews = (prData?.latestReviews?.nodes ?? []).map(r => ({
        author: r.author?.login ?? '',
        state: r.state as 'APPROVED' | 'CHANGES_REQUESTED' | 'COMMENTED' | 'PENDING' | 'DISMISSED',
      }))

      // Parse check runs
      const commitNode = prData?.commits?.nodes?.[0]?.commit
      const contexts = commitNode?.statusCheckRollup?.contexts?.nodes ?? []
      const checkRuns = contexts
        .filter(c => c.__typename === 'CheckRun')
        .map(c => ({
          name: c.name ?? '',
          status: (c.status ?? 'QUEUED') as 'COMPLETED' | 'IN_PROGRESS' | 'QUEUED' | 'WAITING' | 'PENDING' | 'REQUESTED',
          conclusion: (c.conclusion ?? null) as 'SUCCESS' | 'FAILURE' | 'NEUTRAL' | 'CANCELLED' | 'TIMED_OUT' | 'ACTION_REQUIRED' | 'SKIPPED' | null,
        }))

      return {
        prInfo: {
          number: pr.number,
          url: prUrl,
          title: pr.title,
          state: prState,
          reviews,
          checkRuns,
          unresolvedThreads,
          unresolvedCount: unresolvedThreads.length,
        }
      }
    } catch {
      // GraphQL failed — graceful degradation
      return { prInfo: { number: pr.number, url: prUrl, title: pr.title, state: 'OPEN' as const, reviews: [], checkRuns: [], unresolvedThreads: [], unresolvedCount: 0 } }
    }
  } catch (error) {
    return { error: error instanceof Error ? error.message : 'Unknown error' }
  }
})

// Run Actions IPC Handlers
server.onRunActionsDetect((connectionId, workspacePath) => {
  return getRunActionsClientForConnection(connectionId).detect(workspacePath)
})

server.onRunActionsRun((connectionId, workspacePath, actionId) => {
  return getRunActionsClientForConnection(connectionId).run(workspacePath, actionId)
})

// Settings IPC Handlers
server.onSettingsLoad(() => {
  return loadSettings()
})

server.onSettingsSave((settings) => {
  saveSettings(settings)
  return { success: true }
})

// Filesystem IPC Handlers - All proxied to daemon
server.onFsReadDirectory((connectionId, workspacePath, dirPath) => {
  return getClientForConnection(connectionId).readDirectory(workspacePath, dirPath)
})

server.onFsReadFile((connectionId, workspacePath, filePath) => {
  return getClientForConnection(connectionId).readFile(workspacePath, filePath)
})

server.onFsWriteFile((connectionId, workspacePath, filePath, content) => {
  return getClientForConnection(connectionId).writeFile(workspacePath, filePath, content)
})

server.onFsSearchFiles((connectionId, workspacePath, query) => {
  return getClientForConnection(connectionId).searchFiles(workspacePath, query)
})

// Exec IPC Handlers
server.onExecStart((connectionId, cwd, command, args) => {
  try {
    const client = getClientForConnection(connectionId)
    const execId = randomUUID()
    const stream = client.execStream()
    execStreams.set(execId, stream)

    const startInput: ExecInput = {
      start: { cwd, command, args, env: {}, timeoutMs: 30000 }
    }
    stream.write(startInput)
    stream.end()

    stream.on('data', (output: ExecOutput) => {
      if (output.stdout) {
        server.execEvent(execId, { type: 'stdout', data: output.stdout.data.toString('utf-8') })
      } else if (output.stderr) {
        server.execEvent(execId, { type: 'stderr', data: output.stderr.data.toString('utf-8') })
      } else if (output.result) {
        server.execEvent(execId, { type: 'exit', exitCode: output.result.exitCode })
        execStreams.delete(execId)
      }
    })

    stream.on('error', (error) => {
      server.execEvent(execId, { type: 'error', message: error.message })
      execStreams.delete(execId)
    })

    stream.on('end', () => {
      execStreams.delete(execId)
    })

    return { success: true, execId }
  } catch (error) {
    return { success: false, error: error instanceof Error ? error.message : String(error) }
  }
})

server.onExecKill((execId) => {
  const stream = execStreams.get(execId)
  if (stream) {
    stream.cancel()
    execStreams.delete(execId)
  }
})

// Sandbox IPC Handlers
server.onSandboxIsAvailable(() => {
  if (process.platform === 'darwin') {
    return true // macOS always has sandbox-exec
  }
  if (process.platform === 'linux') {
    try {
      execSync('which bwrap', { stdio: 'ignore' })
      return true
    } catch {
      return false
    }
  }
  return false // Windows: no sandbox support
})

server.onAppGetInitialWorkspace(() => {
  const path = initialWorkspacePath
  initialWorkspacePath = null // Clear after first read
  return path
})

server.onAppGetWindowUuid((event) => {
  const windowInfo = windowManager.findWindowByWebContentsId(event.sender.id)
  return windowInfo?.uuid || ''
})

// Helper: sync port forwards for a connection to the saved connection in settings
function syncSavedPortForwards(connectionId: string): void {
  if (!connectionManager) return
  const settings = loadSettings()
  const saved = settings.ssh.savedConnections.find(c => c.id === connectionId)
  if (!saved) return
  const activeForwards = connectionManager.listPortForwards(connectionId)
  saved.portForwards = activeForwards.map(pf => ({
    localPort: pf.localPort,
    remoteHost: pf.remoteHost,
    remotePort: pf.remotePort
  }))
  saveSettings(settings)
}

// Helper: start port forwards from config and register watchers for a window
function autoStartPortForwards(
  config: SSHConnectionConfig,
  senderWindow: BrowserWindow
): void {
  if (!connectionManager || config.portForwards.length === 0) return
  const winId = senderWindow.id
  const windowInfo = windowManager.getWindow(winId)

  for (const spec of config.portForwards) {
    const pfConfig: PortForwardConfig = {
      id: randomUUID(),
      connectionId: config.id,
      localPort: spec.localPort,
      remoteHost: spec.remoteHost,
      remotePort: spec.remotePort,
    }

    try {
      connectionManager.addPortForward(pfConfig)

      const { unsubscribe } = connectionManager.watchPortForwardStatus(pfConfig.id, (pfInfo) => {
        if (windowInfo) {
          windowInfo.ipcServer.sshPortForwardStatus(pfInfo)
        }
      })
      if (!pfStatusWatchUnsubscribers.has(winId)) {
        pfStatusWatchUnsubscribers.set(winId, new Map())
      }
      pfStatusWatchUnsubscribers.get(winId)?.set(pfConfig.id, unsubscribe)
    } catch (err) {
      console.error(`[main:ssh] Failed to auto-start port forward ${String(spec.localPort)}:${spec.remoteHost}:${String(spec.remotePort)}:`, err)
    }
  }
}

// SSH IPC Handlers
server.onSshConnect(async (event, config, options) => {
  if (!connectionManager) throw new Error('ConnectionManager not initialized')

  console.log(`[main:ssh] onSshConnect called for host=${config.host}, id=${config.id}, refreshDaemon=${String(options?.refreshDaemon ?? false)}`)
  const info = await connectionManager.connectRemote(config, { refreshDaemon: options?.refreshDaemon })
  console.log(`[main:ssh] connectRemote returned status=${info.status}${info.status === 'error' ? `, error=${info.error}` : ''}`)

  // Switch the calling window to use the remote daemon
  if (info.status === 'connected') {
    const senderWindow = BrowserWindow.fromWebContents(event.sender)
    if (senderWindow) {
      // Load session from remote daemon and return it alongside connection info
      const remoteClient = connectionManager.getClient(config.id)
      try {
        console.log(`[main:ssh] Starting session watch for remote daemon`)
        const remoteWatch = remoteClient.watchSession(randomUUID(), (updatedSession) => {
          console.log(`[main:ssh] Session sync update received for session=${updatedSession.id}, workspaces=${String(updatedSession.workspaces.length)}`)
          const windowInfo = windowManager.getWindow(senderWindow.id)
          if (windowInfo) {
            windowInfo.ipcServer.sessionSync(config.id, updatedSession)
          }
        })
        const session = await remoteWatch.initial
        console.log(`[main:ssh] Initial session loaded: id=${session.id}, workspaces=${String(session.workspaces.length)}`)
        sessionConnectionMap.set(session.id, config.id)

        // Auto-start saved port forwards
        autoStartPortForwards(config, senderWindow)

        return { info, session }
      } catch (error) {
        const errorMsg = error instanceof Error ? error.message : String(error)
        console.error('[main:ssh] Failed to load remote session:', errorMsg)
        const isOldDaemon = errorMsg.includes('NOT_FOUND') && errorMsg.includes('session')
        const userError = isOldDaemon
          ? `Remote daemon is outdated. Retry with 'Refresh remote daemon' checked. (${errorMsg})`
          : `Connected but failed to load session: ${errorMsg}`
        return {
          info: { ...info, status: 'error' as const, error: userError },
          session: null
        }
      }
    } else {
      console.warn('[main:ssh] Could not find sender window')
    }
  }

  return { info, session: null }
})

// eslint-disable-next-line @typescript-eslint/require-await
server.onSshDisconnect(async (connectionId) => {
  if (!connectionManager) throw new Error('ConnectionManager not initialized')
  connectionManager.disconnectRemote(connectionId)
})

server.onSshListConnections(() => {
  if (!connectionManager) throw new Error('ConnectionManager not initialized')
  return connectionManager.listConnections()
})

// eslint-disable-next-line @typescript-eslint/require-await
server.onSshSaveConnection(async (config) => {
  const settings = loadSettings()
  const existing = settings.ssh.savedConnections.findIndex(
    c => c.host === config.host && c.user === config.user && c.port === config.port
  )
  if (existing >= 0) {
    settings.ssh.savedConnections[existing] = config
  } else {
    settings.ssh.savedConnections.push(config)
  }
  saveSettings(settings)
})

server.onSshGetSavedConnections(() => {
  const settings = loadSettings()
  return settings.ssh.savedConnections
})

// eslint-disable-next-line @typescript-eslint/require-await
server.onSshRemoveSavedConnection(async (id) => {
  const settings = loadSettings()
  settings.ssh.savedConnections = settings.ssh.savedConnections.filter(c => c.id !== id)
  saveSettings(settings)
})

server.onSshGetOutput((connectionId) => {
  if (!connectionManager) throw new Error('ConnectionManager not initialized')
  const tunnel = connectionManager.getSSHTunnel(connectionId)
  return tunnel?.getOutput() || []
})

// Per-window watch subscriptions
const outputWatchUnsubscribers = new Map<number, Map<string, () => void>>()
const statusWatchUnsubscribers = new Map<number, Map<string, () => void>>()

server.onSshWatchOutput((event, connectionId) => {
  if (!connectionManager) throw new Error('ConnectionManager not initialized')

  const senderWindow = BrowserWindow.fromWebContents(event.sender)
  if (!senderWindow) throw new Error('No window found for event sender')
  const winId = senderWindow.id

  // Clean up any existing watch for this window+connection
  outputWatchUnsubscribers.get(winId)?.get(connectionId)?.()

  const windowInfo = windowManager.getWindow(winId)
  const { scrollback, unsubscribe } = connectionManager.watchOutput(connectionId, (line) => {
    if (windowInfo) {
      windowInfo.ipcServer.sshOutput(connectionId, line)
    }
  })

  if (!outputWatchUnsubscribers.has(winId)) {
    outputWatchUnsubscribers.set(winId, new Map())
  }
  outputWatchUnsubscribers.get(winId)?.set(connectionId, unsubscribe)

  return { scrollback }
})

// eslint-disable-next-line @typescript-eslint/require-await
server.onSshUnwatchOutput(async (event, connectionId) => {
  const senderWindow = BrowserWindow.fromWebContents(event.sender)
  if (!senderWindow) return

  const winId = senderWindow.id
  outputWatchUnsubscribers.get(winId)?.get(connectionId)?.()
  outputWatchUnsubscribers.get(winId)?.delete(connectionId)
})

server.onSshWatchConnectionStatus((event, connectionId) => {
  if (!connectionManager) throw new Error('ConnectionManager not initialized')

  const senderWindow = BrowserWindow.fromWebContents(event.sender)
  if (!senderWindow) throw new Error('No window found for event sender')
  const winId = senderWindow.id

  // Clean up any existing watch for this window+connection
  statusWatchUnsubscribers.get(winId)?.get(connectionId)?.()

  const windowInfo = windowManager.getWindow(winId)
  const { initial, unsubscribe } = connectionManager.watchConnectionStatus(connectionId, (info) => {
    if (windowInfo) {
      windowInfo.ipcServer.sshConnectionStatus(info)
    }
  })

  if (!initial) throw new Error(`Connection not found: ${connectionId}`)

  if (!statusWatchUnsubscribers.has(winId)) {
    statusWatchUnsubscribers.set(winId, new Map())
  }
  statusWatchUnsubscribers.get(winId)?.set(connectionId, unsubscribe)

  return { initial }
})

// eslint-disable-next-line @typescript-eslint/require-await
server.onSshUnwatchConnectionStatus(async (event, connectionId) => {
  const senderWindow = BrowserWindow.fromWebContents(event.sender)
  if (!senderWindow) return

  const winId = senderWindow.id
  statusWatchUnsubscribers.get(winId)?.get(connectionId)?.()
  statusWatchUnsubscribers.get(winId)?.delete(connectionId)
})

// Port forward IPC handlers
const pfStatusWatchUnsubscribers = new Map<number, Map<string, () => void>>()

server.onSshAddPortForward((event, config) => {
  if (!connectionManager) throw new Error('ConnectionManager not initialized')
  const info = connectionManager.addPortForward(config)

  // Register a status watcher for the sender window
  const senderWindow = BrowserWindow.fromWebContents(event.sender)
  if (senderWindow) {
    const winId = senderWindow.id
    const windowInfo = windowManager.getWindow(winId)
    const { unsubscribe } = connectionManager.watchPortForwardStatus(config.id, (pfInfo) => {
      if (windowInfo) {
        windowInfo.ipcServer.sshPortForwardStatus(pfInfo)
      }
    })
    if (!pfStatusWatchUnsubscribers.has(winId)) {
      pfStatusWatchUnsubscribers.set(winId, new Map())
    }
    pfStatusWatchUnsubscribers.get(winId)?.set(config.id, unsubscribe)
  }

  // Sync saved connection with updated port forwards
  syncSavedPortForwards(config.connectionId)

  return info
})

// eslint-disable-next-line @typescript-eslint/require-await
server.onSshRemovePortForward(async (portForwardId) => {
  if (!connectionManager) return
  // Find the owning connection before removal so we can sync saved config
  let ownerConnectionId: string | undefined
  for (const connInfo of connectionManager.listConnections()) {
    if (connectionManager.listPortForwards(connInfo.id).some(pf => pf.id === portForwardId)) {
      ownerConnectionId = connInfo.id
      break
    }
  }

  connectionManager.removePortForward(portForwardId)

  if (ownerConnectionId) {
    syncSavedPortForwards(ownerConnectionId)
  }
})

server.onSshListPortForwards((connectionId) => {
  if (!connectionManager) throw new Error('ConnectionManager not initialized')
  return connectionManager.listPortForwards(connectionId)
})

// Per-window port forward watch subscriptions
const pfOutputWatchUnsubscribers = new Map<number, Map<string, () => void>>()

server.onSshWatchPortForwardOutput((event, portForwardId) => {
  if (!connectionManager) throw new Error('ConnectionManager not initialized')

  const senderWindow = BrowserWindow.fromWebContents(event.sender)
  if (!senderWindow) throw new Error('No window found for event sender')
  const winId = senderWindow.id

  pfOutputWatchUnsubscribers.get(winId)?.get(portForwardId)?.()

  const windowInfo = windowManager.getWindow(winId)
  const { scrollback, unsubscribe } = connectionManager.watchPortForwardOutput(portForwardId, (line) => {
    if (windowInfo) {
      windowInfo.ipcServer.sshPortForwardOutput(portForwardId, line)
    }
  })

  if (!pfOutputWatchUnsubscribers.has(winId)) {
    pfOutputWatchUnsubscribers.set(winId, new Map())
  }
  pfOutputWatchUnsubscribers.get(winId)?.set(portForwardId, unsubscribe)

  return { scrollback }
})

// eslint-disable-next-line @typescript-eslint/require-await
server.onSshUnwatchPortForwardOutput(async (event, portForwardId) => {
  const senderWindow = BrowserWindow.fromWebContents(event.sender)
  if (!senderWindow) return

  const winId = senderWindow.id
  pfOutputWatchUnsubscribers.get(winId)?.get(portForwardId)?.()
  pfOutputWatchUnsubscribers.get(winId)?.delete(portForwardId)
})

// App close confirmation IPC handlers
server.onAppCloseConfirmed((event) => {
  const windowInfo = windowManager.findWindowByWebContentsId(event.sender.id)
  if (windowInfo) {
    closeConfirmedWindows.add(event.sender.id)
    windowInfo.window.close()
  }
})

server.onAppCloseCancelled(() => {
  // No-op: closeConfirmedWindows only tracks confirmed windows
})

// App lifecycle
void app.whenReady().then(async () => {
  // Always use daemon mode
  console.log('[main] daemon mode enabled')

  // Show loading screen while connecting to daemon (skip in test mode)
  if (process.env.NODE_ENV !== 'test') {
    createLoadingWindow()
  }

  const localClient = new GrpcDaemonClient(process.env.TREETERM_SOCKET_PATH)

  // Forward daemon disconnections to renderer so the UI can show a warning
  localClient.onDisconnect(() => {
    server.daemonDisconnected()
  })

  // Proactively connect to daemon on startup
  await localClient.ensureDaemonRunning()

  // Create ConnectionManager wrapping local daemon client
  connectionManager = new ConnectionManager(localClient)

  // Close loading window and show main window
  if (loadingWindow) {
    loadingWindow.close()
    loadingWindow = null
  }

  mainWindow = createWindow()
  server.setWindow(mainWindow)
  createApplicationMenu(mainWindow, server, () => { void quitAndKillDaemon() })

  // Handle --ssh startup argument
  if (initialSSHTarget) {
    const parsed = parseSSHTarget(initialSSHTarget)
    if (parsed) {
      console.log('[main] Auto-connecting SSH:', initialSSHTarget)
      void connectionManager.connectRemote(parsed).then(async (info) => {
        if (info.status === 'connected') {
          console.log('[main] SSH connected:', info.id)
          if (mainWindow) {
            // Load session from remote daemon and re-initialize the renderer
            try {
              if (!connectionManager) throw new Error('ConnectionManager not initialized')
              const remoteClient = connectionManager.getClient(parsed.id)
              const windowId = mainWindow.id
              const remoteWatch = remoteClient.watchSession(randomUUID(), (updatedSession) => {
                const windowInfo = windowManager.getWindow(windowId)
                if (windowInfo) {
                  windowInfo.ipcServer.sessionSync(parsed.id, updatedSession)
                }
              })
              const session = await remoteWatch.initial
              sessionConnectionMap.set(session.id, parsed.id)
              const windowInfo = windowManager.getWindow(windowId)
              if (windowInfo) {
                windowInfo.ipcServer.sshAutoConnected(session, info)
              }
            } catch (error) {
              console.error('[main] Failed to load remote session:', error)
            }
          }
        } else {
          console.error('[main] SSH connection failed:', info.status === 'error' ? info.error : `status=${info.status}`)
        }
      }).catch((error: unknown) => {
        console.error('[main] SSH connection error:', error)
      })
    }
    initialSSHTarget = null
  }

  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0) {
      mainWindow = createWindow()
      server.setWindow(mainWindow)
      createApplicationMenu(mainWindow, server, () => { void quitAndKillDaemon() })
    }
  })
}).catch((error: unknown) => {
  console.error('[main] startup failed:', error)
  dialog.showErrorBox('Startup Error', `TreeTerm failed to start: ${error instanceof Error ? error.message : String(error)}`)
  app.quit()
})

  app.on('window-all-closed', () => {
  app.quit()
})

// Parse SSH target string like "user@host" or "user@host:port"
function parseSSHTarget(target: string): SSHConnectionConfig | null {
  const match = target.match(/^([^@]+)@([^:]+)(?::(\d+))?$/)
  if (!match) return null
  // eslint-disable-next-line @typescript-eslint/no-non-null-assertion -- regex groups guaranteed by match
  const user = match[1]!
  // eslint-disable-next-line @typescript-eslint/no-non-null-assertion -- regex groups guaranteed by match
  const host = match[2]!
  return {
    id: `ssh-${host}-${String(Date.now())}`,
    user,
    host,
    port: match[3] ? parseInt(match[3], 10) : 22,
    label: target,
    portForwards: [],
  }
}

async function quitAndKillDaemon(): Promise<void> {
  if (connectionManager) {
    try {
      console.log('[main] shutting down daemon before quit')
      await connectionManager.getClient('local').shutdownDaemon()
    } catch (error) {
      const errorMessage = error instanceof Error ? error.message : 'Unknown error'
      console.error('[main] failed to shutdown daemon:', errorMessage)
    }
  }
  app.quit()
}

app.on('before-quit', () => {
  if (connectionManager) {
    // Disconnect all remote SSH connections
    connectionManager.disconnectAll()
    const localClient = connectionManager.getClient('local')
    if (localClient.isConnected()) {
      localClient.disconnect()
    }
  }
})
