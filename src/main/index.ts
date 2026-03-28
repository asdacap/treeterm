import { app, BrowserWindow, clipboard, dialog, shell, ipcMain } from 'electron'
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
import type { ReasoningEffort, SandboxConfig, SSHConnectionConfig } from '../shared/types'

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
import { loadSettings, saveSettings, Settings, addRecentDirectory } from './settings'
import { createApplicationMenu } from './menu'
import { registerSTTHandlers } from './stt'
import { startChatStream, cancelChatStream, completeChatCall, formatLlmError, parseLlmJson } from './llm'

let mainWindow: BrowserWindow | null = null
let loadingWindow: BrowserWindow | null = null
const closeConfirmedWindows: Set<number> = new Set()
let daemonClient: GrpcDaemonClient | null = null
let connectionManager: ConnectionManager | null = null
let gitClient: GitClient | null = null
let runActionsClient: RunActionsClient | null = null
let useDaemon = true // Always use daemon mode

// Simple object storage — each entry is an independent terminal's stream.
const ptyStreams = new Map<string, PtyStream>()

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

  loadingWindow.loadFile(join(__dirname, 'loading.html'))

  loadingWindow.once('ready-to-show', () => {
    if (!isTest) {
      loadingWindow?.show()
    }
  })

  return loadingWindow
}

