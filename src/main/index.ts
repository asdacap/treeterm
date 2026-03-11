import { app, BrowserWindow, dialog, shell } from 'electron'
import { execSync } from 'child_process'
import { join } from 'path'
import { ptyManager } from './pty'
import { GrpcDaemonClient } from './grpcClient'
import { IpcServer } from './ipc/ipc-server'

// Parse initial workspace from command line
let initialWorkspacePath: string | null = null
for (const arg of process.argv) {
  if (arg.startsWith('--workspace=')) {
    initialWorkspacePath = arg.substring('--workspace='.length)
    break
  }
}
import {
  getGitInfo,
  createWorktree,
  removeWorktree,
  listWorktrees,
  getChildWorktrees,
  listLocalBranches,
  listRemoteBranches,
  getBranchesInWorktrees,
  createWorktreeFromBranch,
  createWorktreeFromRemote,
  getDiff,
  getFileDiff,
  getDiffAgainstHead,
  getFileDiffAgainstHead,
  mergeWorktree,
  hasUncommittedChanges,
  commitAll,
  deleteBranch,
  checkMergeConflicts,
  getUncommittedChanges,
  getUncommittedFileDiff,
  stageFile,
  unstageFile,
  stageAll,
  unstageAll,
  commitStaged,
  getFileContentsForDiff,
  getFileContentsForDiffAgainstHead,
  getUncommittedFileContentsForDiff
} from './git'
import { loadSettings, saveSettings, Settings } from './settings'
import { createApplicationMenu } from './menu'
import { registerFilesystemHandlers } from './filesystem'
import { registerSTTHandlers } from './stt'

let mainWindow: BrowserWindow | null = null
let closeConfirmed = false
let daemonClient: GrpcDaemonClient | null = null
let useDaemon = false
let attachedSessions: Set<string> = new Set()

// Initialize IPC server
const server = new IpcServer()

