import { app, BrowserWindow, dialog, shell } from 'electron'
import { execSync } from 'child_process'
import { join } from 'path'
import { GrpcDaemonClient } from './grpcClient'
import { IpcServer } from './ipc/ipc-server'
import { GitClient } from './git'
import { ReviewsClient } from './reviews'
import { windowManager } from './windowManager'

// Parse initial workspace from command line
let initialWorkspacePath: string | null = null
for (const arg of process.argv) {
  if (arg.startsWith('--workspace=')) {
    initialWorkspacePath = arg.substring('--workspace='.length)
    break
  }
}
import { loadSettings, saveSettings, Settings } from './settings'
import { createApplicationMenu } from './menu'
import { registerSTTHandlers } from './stt'

let mainWindow: BrowserWindow | null = null
let loadingWindow: BrowserWindow | null = null
let closeConfirmed = false
let daemonClient: GrpcDaemonClient | null = null
let gitClient: GitClient | null = null
let reviewsClient: ReviewsClient | null = null
let useDaemon = true // Always use daemon mode
let attachedSessions: Set<string> = new Set()

// Initialize IPC server
const server = new IpcServer()

function createLoadingWindow(): BrowserWindow {
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
    loadingWindow?.show()
  })

  return loadingWindow
}

function createWindow(initialSessionId?: string): BrowserWindow {
  const window = new BrowserWindow({
    width: 1200,
    height: 800,
    minWidth: 800,
    minHeight: 600,
    webPreferences: {
      preload: join(__dirname, '../preload/index.js'),
      nodeIntegration: false,
      contextIsolation: true
    },
    titleBarStyle: 'hiddenInset',
    trafficLightPosition: { x: 15, y: 15 }
  })

  // Create a dedicated IPC server for this window
  const windowServer = new IpcServer()
  windowServer.setWindow(window)

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
    shell.openExternal(url)
    return { action: 'deny' }
  })

  // Intercept close event to check for unmerged workspaces
  window.on('close', (event) => {
    if (!closeConfirmed) {
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
      
      if (initialSessionId) {
        // Get the specific session from daemon
        const session = await daemonClient.getSession(initialSessionId)
        if (session) {
          console.log('[main] loaded session:', session.id)
          windowServer.appReady(session)
        } else {
          // Session not found, fall back to default
          const defaultSession = await daemonClient.getDefaultSession()
          console.log('[main] session not found, using default:', defaultSession.id)
          windowServer.appReady(defaultSession)
        }
      } else {
        // Get the default session from daemon (creates one if doesn't exist)
        const session = await daemonClient.getDefaultSession()
        console.log('[main] got default session:', session.id)
        windowServer.appReady(session)
      }
    } catch (error) {
      console.error('[main] failed to get session:', error)
      // Still signal ready but without a session
      windowServer.appReady(null)
    }
  })

  window.on('closed', () => {
    // Detach from sessions
    if (daemonClient) {
      for (const sessionId of attachedSessions) {
        daemonClient.detachPtySession(sessionId).catch(console.error)
      }
      attachedSessions.clear()
    }
  })

  // Register with window manager
  windowManager.registerWindow(window, initialSessionId || null, windowServer)

  return window
}

// IPC Handlers
server.onPtyCreate(async (cwd, sandbox, startupCommand) => {
  if (!mainWindow) return null
  if (!daemonClient) throw new Error('Daemon not initialized')

  try {
    await daemonClient.ensureDaemonRunning()
    const sessionId = await daemonClient.createPtySession({ cwd, sandbox, startupCommand })

    // Set up data forwarding
    daemonClient.onPtySessionData(sessionId, (data) => {
      server.ptyData(sessionId, data)
    })

    daemonClient.onPtySessionExit(sessionId, (exitCode, signal) => {
      server.ptyExit(sessionId, exitCode)
      attachedSessions.delete(sessionId)
    })

    attachedSessions.add(sessionId)
    return sessionId
  } catch (error) {
    console.error('[main] failed to create session via daemon:', error)
    return null
  }
})

server.onPtyAttach(async (sessionId) => {
  if (!daemonClient) {
    return { success: false, error: 'Daemon not initialized' }
  }

  try {
    await daemonClient.ensureDaemonRunning()
    const result = await daemonClient.attachPtySession(sessionId)

    // Set up data forwarding if not already attached
    if (!attachedSessions.has(sessionId)) {
      daemonClient.onPtySessionData(sessionId, (data) => {
        server.ptyData(sessionId, data)
      })

      daemonClient.onPtySessionExit(sessionId, (exitCode, signal) => {
        server.ptyExit(sessionId, exitCode)
        attachedSessions.delete(sessionId)
      })

      attachedSessions.add(sessionId)
    }

    return { success: true, scrollback: result.scrollback }
  } catch (error) {
    const errorMessage = error instanceof Error ? error.message : 'Unknown error'
    console.error('[main] failed to attach to session:', errorMessage)
    return { success: false, error: errorMessage }
  }
})