function createWindow(initialSessionId?: string): BrowserWindow {
  const isTest = process.env.NODE_ENV === 'test'

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

  // Handle media permissions for speech recognition and microphone
  window.webContents.session.setPermissionRequestHandler((webContents, permission, callback) => {
    if (permission === 'media') {
      // Always allow microphone access for push-to-talk
      callback(true)
    } else {
      callback(false)
    }
  })

  // Forward all keyboard events including Caps Lock to renderer
  window.webContents.on('before-input-event', (event, input) => {
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
    shell.openExternal(url)
  })

  window.webContents.setWindowOpenHandler(({ url }) => {
    if (url && url !== 'about:blank') {
      shell.openExternal(url)
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

  // Build the load URL with sessionId query parameter if provided
  let loadUrl: string
  if (process.env.ELECTRON_RENDERER_URL) {
    const url = new URL(process.env.ELECTRON_RENDERER_URL)
    if (initialSessionId) {
      url.searchParams.set('sessionId', initialSessionId)
    }
    loadUrl = url.toString()
  } else {
    const basePath = join(__dirname, '../renderer/index.html')
    if (initialSessionId) {
      loadUrl = `file://${basePath}?sessionId=${encodeURIComponent(initialSessionId)}`
    } else {
      loadUrl = `file://${basePath}`
    }
  }

  // Load the renderer
  window.loadURL(loadUrl)

  // Signal renderer when ready to initialize with the session
  window.webContents.on('did-finish-load', async () => {
    if (!daemonClient) {
      windowServer.appReady(null)
      return
    }

    try {
      await daemonClient.ensureDaemonRunning()

      let sessionId = initialSessionId
      if (!sessionId) {
        sessionId = await daemonClient.getDefaultSessionId()
        console.log('[main] got default session id:', sessionId)
      }

      // Start watching this session for changes from other windows (cancel previous if HMR reload)
      if (unwatchSession) {
        unwatchSession()
      }
      const watch = daemonClient.watchSession(sessionId, windowUuid, (updatedSession) => {
        console.log('[main] session sync received for window', window.id, {
          sessionId: updatedSession.id,
          workspaces: updatedSession.workspaces.map(ws => ({ path: ws.path, metadata: ws.metadata })),
        })
        windowServer.sessionSync(updatedSession)
      })
      unwatchSession = watch.unsubscribe

      try {
        const session = await watch.initial
        console.log('[main] loaded session:', session.id)
        windowServer.appReady(session)
      } catch {
        // Session not found (e.g. stale initialSessionId), fall back to default
        unwatchSession()
        const defaultId = await daemonClient.getDefaultSessionId()
        const fallbackWatch = daemonClient.watchSession(defaultId, windowUuid, (updatedSession) => {
          windowServer.sessionSync(updatedSession)
        })
        unwatchSession = fallbackWatch.unsubscribe
        const session = await fallbackWatch.initial
        console.log('[main] session not found, using default:', session.id)
        windowServer.appReady(session)
      }
    } catch (error) {
      console.error('[main] failed to get session:', error)
      windowServer.appReady(null)
    }
  })

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
// PTY create/attach use ipcMain.handle directly to get event.sender for routing PTY data to the correct window
ipcMain.handle('pty:create', async (event, connectionId: string, cwd: string, sandbox?: unknown, startupCommand?: string) => {
  if (!daemonClient) throw new Error('Daemon not initialized')

  try {
    const client = getClientForConnection(connectionId)
    await client.ensureDaemonRunning()
    const sessionId = await client.createPtySession({ cwd, sandbox: sandbox as SandboxConfig | undefined, startupCommand })
    const ptyStream = client.openPtyStream(sessionId, (evt) => {
      event.sender.send('pty:event', ptyStream.handle, evt)
      if (evt.type === 'exit') ptyStreams.delete(ptyStream.handle)
    })
    ptyStreams.set(ptyStream.handle, ptyStream)

    return { sessionId, handle: ptyStream.handle }
  } catch (error) {
    console.error('[main] failed to create PTY session via daemon:', error)
    return null
  }
})

ipcMain.handle('pty:attach', async (_event, connectionId: string, sessionId: string) => {
  if (!daemonClient) {
    return { success: false, error: 'Daemon not initialized' }
  }

  try {
    const client = getClientForConnection(connectionId)
    await client.ensureDaemonRunning()
    const ptyStream = client.openPtyStream(sessionId, (evt) => {
      _event.sender.send('pty:event', ptyStream.handle, evt)
      if (evt.type === 'exit') ptyStreams.delete(ptyStream.handle)
    })
    ptyStreams.set(ptyStream.handle, ptyStream)

    return { success: true, handle: ptyStream.handle }
  } catch (error) {
    const errorMessage = error instanceof Error ? error.message : 'Unknown error'
    console.error('[main] failed to attach to PTY session:', errorMessage)
    return { success: false, error: errorMessage }
  }
})

ipcMain.handle('pty:list', async (_event, connectionId: string) => {
  try {
    const client = getClientForConnection(connectionId)
    await client.ensureDaemonRunning()
    return await client.listPtySessions()
  } catch (error) {
    console.error('[main] failed to list sessions:', error)
    return []
  }
})

ipcMain.on('pty:write', (_event, handle: string, data: string) => {
  ptyStreams.get(handle)?.write(data)
})

ipcMain.on('pty:resize', (_event, handle: string, cols: number, rows: number) => {
  ptyStreams.get(handle)?.resize(cols, rows)
})

ipcMain.on('pty:kill', (_event, connectionId: string, sessionId: string) => {
  // Close any PtyStreams for this session
  for (const [handle, stream] of ptyStreams) {
    if (stream.sessionId === sessionId) {
      stream.close()
      ptyStreams.delete(handle)
    }
  }
  try {
    const client = getClientForConnection(connectionId)
    client.killPtySession(sessionId).catch(error => {
      console.error('[main] failed to kill PTY:', error)
    })
  } catch (error) {
    console.error('[main] failed to kill PTY:', error)
  }
})

ipcMain.handle('pty:isAlive', async (_event, connectionId: string, id: string) => {
  try {
    const client = getClientForConnection(connectionId)
    const sessions = await client.listPtySessions()
    return sessions.some(s => s.id === id)
  } catch (error) {
    console.warn('[main] PTY alive check failed:', error)
    return false
  }
})

// LLM chat — uses ipcMain.handle directly for event.sender access (like PTY)
ipcMain.handle('llm:chat:send', async (event, requestId: string, messages: { role: 'user' | 'assistant' | 'system'; content: string }[], settings: { baseUrl: string; apiKey: string; model: string; reasoning: ReasoningEffort }) => {
  await startChatStream(requestId, messages, settings, event.sender)
})

// Terminal analyzer — non-streaming LLM call with buffer cache
const analyzerCache: { buffer: string; result: { state: string; reason: string } }[] = []
const ANALYZER_CACHE_SIZE = 10

ipcMain.handle('llm:analyzeTerminal', async (_event, buffer: string, cwd: string, settings: { baseUrl: string; apiKey: string; model: string; systemPrompt: string; reasoningEffort: ReasoningEffort; safePaths: string[] }) => {
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
    const result = { state: parsed.state as string, reason: (parsed.reason as string) ?? '' }
    analyzerCache.push({ buffer, result })
    if (analyzerCache.length > ANALYZER_CACHE_SIZE) {
      analyzerCache.shift()
    }
    return { ...result, systemPrompt }
  } catch (error) {
    return { error: formatLlmError(error), systemPrompt }
  }
})

ipcMain.handle('llm:clearAnalyzerCache', () => {
  analyzerCache.length = 0
})

ipcMain.handle('llm:generateTitle', async (_event, buffer: string, settings: { baseUrl: string; apiKey: string; model: string; titleSystemPrompt: string; reasoningEffort: ReasoningEffort }) => {
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

server.onDaemonShutdown(async () => {
  if (!daemonClient) {
    return { success: false, error: 'Daemon not initialized' }
  }

  try {
    await daemonClient.shutdownDaemon()
    return { success: true }
  } catch (error) {
    const errorMessage = error instanceof Error ? error.message : 'Unknown error'
    console.error('[main] failed to shutdown daemon:', errorMessage)
    return { success: false, error: errorMessage }
  }
})

// Session IPC Handlers (workspace sessions)
server.onSessionCreate(async (workspaces) => {
  if (!useDaemon || !daemonClient) {
    return { success: false, error: 'Daemon not enabled' }
  }

  try {
    await daemonClient.ensureDaemonRunning()
    const result = await daemonClient.createSession(workspaces)
    console.log('[main] session created:', result?.id)
    return { success: true, session: result }
  } catch (error) {
    const errorMessage = error instanceof Error ? error.message : 'Unknown error'
    console.error('[main] failed to create session:', errorMessage)
    return { success: false, error: errorMessage }
  }
})

server.onSessionUpdate(async (sessionId, workspaces, senderUuid) => {
  if (!useDaemon || !daemonClient) {
    return { success: false, error: 'Daemon not enabled' }
  }

  try {
    await daemonClient.ensureDaemonRunning()
    const result = await daemonClient.updateSession(sessionId, workspaces, senderUuid)
    return { success: true, session: result }
  } catch (error) {
    const errorMessage = error instanceof Error ? error.message : 'Unknown error'
    console.error('[main] failed to update session:', errorMessage)
    return { success: false, error: errorMessage }
  }
})

server.onSessionList(async () => {
  if (!useDaemon || !daemonClient) {
    return { success: true, sessions: [] }
  }

  try {
    await daemonClient.ensureDaemonRunning()
    const sessions = await daemonClient.listSessions()
    console.log('[main] listed sessions:', sessions.length)
    return { success: true, sessions }
  } catch (error) {
    const errorMessage = error instanceof Error ? error.message : 'Unknown error'
    console.error('[main] failed to list sessions:', errorMessage)
    return { success: false, error: errorMessage }
  }
})

server.onSessionDelete(async (sessionId) => {
  if (!useDaemon || !daemonClient) {
    return { success: false, error: 'Daemon not enabled' }
  }

  try {
    await daemonClient.deleteSession(sessionId)
    return { success: true }
  } catch (error) {
    const errorMessage = error instanceof Error ? error.message : 'Unknown error'
    console.error('[main] failed to delete session:', errorMessage)
    return { success: false, error: errorMessage }
  }
})

server.onSessionOpenInNewWindow(async (sessionId) => {
  if (!useDaemon || !daemonClient) {
    return { success: false, error: 'Daemon not enabled' }
  }

  try {
    // Verify the session exists
    const sessions = await daemonClient.listSessions()
    if (!sessions.some(s => s.id === sessionId)) {
      return { success: false, error: 'Session not found' }
    }

    // Create new window with the session
    createWindow(sessionId)
    console.log('[main] opened session in new window:', sessionId)
    return { success: true }
  } catch (error) {
    const errorMessage = error instanceof Error ? error.message : 'Unknown error'
    console.error('[main] failed to open session in new window:', errorMessage)
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

server.onDialogGetRecentDirectories(async () => {
  try {
    const settings = loadSettings()
    return settings.recentDirectories || []
  } catch (error) {
    console.error('[main] failed to load recent directories:', error)
    return []
  }
})

// Initialize git client when daemon is ready
function initializeGitClient(): void {
  if (daemonClient && !gitClient) {
    gitClient = new GitClient(daemonClient)
  }
}

// Initialize run actions client when daemon is ready
function initializeRunActionsClient(): void {
  if (daemonClient && !runActionsClient) {
    runActionsClient = createRunActionsClient(daemonClient)
  }
}

// Git IPC Handlers - Now handled in main process via ExecStream
server.onGitGetInfo(async (dirPath) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  initializeGitClient()
  if (!gitClient) throw new Error('Git client not initialized')
  return gitClient.getGitInfo(dirPath)
})

server.onGitCreateWorktree(async (repoPath, name, baseBranch, operationId) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  initializeGitClient()
  if (!gitClient) throw new Error('Git client not initialized')
  try {
    const onProgress = operationId
      ? (data: string) => server.gitOutput(operationId, data)
      : undefined
    const result = await gitClient.createWorktree(repoPath, name, baseBranch, onProgress)
    return { success: true, path: result.path, branch: result.branch }
  } catch (error) {
    const errorMessage = error instanceof Error ? error.message : 'Failed to create worktree'
    console.error('[main] Failed to create worktree:', errorMessage)
    return { success: false, error: errorMessage }
  }
})

server.onGitRemoveWorktree(async (repoPath, worktreePath, deleteBranch, operationId) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  initializeGitClient()
  if (!gitClient) throw new Error('Git client not initialized')
  const onProgress = operationId
    ? (data: string) => server.gitOutput(operationId, data)
    : undefined
  await gitClient.removeWorktree(repoPath, worktreePath, deleteBranch, onProgress)
  return { success: true }
})

server.onGitListWorktrees(async (repoPath) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  initializeGitClient()
  if (!gitClient) throw new Error('Git client not initialized')
  return gitClient.listWorktrees(repoPath)
})

server.onGitListLocalBranches(async (repoPath) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  initializeGitClient()
  if (!gitClient) throw new Error('Git client not initialized')
  return gitClient.listLocalBranches(repoPath)
})

server.onGitListRemoteBranches(async (repoPath) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  initializeGitClient()
  if (!gitClient) throw new Error('Git client not initialized')
  return gitClient.listRemoteBranches(repoPath)
})

server.onGitGetBranchesInWorktrees(async (repoPath) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  initializeGitClient()
  if (!gitClient) throw new Error('Git client not initialized')
  return gitClient.getBranchesInWorktrees(repoPath)
})

server.onGitCreateWorktreeFromBranch(async (repoPath, branch, worktreeName, operationId) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  initializeGitClient()
  if (!gitClient) throw new Error('Git client not initialized')
  try {
    const onProgress = operationId
      ? (data: string) => server.gitOutput(operationId, data)
      : undefined
    const result = await gitClient.createWorktreeFromBranch(repoPath, branch, worktreeName, onProgress)
    return { success: true, path: result.path, branch: result.branch }
  } catch (error) {
    const errorMessage = error instanceof Error ? error.message : 'Failed to create worktree from branch'
    console.error('[main] Failed to create worktree from branch:', errorMessage)
    return { success: false, error: errorMessage }
  }
})

