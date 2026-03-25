import { Menu, app, BrowserWindow } from 'electron'
import type { IpcServer } from './ipc/ipc-server'
import { windowManager } from './windowManager'

function sendToFocusedWindow(fallbackServer: IpcServer, action: (server: IpcServer) => void): void {
  const focused = BrowserWindow.getFocusedWindow()
  if (focused) {
    const windowInfo = windowManager.getWindow(focused.id)
    if (windowInfo) {
      action(windowInfo.ipcServer)
      return
    }
  }
  action(fallbackServer)
}

export function createApplicationMenu(
  mainWindow: BrowserWindow | null,
  server: IpcServer,
  onQuitAndKillDaemon?: () => void
): void {
  const isMac = process.platform === 'darwin'

  const template: Electron.MenuItemConstructorOptions[] = [
    // App menu (macOS only)
    ...(isMac
      ? [
          {
            label: app.name,
            submenu: [
              { role: 'about' as const },
              { type: 'separator' as const },
              {
                label: 'Preferences...',
                accelerator: 'Cmd+,',
                click: () => {
                  sendToFocusedWindow(server, s => s.settingsOpen())
                }
              },
              { type: 'separator' as const },
              { role: 'services' as const },
              { type: 'separator' as const },
              { role: 'hide' as const },
              { role: 'hideOthers' as const },
              { role: 'unhide' as const },
              { type: 'separator' as const },
              { role: 'quit' as const }
            ]
          }
        ]
      : []),

    // File menu
    {
      label: 'File',
      submenu: [
        ...(isMac
          ? []
          : [
              {
                label: 'Settings',
                accelerator: 'Ctrl+,',
                click: () => {
                  sendToFocusedWindow(server, s => s.settingsOpen())
                }
              },
              { type: 'separator' as const }
            ]),
        isMac ? { role: 'close' as const } : { role: 'quit' as const },
        { type: 'separator' as const },
        {
          label: 'Exit and Kill Daemon',
          accelerator: isMac ? 'Cmd+Shift+Q' : 'Ctrl+Shift+Q',
          click: () => {
            if (onQuitAndKillDaemon) {
              onQuitAndKillDaemon()
            } else {
              app.quit()
            }
          }
        }
      ]
    },

    // Edit menu
    {
      label: 'Edit',
      submenu: [
        { role: 'undo' as const },
        { role: 'redo' as const },
        { type: 'separator' as const },
        { role: 'cut' as const },
        { role: 'copy' as const },
        { role: 'paste' as const },
        ...(isMac
          ? [
              { role: 'pasteAndMatchStyle' as const },
              { role: 'delete' as const },
              { role: 'selectAll' as const }
            ]
          : [{ role: 'delete' as const }, { type: 'separator' as const }, { role: 'selectAll' as const }])
      ]
    },

    // Workspace menu
    {
      label: 'Workspace',
      submenu: [
        {
          label: 'Browse Sessions...',
          click: () => {
            sendToFocusedWindow(server, s => s.sessionShowSessions())
          }
        }
      ]
    },

    // View menu
    {
      label: 'View',
      submenu: [
        { role: 'reload' as const },
        { role: 'forceReload' as const },
        { role: 'toggleDevTools' as const },
        { type: 'separator' as const },
        { role: 'resetZoom' as const },
        { role: 'zoomIn' as const },
        { role: 'zoomOut' as const },
        { type: 'separator' as const },
        { role: 'togglefullscreen' as const },
        { type: 'separator' as const },
        {
          label: 'Reset Input',
          click: () => {
            const focused = BrowserWindow.getFocusedWindow()
            if (focused) {
              focused.blur()
              focused.focus()
            }
          }
        }
      ]
    },

    // Window menu
    {
      label: 'Window',
      submenu: [
        { role: 'minimize' as const },
        { role: 'zoom' as const },
        { type: 'separator' as const },
        {
          label: 'Active Processes',
          click: () => {
            sendToFocusedWindow(server, s => s.activeProcessesOpen())
          }
        },
        ...(isMac
          ? [{ type: 'separator' as const }, { role: 'front' as const }, { type: 'separator' as const }, { role: 'window' as const }]
          : [{ role: 'close' as const }])
      ]
    },

    // Help menu
    {
      role: 'help' as const,
      submenu: [
        {
          label: 'Learn More',
          click: async () => {
            const { shell } = await import('electron')
            await shell.openExternal('https://github.com')
          }
        }
      ]
    }
  ]

  const menu = Menu.buildFromTemplate(template)
  Menu.setApplicationMenu(menu)
}
