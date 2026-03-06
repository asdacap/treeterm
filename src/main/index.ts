import { app, BrowserWindow, ipcMain, dialog } from 'electron'
import { join } from 'path'
import { ptyManager } from './pty'
import { getGitInfo, createWorktree, removeWorktree, listWorktrees } from './git'

let mainWindow: BrowserWindow | null = null

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

  // Load the renderer
  if (process.env.ELECTRON_RENDERER_URL) {
    mainWindow.loadURL(process.env.ELECTRON_RENDERER_URL)
  } else {
    mainWindow.loadFile(join(__dirname, '../renderer/index.html'))
  }

  mainWindow.on('closed', () => {
    mainWindow = null
    ptyManager.killAll()
  })
}

// IPC Handlers
ipcMain.handle('pty:create', (_event, cwd: string) => {
  if (!mainWindow) return null
  return ptyManager.create(cwd, mainWindow)
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

// App lifecycle
app.whenReady().then(() => {
  createWindow()

  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0) {
      createWindow()
    }
  })
})

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') {
    app.quit()
  }
})

app.on('before-quit', () => {
  ptyManager.killAll()
})