server.onGitCreateWorktreeFromRemote(async (repoPath, remoteBranch, worktreeName, operationId) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  initializeGitClient()
  if (!gitClient) throw new Error('Git client not initialized')
  try {
    const onProgress = operationId
      ? (data: string) => server.gitOutput(operationId, data)
      : undefined
    const result = await gitClient.createWorktreeFromRemote(repoPath, remoteBranch, worktreeName, onProgress)
    return { success: true, path: result.path, branch: result.branch }
  } catch (error) {
    const errorMessage = error instanceof Error ? error.message : 'Failed to create worktree from remote branch'
    console.error('[main] Failed to create worktree from remote branch:', errorMessage)
    return { success: false, error: errorMessage }
  }
})

server.onGitGetDiff(async (worktreePath, parentBranch) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  initializeGitClient()
  if (!gitClient) throw new Error('Git client not initialized')
  const result = await gitClient.getDiff(worktreePath, parentBranch)
  return { success: true, diff: result }
})

server.onGitGetFileDiff(async (worktreePath, parentBranch, filePath) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  initializeGitClient()
  if (!gitClient) throw new Error('Git client not initialized')
  const diff = await gitClient.getFileDiff(worktreePath, parentBranch, filePath)
  return { success: true, diff }
})

server.onGitMerge(async (targetWorktreePath, worktreeBranch, squash, operationId) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  initializeGitClient()
  if (!gitClient) throw new Error('Git client not initialized')
  const onProgress = operationId
    ? (data: string) => server.gitOutput(operationId, data)
    : undefined
  await gitClient.mergeWorktree(targetWorktreePath, worktreeBranch, squash, onProgress)
  return { success: true }
})