server.onPtyDetach(async (sessionId) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  await daemonClient.detachPtySession(sessionId)
  attachedSessions.delete(sessionId)
})

server.onPtyList(async () => {
  if (!daemonClient) return []

  try {
    await daemonClient.ensureDaemonRunning()
    return await daemonClient.listPtySessions()
  } catch (error) {
    console.error('[main] failed to list sessions:', error)
    return []
  }
})

server.onPtyWrite((id, data) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  daemonClient.writeToPtySession(id, data)
})

server.onPtyResize((id, cols, rows) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  daemonClient.resizePtySession(id, cols, rows)
})

server.onPtyKill(async (id) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  await daemonClient.killPtySession(id)
  attachedSessions.delete(id)
})

server.onPtyIsAlive(async (id) => {
  if (!daemonClient) return false
  try {
    const sessions = await daemonClient.listPtySessions()
    return sessions.some(s => s.id === id)
  } catch {
    return false
  }
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

server.onSessionUpdate(async (sessionId, workspaces) => {
  if (!useDaemon || !daemonClient) {
    return { success: false, error: 'Daemon not enabled' }
  }

  try {
    await daemonClient.ensureDaemonRunning()
    const result = await daemonClient.updateSession(sessionId, workspaces)
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

server.onSessionGet(async (sessionId) => {
  if (!useDaemon || !daemonClient) {
    return { success: false, error: 'Daemon not enabled' }
  }

  try {
    await daemonClient.ensureDaemonRunning()
    const session = await daemonClient.getSession(sessionId)
    return { success: true, session: session || undefined }
  } catch (error) {
    const errorMessage = error instanceof Error ? error.message : 'Unknown error'
    console.error('[main] failed to get session:', errorMessage)
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
    // Check if session is already open in another window
    if (windowManager.isSessionOpen(sessionId)) {
      // Focus the existing window
      windowManager.focusWindowBySessionId(sessionId)
      return { success: true }
    }

    // Verify the session exists
    const session = await daemonClient.getSession(sessionId)
    if (!session) {
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

server.onDialogSelectFolder(async () => {
  if (!mainWindow) return null
  const result = await dialog.showOpenDialog(mainWindow, {
    properties: ['openDirectory']
  })
  if (result.canceled || result.filePaths.length === 0) {
    return null
  }
  return result.filePaths[0]
})

// Initialize git client when daemon is ready
function initializeGitClient() {
  if (daemonClient && !gitClient) {
    gitClient = new GitClient(daemonClient)
  }
}

// Initialize reviews client when daemon is ready
function initializeReviewsClient() {
  if (daemonClient && !reviewsClient) {
    reviewsClient = new ReviewsClient(daemonClient)
  }
}

// Git IPC Handlers - Now handled in main process via ExecStream
server.onGitGetInfo(async (dirPath) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  initializeGitClient()
  if (!gitClient) throw new Error('Git client not initialized')
  return gitClient.getGitInfo(dirPath)
})

server.onGitCreateWorktree(async (repoPath, name, baseBranch) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  initializeGitClient()
  if (!gitClient) throw new Error('Git client not initialized')
  try {
    const result = await gitClient.createWorktree(repoPath, name, baseBranch)
    return { success: true, path: result.path, branch: result.branch }
  } catch (error) {
    const errorMessage = error instanceof Error ? error.message : 'Failed to create worktree'
    console.error('[main] Failed to create worktree:', errorMessage)
    return { success: false, error: errorMessage }
  }
})

server.onGitRemoveWorktree(async (repoPath, worktreePath, deleteBranch) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  initializeGitClient()
  if (!gitClient) throw new Error('Git client not initialized')
  await gitClient.removeWorktree(repoPath, worktreePath, deleteBranch)
  return { success: true }
})

server.onGitListWorktrees(async (repoPath) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  initializeGitClient()
  if (!gitClient) throw new Error('Git client not initialized')
  return gitClient.listWorktrees(repoPath)
})

server.onGitGetChildWorktrees(async (repoPath, parentBranch) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  initializeGitClient()
  if (!gitClient) throw new Error('Git client not initialized')
  return gitClient.getChildWorktrees(repoPath, parentBranch)
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

server.onGitCreateWorktreeFromBranch(async (repoPath, branch, worktreeName) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  initializeGitClient()
  if (!gitClient) throw new Error('Git client not initialized')
  try {
    const result = await gitClient.createWorktreeFromBranch(repoPath, branch, worktreeName)
    return { success: true, path: result.path, branch: result.branch }
  } catch (error) {
    const errorMessage = error instanceof Error ? error.message : 'Failed to create worktree from branch'
    console.error('[main] Failed to create worktree from branch:', errorMessage)
    return { success: false, error: errorMessage }
  }
})

server.onGitCreateWorktreeFromRemote(async (repoPath, remoteBranch, worktreeName) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  initializeGitClient()
  if (!gitClient) throw new Error('Git client not initialized')
  try {
    const result = await gitClient.createWorktreeFromRemote(repoPath, remoteBranch, worktreeName)
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

server.onGitGetDiffAgainstHead(async (worktreePath, parentBranch) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  initializeGitClient()
  if (!gitClient) throw new Error('Git client not initialized')
  const result = await gitClient.getDiffAgainstHead(worktreePath, parentBranch)
  return { success: true, diff: result }
})

server.onGitGetFileDiffAgainstHead(async (worktreePath, parentBranch, filePath) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  initializeGitClient()
  if (!gitClient) throw new Error('Git client not initialized')
  const diff = await gitClient.getFileDiffAgainstHead(worktreePath, parentBranch, filePath)
  return { success: true, diff }
})

server.onGitMerge(async (mainRepoPath, worktreeBranch, targetBranch, squash) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  initializeGitClient()
  if (!gitClient) throw new Error('Git client not initialized')
  await gitClient.mergeWorktree(mainRepoPath, worktreeBranch, targetBranch, squash)
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

server.onGitDeleteBranch(async (repoPath, branchName) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  initializeGitClient()
  if (!gitClient) throw new Error('Git client not initialized')
  await gitClient.deleteBranch(repoPath, branchName)
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

server.onGitGetFileContentsForDiffAgainstHead(async (worktreePath, parentBranch, filePath) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  initializeGitClient()
  if (!gitClient) throw new Error('Git client not initialized')
  const result = await gitClient.getFileContentsForDiffAgainstHead(worktreePath, parentBranch, filePath)
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

// Reviews IPC Handlers - Now handled in main process via daemon filesystem gRPC
server.onReviewsLoad(async (worktreePath) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  initializeReviewsClient()
  if (!reviewsClient) throw new Error('Reviews client not initialized')
  const reviews = await reviewsClient.loadReviews(worktreePath)
  return { success: true, reviews }
})

server.onReviewsSave(async (worktreePath, reviews) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  initializeReviewsClient()
  if (!reviewsClient) throw new Error('Reviews client not initialized')
  await reviewsClient.saveReviews(worktreePath, reviews)
  return { success: true }
})

server.onReviewsAddComment(async (worktreePath, comment) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  initializeReviewsClient()
  if (!reviewsClient) throw new Error('Reviews client not initialized')
  const newComment = await reviewsClient.addComment(worktreePath, comment)
  return { success: true, comment: newComment }
})

server.onReviewsDeleteComment(async (worktreePath, commentId) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  initializeReviewsClient()
  if (!reviewsClient) throw new Error('Reviews client not initialized')
  const success = await reviewsClient.deleteComment(worktreePath, commentId)
  return { success }
})

server.onReviewsUpdateOutdated(async (worktreePath, currentCommitHash) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  initializeReviewsClient()
  if (!reviewsClient) throw new Error('Reviews client not initialized')
  const reviews = await reviewsClient.updateOutdatedComments(worktreePath, currentCommitHash)
  return { success: true, reviews }
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

// App close confirmation IPC handlers
server.onAppCloseConfirmed(() => {
  closeConfirmed = true
  mainWindow?.close()
})

server.onAppCloseCancelled(() => {
  closeConfirmed = false
})

// App lifecycle
app.whenReady().then(async () => {
  // Always use daemon mode
  console.log('[main] daemon mode enabled')

  // Show loading screen while connecting to daemon
  createLoadingWindow()

  daemonClient = new GrpcDaemonClient()

  // Proactively connect to daemon on startup
  await daemonClient.ensureDaemonRunning()

  // Close loading window and show main window
  if (loadingWindow) {
    loadingWindow.close()
    loadingWindow = null
  }

  registerSTTHandlers(server)
  mainWindow = createWindow()
  server.setWindow(mainWindow)  // Set window on global server for PTY data forwarding
  createApplicationMenu(mainWindow, server, quitAndKillDaemon)

  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0) {
      mainWindow = createWindow()
      server.setWindow(mainWindow)  // Set window on global server for PTY data forwarding
      createApplicationMenu(mainWindow, server, quitAndKillDaemon)
    }
  })
})

  app.on('window-all-closed', () => {
  app.quit()
})

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
  if (daemonClient) {
    const settings = loadSettings()
    if (settings.daemon.killOnQuit) {
      console.log('[main] killing all sessions before quit')
      for (const sessionId of attachedSessions) {
        await daemonClient.killPtySession(sessionId)
      }
    } else {
      console.log('[main] detaching from sessions before quit')
      for (const sessionId of attachedSessions) {
        await daemonClient.detachPtySession(sessionId)
      }
    }
    attachedSessions.clear()
    daemonClient.disconnect()
  }
})