function createWindow(): void {
  mainWindow = new BrowserWindow({
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

  // Handle media permissions for speech recognition and microphone
  mainWindow.webContents.session.setPermissionRequestHandler((webContents, permission, callback) => {
    if (permission === 'media' || permission === 'microphone') {
      // Always allow microphone access for push-to-talk
      callback(true)
    } else {
      callback(false)
    }
  })

  // Set the window on the IPC server
  server.setWindow(mainWindow)

  // Forward all keyboard events including Caps Lock to renderer
  mainWindow.webContents.on('before-input-event', (event, input) => {
    // Forward Caps Lock events to renderer via IPC
    if (input.code === 'CapsLock' || input.key === 'CapsLock') {
      server.capsLockEvent({
        type: input.type, // 'keyDown' or 'keyUp'
        key: input.key,
        code: input.code
      })
    }
  })

  // Open external links in the default browser instead of within Electron
  mainWindow.webContents.on('will-navigate', (event, url) => {
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

  mainWindow.webContents.setWindowOpenHandler(({ url }) => {
    shell.openExternal(url)
    return { action: 'deny' }
  })

  // Intercept close event to check for unmerged workspaces
  mainWindow.on('close', (event) => {
    if (!closeConfirmed) {
      event.preventDefault()
      mainWindow?.webContents.send('app:confirm-close')
    }
  })

  // Load the renderer
  if (process.env.ELECTRON_RENDERER_URL) {
    mainWindow.loadURL(process.env.ELECTRON_RENDERER_URL)
  } else {
    mainWindow.loadFile(join(__dirname, '../renderer/index.html'))
  }

  // Signal renderer when ready to initialize
  mainWindow.webContents.on('did-finish-load', async () => {
    server.appReady()
    // Renderer will request workspaces when ready via workspace:list
  })

  mainWindow.on('closed', () => {
    // Detach from sessions if using daemon
    if (daemonClient && useDaemon) {
      for (const sessionId of attachedSessions) {
        daemonClient.detachPtySession(sessionId).catch(console.error)
      }
      attachedSessions.clear()
      daemonClient.disconnect()
    } else {
      // Legacy mode: kill all PTYs
      ptyManager.killAll()
    }
    mainWindow = null
  })
}

// IPC Handlers
server.onPtyCreate(async (cwd, sandbox, startupCommand) => {
  if (!mainWindow) return null

  if (useDaemon && daemonClient) {
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
  } else {
    // Legacy mode: direct PTY
    return ptyManager.create(cwd, mainWindow, sandbox, startupCommand)
  }
})

server.onPtyAttach(async (sessionId) => {
  if (!useDaemon || !daemonClient) {
    return { success: false, error: 'Daemon not enabled' }
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
  if (useDaemon && daemonClient) {
    await daemonClient.detachPtySession(sessionId)
    attachedSessions.delete(sessionId)
  }
})

server.onPtyList(async () => {
  if (!useDaemon || !daemonClient) {
    return []
  }

  try {
    await daemonClient.ensureDaemonRunning()
    return await daemonClient.listPtySessions()
  } catch (error) {
    console.error('[main] failed to list sessions:', error)
    return []
  }
})

server.onPtyWrite((id, data) => {
  if (useDaemon && daemonClient) {
    daemonClient.writeToPtySession(id, data)
  } else {
    ptyManager.write(id, data)
  }
})

server.onPtyResize((id, cols, rows) => {
  if (useDaemon && daemonClient) {
    daemonClient.resizePtySession(id, cols, rows)
  } else {
    ptyManager.resize(id, cols, rows)
  }
})

server.onPtyKill(async (id) => {
  if (useDaemon && daemonClient) {
    await daemonClient.killPtySession(id)
    attachedSessions.delete(id)
  } else {
    ptyManager.kill(id)
  }
})

server.onPtyIsAlive(async (id) => {
  if (useDaemon && daemonClient) {
    try {
      const sessions = await daemonClient.listPtySessions()
      return sessions.some(s => s.id === id)
    } catch {
      return false
    }
  } else {
    return ptyManager.isAlive(id)
  }
})

server.onDaemonShutdown(async () => {
  if (!useDaemon || !daemonClient) {
    return { success: false, error: 'Daemon not enabled' }
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

// Git IPC Handlers
server.onGitGetInfo(async (dirPath) => {
  return getGitInfo(dirPath)
})

server.onGitCreateWorktree(async (repoPath, name, baseBranch) => {
  return createWorktree(repoPath, name, baseBranch)
})

server.onGitRemoveWorktree(async (repoPath, worktreePath, deleteBranch) => {
  return removeWorktree(repoPath, worktreePath, deleteBranch)
})

server.onGitListWorktrees(async (repoPath) => {
  return listWorktrees(repoPath)
})

server.onGitGetChildWorktrees(async (repoPath, parentBranch) => {
  return getChildWorktrees(repoPath, parentBranch)
})

server.onGitListLocalBranches(async (repoPath) => {
  return listLocalBranches(repoPath)
})

server.onGitListRemoteBranches(async (repoPath) => {
  return listRemoteBranches(repoPath)
})

server.onGitGetBranchesInWorktrees(async (repoPath) => {
  return getBranchesInWorktrees(repoPath)
})

server.onGitCreateWorktreeFromBranch(async (repoPath, branch, worktreeName) => {
  return createWorktreeFromBranch(repoPath, branch, worktreeName)
})

server.onGitCreateWorktreeFromRemote(async (repoPath, remoteBranch, worktreeName) => {
  return createWorktreeFromRemote(repoPath, remoteBranch, worktreeName)
})

server.onGitGetDiff(async (worktreePath, parentBranch) => {
  return getDiff(worktreePath, parentBranch)
})

server.onGitGetFileDiff(async (worktreePath, parentBranch, filePath) => {
  return getFileDiff(worktreePath, parentBranch, filePath)
})

server.onGitGetDiffAgainstHead(async (worktreePath, parentBranch) => {
  return getDiffAgainstHead(worktreePath, parentBranch)
})

server.onGitGetFileDiffAgainstHead(async (worktreePath, parentBranch, filePath) => {
  return getFileDiffAgainstHead(worktreePath, parentBranch, filePath)
})

server.onGitMerge(async (mainRepoPath, worktreeBranch, targetBranch, squash) => {
  return mergeWorktree(mainRepoPath, worktreeBranch, targetBranch, squash)
})

server.onGitCheckMergeConflicts(async (repoPath, sourceBranch, targetBranch) => {
  return checkMergeConflicts(repoPath, sourceBranch, targetBranch)
})

server.onGitHasUncommittedChanges(async (repoPath) => {
  return hasUncommittedChanges(repoPath)
})

server.onGitCommitAll(async (repoPath, message) => {
  return commitAll(repoPath, message)
})

server.onGitDeleteBranch(async (repoPath, branchName) => {
  return deleteBranch(repoPath, branchName)
})

server.onGitGetUncommittedChanges(async (repoPath) => {
  return getUncommittedChanges(repoPath)
})

server.onGitGetUncommittedFileDiff(async (repoPath, filePath, staged) => {
  return getUncommittedFileDiff(repoPath, filePath, staged)
})

server.onGitStageFile(async (repoPath, filePath) => {
  return stageFile(repoPath, filePath)
})

server.onGitUnstageFile(async (repoPath, filePath) => {
  return unstageFile(repoPath, filePath)
})

server.onGitStageAll(async (repoPath) => {
  return stageAll(repoPath)
})

server.onGitUnstageAll(async (repoPath) => {
  return unstageAll(repoPath)
})

server.onGitCommitStaged(async (repoPath, message) => {
  return commitStaged(repoPath, message)
})

server.onGitGetFileContentsForDiff(async (worktreePath, parentBranch, filePath) => {
  return getFileContentsForDiff(worktreePath, parentBranch, filePath)
})

server.onGitGetFileContentsForDiffAgainstHead(async (worktreePath, parentBranch, filePath) => {
  return getFileContentsForDiffAgainstHead(worktreePath, parentBranch, filePath)
})

server.onGitGetUncommittedFileContentsForDiff(async (repoPath, filePath, staged) => {
  return getUncommittedFileContentsForDiff(repoPath, filePath, staged)
})

// Settings IPC Handlers
server.onSettingsLoad(() => {
  return loadSettings()
})

server.onSettingsSave((settings) => {
  saveSettings(settings)
  return { success: true }
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
  // Load settings to check daemon mode
  const settings = loadSettings()
  useDaemon = settings.daemon.enabled

  if (useDaemon) {
    console.log('[main] daemon mode enabled')
    daemonClient = new GrpcDaemonClient()
  } else {
    console.log('[main] legacy mode enabled (direct PTY)')
  }

  registerFilesystemHandlers(server)
  registerSTTHandlers(server)
  createWindow()
  createApplicationMenu(mainWindow, server)

  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0) {
      createWindow()
      createApplicationMenu(mainWindow, server)
    }
  })
})

app.on('window-all-closed', () => {
  app.quit()
})

app.on('before-quit', async () => {
  if (useDaemon && daemonClient) {
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
  } else {
    ptyManager.killAll()
  }
})