server.onGitCheckMergeConflicts(async (repoPath, sourceBranch, targetBranch) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  initializeGitClient()
  if (!gitClient) throw new Error('Git client not initialized')
  const result = await gitClient.checkMergeConflicts(repoPath, sourceBranch, targetBranch)
  return { 
    success: true, 
    conflicts: {
      hasConflicts: result.hasConflicts,
      conflictedFiles: result.conflictedFiles,
      messages: result.messages
    }
  }
})

server.onGitHasUncommittedChanges(async (repoPath) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  initializeGitClient()
  if (!gitClient) throw new Error('Git client not initialized')
  return gitClient.hasUncommittedChanges(repoPath)
})

server.onGitCommitAll(async (repoPath, message) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  initializeGitClient()
  if (!gitClient) throw new Error('Git client not initialized')
  const hash = await gitClient.commitAll(repoPath, message)
  return { success: true, hash }
})

server.onGitDeleteBranch(async (repoPath, branchName, operationId) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  initializeGitClient()
  if (!gitClient) throw new Error('Git client not initialized')
  const onProgress = operationId
    ? (data: string) => server.gitOutput(operationId, data)
    : undefined
  await gitClient.deleteBranch(repoPath, branchName, false, onProgress)
  return { success: true }
})

server.onGitRenameBranch(async (repoPath, oldName, newName) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  initializeGitClient()
  if (!gitClient) throw new Error('Git client not initialized')
  await gitClient.renameBranch(repoPath, oldName, newName)
  return { success: true }
})

