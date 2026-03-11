import { app, BrowserWindow, ipcMain, dialog, shell } from 'electron'
import { execSync } from 'child_process'
import { join } from 'path'
import { ptyManager } from './pty'
import { GrpcDaemonClient } from './grpcClient'

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

  // Forward all keyboard events including Caps Lock to renderer
  mainWindow.webContents.on('before-input-event', (event, input) => {
    // Forward Caps Lock events to renderer via IPC
    if (input.code === 'CapsLock' || input.key === 'CapsLock') {
      mainWindow?.webContents.send('capslock-event', {
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
    mainWindow?.webContents.send('app:ready')
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
ipcMain.handle('pty:create', async (_event, cwd: string, sandbox?: { enabled: boolean; allowNetwork: boolean; allowedPaths: string[] }, startupCommand?: string) => {
  if (!mainWindow) return null

  if (useDaemon && daemonClient) {
    try {
      await daemonClient.ensureDaemonRunning()
      const sessionId = await daemonClient.createPtySession({ cwd, sandbox, startupCommand })

      // Set up data forwarding
      daemonClient.onPtySessionData(sessionId, (data) => {
        mainWindow?.webContents.send('pty:data', sessionId, data)
      })

      daemonClient.onPtySessionExit(sessionId, (exitCode, signal) => {
        mainWindow?.webContents.send('pty:exit', sessionId, exitCode)
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

ipcMain.handle('pty:attach', async (_event, sessionId: string) => {
  if (!useDaemon || !daemonClient) {
    return { success: false, error: 'Daemon not enabled' }
  }

  try {
    await daemonClient.ensureDaemonRunning()
    const result = await daemonClient.attachPtySession(sessionId)

    // Set up data forwarding if not already attached
    if (!attachedSessions.has(sessionId)) {
      daemonClient.onPtySessionData(sessionId, (data) => {
        mainWindow?.webContents.send('pty:data', sessionId, data)
      })

      daemonClient.onPtySessionExit(sessionId, (exitCode, signal) => {
        mainWindow?.webContents.send('pty:exit', sessionId, exitCode)
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

ipcMain.handle('pty:detach', async (_event, sessionId: string) => {
  if (useDaemon && daemonClient) {
    await daemonClient.detachPtySession(sessionId)
    attachedSessions.delete(sessionId)
  }
})

ipcMain.handle('pty:list', async () => {
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

ipcMain.on('pty:write', (_event, id: string, data: string) => {
  if (useDaemon && daemonClient) {
    daemonClient.writeToPtySession(id, data)
  } else {
    ptyManager.write(id, data)
  }
})

ipcMain.on('pty:resize', (_event, id: string, cols: number, rows: number) => {
  if (useDaemon && daemonClient) {
    daemonClient.resizePtySession(id, cols, rows)
  } else {
    ptyManager.resize(id, cols, rows)
  }
})

ipcMain.on('pty:kill', async (_event, id: string) => {
  if (useDaemon && daemonClient) {
    await daemonClient.killPtySession(id)
    attachedSessions.delete(id)
  } else {
    ptyManager.kill(id)
  }
})

ipcMain.handle('pty:isAlive', async (_event, id: string) => {
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

ipcMain.handle('daemon:shutdown', async () => {
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
ipcMain.handle('session:create', async (_event, workspaces) => {
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

ipcMain.handle('session:update', async (_event, sessionId: string, workspaces) => {
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

ipcMain.handle('session:list', async () => {
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

ipcMain.handle('session:get', async (_event, sessionId: string) => {
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

ipcMain.handle('session:delete', async (_event, sessionId: string) => {
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

ipcMain.handle('dialog:selectFolder', async () => {
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
ipcMain.handle('git:getInfo', async (_event, dirPath: string) => {
  return getGitInfo(dirPath)
})

ipcMain.handle('git:createWorktree', async (_event, repoPath: string, name: string, baseBranch?: string) => {
  return createWorktree(repoPath, name, baseBranch)
})

ipcMain.handle('git:removeWorktree', async (_event, repoPath: string, worktreePath: string, deleteBranch: boolean) => {
  return removeWorktree(repoPath, worktreePath, deleteBranch)
})

ipcMain.handle('git:listWorktrees', async (_event, repoPath: string) => {
  return listWorktrees(repoPath)
})

ipcMain.handle('git:getChildWorktrees', async (_event, repoPath: string, parentBranch: string | null) => {
  return getChildWorktrees(repoPath, parentBranch)
})

ipcMain.handle('git:listLocalBranches', async (_event, repoPath: string) => {
  return listLocalBranches(repoPath)
})

ipcMain.handle('git:listRemoteBranches', async (_event, repoPath: string) => {
  return listRemoteBranches(repoPath)
})

ipcMain.handle('git:getBranchesInWorktrees', async (_event, repoPath: string) => {
  return getBranchesInWorktrees(repoPath)
})

ipcMain.handle('git:createWorktreeFromBranch', async (_event, repoPath: string, branch: string, worktreeName: string) => {
  return createWorktreeFromBranch(repoPath, branch, worktreeName)
})

ipcMain.handle('git:createWorktreeFromRemote', async (_event, repoPath: string, remoteBranch: string, worktreeName: string) => {
  return createWorktreeFromRemote(repoPath, remoteBranch, worktreeName)
})

ipcMain.handle('git:getDiff', async (_event, worktreePath: string, parentBranch: string) => {
  return getDiff(worktreePath, parentBranch)
})

ipcMain.handle('git:getFileDiff', async (_event, worktreePath: string, parentBranch: string, filePath: string) => {
  return getFileDiff(worktreePath, parentBranch, filePath)
})

ipcMain.handle('git:getDiffAgainstHead', async (_event, worktreePath: string, parentBranch: string) => {
  return getDiffAgainstHead(worktreePath, parentBranch)
})

ipcMain.handle('git:getFileDiffAgainstHead', async (_event, worktreePath: string, parentBranch: string, filePath: string) => {
  return getFileDiffAgainstHead(worktreePath, parentBranch, filePath)
})

ipcMain.handle('git:merge', async (_event, mainRepoPath: string, worktreeBranch: string, targetBranch: string, squash: boolean) => {
  return mergeWorktree(mainRepoPath, worktreeBranch, targetBranch, squash)
})

ipcMain.handle('git:checkMergeConflicts', async (_event, repoPath: string, sourceBranch: string, targetBranch: string) => {
  return checkMergeConflicts(repoPath, sourceBranch, targetBranch)
})

ipcMain.handle('git:hasUncommittedChanges', async (_event, repoPath: string) => {
  return hasUncommittedChanges(repoPath)
})

ipcMain.handle('git:commitAll', async (_event, repoPath: string, message: string) => {
  return commitAll(repoPath, message)
})

ipcMain.handle('git:deleteBranch', async (_event, repoPath: string, branchName: string) => {
  return deleteBranch(repoPath, branchName)
})

ipcMain.handle('git:getUncommittedChanges', async (_event, repoPath: string) => {
  return getUncommittedChanges(repoPath)
})

ipcMain.handle('git:getUncommittedFileDiff', async (_event, repoPath: string, filePath: string, staged: boolean) => {
  return getUncommittedFileDiff(repoPath, filePath, staged)
})

ipcMain.handle('git:stageFile', async (_event, repoPath: string, filePath: string) => {
  return stageFile(repoPath, filePath)
})

ipcMain.handle('git:unstageFile', async (_event, repoPath: string, filePath: string) => {
  return unstageFile(repoPath, filePath)
})

ipcMain.handle('git:stageAll', async (_event, repoPath: string) => {
  return stageAll(repoPath)
})

ipcMain.handle('git:unstageAll', async (_event, repoPath: string) => {
  return unstageAll(repoPath)
})

ipcMain.handle('git:commitStaged', async (_event, repoPath: string, message: string) => {
  return commitStaged(repoPath, message)
})

ipcMain.handle('git:getFileContentsForDiff', async (_event, worktreePath: string, parentBranch: string, filePath: string) => {
  return getFileContentsForDiff(worktreePath, parentBranch, filePath)
})

ipcMain.handle('git:getFileContentsForDiffAgainstHead', async (_event, worktreePath: string, parentBranch: string, filePath: string) => {
  return getFileContentsForDiffAgainstHead(worktreePath, parentBranch, filePath)
})

ipcMain.handle('git:getUncommittedFileContentsForDiff', async (_event, repoPath: string, filePath: string, staged: boolean) => {
  return getUncommittedFileContentsForDiff(repoPath, filePath, staged)
})

// Settings IPC Handlers
ipcMain.handle('settings:load', () => {
  return loadSettings()
})

ipcMain.handle('settings:save', (_event, settings: Settings) => {
  saveSettings(settings)
  return { success: true }
})

// Sandbox IPC Handlers
ipcMain.handle('sandbox:isAvailable', () => {
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

ipcMain.handle('app:getInitialWorkspace', () => {
  const path = initialWorkspacePath
  initialWorkspacePath = null // Clear after first read
  return path
})

// App close confirmation IPC handlers
ipcMain.on('app:close-confirmed', () => {
  closeConfirmed = true
  mainWindow?.close()
})

ipcMain.on('app:close-cancelled', () => {
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

  registerFilesystemHandlers()
  registerSTTHandlers()
  createWindow()
  createApplicationMenu(mainWindow)

  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0) {
      createWindow()
      createApplicationMenu(mainWindow)
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
