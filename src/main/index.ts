import { app, BrowserWindow, ipcMain, dialog, shell } from 'electron'
import { execSync } from 'child_process'
import { join } from 'path'
import { ptyManager } from './pty'

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
  mainWindow.webContents.on('did-finish-load', () => {
    mainWindow?.webContents.send('app:ready')
  })

  mainWindow.on('closed', () => {
    mainWindow = null
    ptyManager.killAll()
  })
}

// IPC Handlers
ipcMain.handle('pty:create', (_event, cwd: string, sandbox?: { enabled: boolean; allowNetwork: boolean; allowedPaths: string[] }, startupCommand?: string) => {
  if (!mainWindow) return null
  return ptyManager.create(cwd, mainWindow, sandbox, startupCommand)
})

ipcMain.on('pty:write', (_event, id: string, data: string) => {
  ptyManager.write(id, data)
})

ipcMain.on('pty:resize', (_event, id: string, cols: number, rows: number) => {
  ptyManager.resize(id, cols, rows)
})

ipcMain.on('pty:kill', (_event, id: string) => {
  ptyManager.kill(id)
})

ipcMain.handle('pty:isAlive', (_event, id: string) => {
  return ptyManager.isAlive(id)
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
app.whenReady().then(() => {
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

app.on('before-quit', () => {
  ptyManager.killAll()
})