server.onGitGetUncommittedChanges(async (repoPath) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  initializeGitClient()
  if (!gitClient) throw new Error('Git client not initialized')
  const result = await gitClient.getUncommittedChanges(repoPath)
  return { 
    success: true, 
    changes: {
      files: result.files,
      totalAdditions: result.totalAdditions,
      totalDeletions: result.totalDeletions
    }
  }
})

server.onGitGetUncommittedFileDiff(async (repoPath, filePath, staged) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  initializeGitClient()
  if (!gitClient) throw new Error('Git client not initialized')
  const diff = await gitClient.getUncommittedFileDiff(repoPath, filePath, staged)
  return { success: true, diff }
})

server.onGitStageFile(async (repoPath, filePath) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  initializeGitClient()
  if (!gitClient) throw new Error('Git client not initialized')
  await gitClient.stageFile(repoPath, filePath)
  return { success: true }
})

server.onGitUnstageFile(async (repoPath, filePath) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  initializeGitClient()
  if (!gitClient) throw new Error('Git client not initialized')
  await gitClient.unstageFile(repoPath, filePath)
  return { success: true }
})

server.onGitStageAll(async (repoPath) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  initializeGitClient()
  if (!gitClient) throw new Error('Git client not initialized')
  await gitClient.stageAll(repoPath)
  return { success: true }
})

server.onGitUnstageAll(async (repoPath) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  initializeGitClient()
  if (!gitClient) throw new Error('Git client not initialized')
  await gitClient.unstageAll(repoPath)
  return { success: true }
})

server.onGitCommitStaged(async (repoPath, message) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  initializeGitClient()
  if (!gitClient) throw new Error('Git client not initialized')
  const hash = await gitClient.commitStaged(repoPath, message)
  return { success: true, hash }
})

server.onGitGetFileContentsForDiff(async (worktreePath, parentBranch, filePath) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  initializeGitClient()
  if (!gitClient) throw new Error('Git client not initialized')
  const result = await gitClient.getFileContentsForDiff(worktreePath, parentBranch, filePath)
  return { 
    success: true, 
    contents: {
      originalContent: result.originalContent,
      modifiedContent: result.modifiedContent,
      language: result.language
    }
  }
})

server.onGitGetUncommittedFileContentsForDiff(async (repoPath, filePath, staged) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  initializeGitClient()
  if (!gitClient) throw new Error('Git client not initialized')
  const result = await gitClient.getUncommittedFileContentsForDiff(repoPath, filePath, staged)
  return { 
    success: true, 
    contents: {
      originalContent: result.originalContent,
      modifiedContent: result.modifiedContent,
      language: result.language
    }
  }
})

server.onGitGetHeadCommitHash(async (repoPath) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  initializeGitClient()
  if (!gitClient) throw new Error('Git client not initialized')
  const hash = await gitClient.getHeadCommitHash(repoPath)
  return { success: true, hash }
})

server.onGitGetLog(async (repoPath, parentBranch, skip, limit) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  initializeGitClient()
  if (!gitClient) throw new Error('Git client not initialized')
  try {
    const result = await gitClient.getLog(repoPath, parentBranch, skip, limit)
    return { success: true, result }
  } catch (error) {
    return { success: false, error: error instanceof Error ? error.message : 'Unknown error' }
  }
})

server.onGitGetCommitDiff(async (repoPath, commitHash) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  initializeGitClient()
  if (!gitClient) throw new Error('Git client not initialized')
  try {
    const files = await gitClient.getCommitDiff(repoPath, commitHash)
    return { success: true, files }
  } catch (error) {
    return { success: false, error: error instanceof Error ? error.message : 'Unknown error' }
  }
})

server.onGitGetCommitFileDiff(async (repoPath, commitHash, filePath) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  initializeGitClient()
  if (!gitClient) throw new Error('Git client not initialized')
  try {
    const contents = await gitClient.getCommitFileDiff(repoPath, commitHash, filePath)
    return { success: true, contents }
  } catch (error) {
    return { success: false, error: error instanceof Error ? error.message : 'Unknown error' }
  }
})

// Git fetch/pull IPC Handlers
server.onGitFetch(async (repoPath) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  initializeGitClient()
  if (!gitClient) throw new Error('Git client not initialized')
  try {
    await gitClient.fetch(repoPath)
    return { success: true }
  } catch (error) {
    return { success: false, error: error instanceof Error ? error.message : 'Unknown error' }
  }
})

server.onGitPull(async (repoPath) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  initializeGitClient()
  if (!gitClient) throw new Error('Git client not initialized')
  try {
    return await gitClient.pull(repoPath)
  } catch (error) {
    return { success: false, error: error instanceof Error ? error.message : 'Unknown error' }
  }
})

server.onGitGetBehindCount(async (repoPath) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  initializeGitClient()
  if (!gitClient) throw new Error('Git client not initialized')
  return await gitClient.getBehindCount(repoPath)
})

// GitHub IPC Handlers
server.onGitGetRemoteUrl(async (repoPath) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  initializeGitClient()
  if (!gitClient) throw new Error('Git client not initialized')
  try {
    const url = await gitClient.getRemoteUrl(repoPath)
    return { url }
  } catch (error) {
    return { error: error instanceof Error ? error.message : 'Unknown error' }
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
      stream.on('error', (error: Error) => reject(error))
      stream.on('end', () => {
        if (!resultReceived) resolve({ exitCode: -1, stdout: '', stderr: 'Stream ended unexpectedly' })
      })
    } catch (error) {
      reject(error)
    }
  })
}

function parseGitHubOwnerRepo(remoteUrl: string): { owner: string; repo: string } | null {
  // Handle SSH: git@github.com:owner/repo.git
  const sshMatch = remoteUrl.match(/github\.com[:/]([^/]+)\/([^/]+?)(?:\.git)?$/)
  if (sshMatch) return { owner: sshMatch[1], repo: sshMatch[2] }
  // Handle HTTPS: https://github.com/owner/repo.git
  const httpsMatch = remoteUrl.match(/github\.com\/([^/]+)\/([^/]+?)(?:\.git)?$/)
  if (httpsMatch) return { owner: httpsMatch[1], repo: httpsMatch[2] }
  return null
}

server.onGithubGetPrInfo(async (repoPath, head, base) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  initializeGitClient()
  if (!gitClient) throw new Error('Git client not initialized')
  try {
    // Get GitHub token
    const settings = loadSettings()
    let token: string
    if (settings.github?.autodetectViaGh !== false) {
      const result = await execCommand(daemonClient, repoPath, 'gh', ['auth', 'token'])
      if (result.exitCode !== 0) {
        return { error: 'Failed to get token from gh CLI. Is gh installed and authenticated?' }
      }
      token = result.stdout.trim()
    } else {
      token = settings.github?.pat || ''
      if (!token) return { error: 'No GitHub PAT configured. Set one in Settings > GitHub.' }
    }

    // Get remote URL and parse owner/repo
    const remoteUrl = await gitClient.getRemoteUrl(repoPath)
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
      return { error: `GitHub API error: ${prResponse.status} ${prResponse.statusText}` }
    }
    const prs = await prResponse.json() as Array<{ number: number; title: string }>

    if (prs.length === 0) {
      return { noPr: true as const, createUrl: `https://github.com/${owner}/${repo}/compare/${base}...${head}?expand=1` }
    }

    const pr = prs[0]
    const prUrl = `https://github.com/${owner}/${repo}/pull/${pr.number}`

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
server.onRunActionsDetect(async (workspacePath) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  initializeRunActionsClient()
  if (!runActionsClient) throw new Error('RunActions client not initialized')
  return runActionsClient.detect(workspacePath)
})

server.onRunActionsRun(async (workspacePath, actionId) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  initializeRunActionsClient()
  if (!runActionsClient) throw new Error('RunActions client not initialized')
  return runActionsClient.run(workspacePath, actionId)
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
server.onFsReadDirectory(async (workspacePath, dirPath) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  return daemonClient.readDirectory(workspacePath, dirPath)
})

server.onFsReadFile(async (workspacePath, filePath) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  return daemonClient.readFile(workspacePath, filePath)
})

server.onFsWriteFile(async (workspacePath, filePath, content) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  return daemonClient.writeFile(workspacePath, filePath, content)
})

server.onFsSearchFiles(async (workspacePath, query) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  return daemonClient.searchFiles(workspacePath, query)
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

// SSH IPC Handlers
server.onSshConnect(async (event, config, options) => {
  if (!connectionManager) throw new Error('ConnectionManager not initialized')

  console.log(`[main:ssh] onSshConnect called for host=${config.host}, id=${config.id}, refreshDaemon=${options?.refreshDaemon ?? false}`)
  const info = await connectionManager.connectRemote(config, { refreshDaemon: options?.refreshDaemon })
  console.log(`[main:ssh] connectRemote returned status=${info.status}${info.error ? `, error=${info.error}` : ''}`)

  // Switch the calling window to use the remote daemon
  if (info.status === 'connected') {
    const senderWindow = BrowserWindow.fromWebContents(event.sender)
    if (senderWindow) {
      // Load session from remote daemon and return it alongside connection info
      const remoteClient = connectionManager.getClient(config.id)
      try {
        console.log(`[main:ssh] Fetching default session ID from remote daemon...`)
        const remoteSessionId = await remoteClient.getDefaultSessionId()
        console.log(`[main:ssh] Got remote session ID: ${remoteSessionId}`)

        console.log(`[main:ssh] Starting session watch for session=${remoteSessionId}`)
        const remoteWatch = remoteClient.watchSession(remoteSessionId, randomUUID(), (updatedSession) => {
          console.log(`[main:ssh] Session sync update received for session=${updatedSession.id}, workspaces=${updatedSession.workspaces?.length ?? 0}`)
          const windowInfo = windowManager.getWindow(senderWindow.id)
          if (windowInfo) {
            windowInfo.ipcServer.sessionSync(updatedSession)
          }
        })
        const session = await remoteWatch.initial
        console.log(`[main:ssh] Initial session loaded: id=${session.id}, workspaces=${session.workspaces?.length ?? 0}`)
        return { info, session }
      } catch (error) {
        const errorMsg = error instanceof Error ? error.message : String(error)
        console.error('[main:ssh] Failed to load remote session:', errorMsg)
        return {
          info: { ...info, status: 'error' as const, error: `Connected but failed to load session: ${errorMsg}` }
        }
      }
    } else {
      console.warn('[main:ssh] Could not find sender window')
    }
  }

  return { info }
})

server.onSshDisconnect(async (connectionId) => {
  if (!connectionManager) throw new Error('ConnectionManager not initialized')
  connectionManager.disconnectRemote(connectionId)
})

server.onSshListConnections(async () => {
  if (!connectionManager) return []
  return connectionManager.listConnections()
})

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

server.onSshGetSavedConnections(async () => {
  const settings = loadSettings()
  return settings.ssh.savedConnections
})

server.onSshRemoveSavedConnection(async (id) => {
  const settings = loadSettings()
  settings.ssh.savedConnections = settings.ssh.savedConnections.filter(c => c.id !== id)
  saveSettings(settings)
})

server.onSshGetOutput(async (connectionId) => {
  if (!connectionManager) return []
  const tunnel = connectionManager.getSSHTunnel(connectionId)
  return tunnel?.getOutput() || []
})

// Per-window watch subscriptions
const outputWatchUnsubscribers = new Map<number, Map<string, () => void>>()
const statusWatchUnsubscribers = new Map<number, Map<string, () => void>>()

server.onSshWatchOutput(async (event, connectionId) => {
  if (!connectionManager) return { scrollback: [] }

  const senderWindow = BrowserWindow.fromWebContents(event.sender)
  if (!senderWindow) return { scrollback: [] }
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
  outputWatchUnsubscribers.get(winId)!.set(connectionId, unsubscribe)

  return { scrollback }
})

server.onSshUnwatchOutput(async (event, connectionId) => {
  const senderWindow = BrowserWindow.fromWebContents(event.sender)
  if (!senderWindow) return

  const winId = senderWindow.id
  outputWatchUnsubscribers.get(winId)?.get(connectionId)?.()
  outputWatchUnsubscribers.get(winId)?.delete(connectionId)
})

server.onSshWatchConnectionStatus(async (event, connectionId) => {
  if (!connectionManager) return { initial: undefined }

  const senderWindow = BrowserWindow.fromWebContents(event.sender)
  if (!senderWindow) return { initial: undefined }
  const winId = senderWindow.id

  // Clean up any existing watch for this window+connection
  statusWatchUnsubscribers.get(winId)?.get(connectionId)?.()

  const windowInfo = windowManager.getWindow(winId)
  const { initial, unsubscribe } = connectionManager.watchConnectionStatus(connectionId, (info) => {
    if (windowInfo) {
      windowInfo.ipcServer.sshConnectionStatus(info)
    }
  })

  if (!statusWatchUnsubscribers.has(winId)) {
    statusWatchUnsubscribers.set(winId, new Map())
  }
  statusWatchUnsubscribers.get(winId)!.set(connectionId, unsubscribe)

  return { initial }
})

server.onSshUnwatchConnectionStatus(async (event, connectionId) => {
  const senderWindow = BrowserWindow.fromWebContents(event.sender)
  if (!senderWindow) return

  const winId = senderWindow.id
  statusWatchUnsubscribers.get(winId)?.get(connectionId)?.()
  statusWatchUnsubscribers.get(winId)?.delete(connectionId)
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
app.whenReady().then(async () => {
  // Always use daemon mode
  console.log('[main] daemon mode enabled')

  // Show loading screen while connecting to daemon (skip in test mode)
  if (process.env.NODE_ENV !== 'test') {
    createLoadingWindow()
  }

  daemonClient = new GrpcDaemonClient(process.env.TREETERM_SOCKET_PATH)

  // Forward daemon disconnections to renderer so the UI can show a warning
  daemonClient.onDisconnect(() => {
    server.daemonDisconnected()
  })

  // Proactively connect to daemon on startup
  await daemonClient.ensureDaemonRunning()

  // Create ConnectionManager wrapping local daemon client
  connectionManager = new ConnectionManager(daemonClient)

  // Close loading window and show main window
  if (loadingWindow) {
    loadingWindow.close()
    loadingWindow = null
  }

  registerSTTHandlers(server)
  mainWindow = createWindow()
  server.setWindow(mainWindow)
  createApplicationMenu(mainWindow, server, quitAndKillDaemon)

  // Handle --ssh startup argument
  if (initialSSHTarget && connectionManager) {
    const parsed = parseSSHTarget(initialSSHTarget)
    if (parsed) {
      console.log('[main] Auto-connecting SSH:', initialSSHTarget)
      connectionManager.connectRemote(parsed).then(async (info) => {
        if (info.status === 'connected') {
          console.log('[main] SSH connected:', info.id)
          if (mainWindow) {
            // Load session from remote daemon and re-initialize the renderer
            try {
              const remoteClient = connectionManager!.getClient(parsed.id)
              const remoteSessionId = await remoteClient.getDefaultSessionId()
              const windowId = mainWindow.id
              const remoteWatch = remoteClient.watchSession(remoteSessionId, randomUUID(), (updatedSession) => {
                const windowInfo = windowManager.getWindow(windowId)
                if (windowInfo) {
                  windowInfo.ipcServer.sessionSync(updatedSession)
                }
              })
              const session = await remoteWatch.initial
              const windowInfo = windowManager.getWindow(windowId)
              if (windowInfo) {
                windowInfo.ipcServer.appReady(session)
              }
            } catch (error) {
              console.error('[main] Failed to load remote session:', error)
            }
          }
        } else {
          console.error('[main] SSH connection failed:', info.error)
        }
      }).catch((error) => {
        console.error('[main] SSH connection error:', error)
      })
    }
    initialSSHTarget = null
  }

  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0) {
      mainWindow = createWindow()
      server.setWindow(mainWindow)
      createApplicationMenu(mainWindow, server, quitAndKillDaemon)
    }
  })
}).catch((error) => {
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
  return {
    id: `ssh-${match[2]}-${Date.now()}`,
    user: match[1],
    host: match[2],
    port: match[3] ? parseInt(match[3], 10) : 22,
    label: target
  }
}

async function quitAndKillDaemon(): Promise<void> {
  if (daemonClient) {
    try {
      console.log('[main] shutting down daemon before quit')
      await daemonClient.shutdownDaemon()
    } catch (error) {
      const errorMessage = error instanceof Error ? error.message : 'Unknown error'
      console.error('[main] failed to shutdown daemon:', errorMessage)
    }
  }
  app.quit()
}

app.on('before-quit', async () => {
  // Disconnect all remote SSH connections
  if (connectionManager) {
    connectionManager.disconnectAll()
  }

  if (daemonClient && daemonClient.isConnected()) {
    daemonClient.disconnect()
  }
})
