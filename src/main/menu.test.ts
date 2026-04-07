import { describe, it, expect, vi, beforeEach } from 'vitest'
import type { IpcServer } from './ipc/ipc-server'

vi.mock('electron', () => ({
  Menu: {
    buildFromTemplate: vi.fn(() => 'built-menu'),
    setApplicationMenu: vi.fn()
  },
  app: {
    name: 'TreeTerm',
    quit: vi.fn()
  },
  BrowserWindow: Object.assign(vi.fn(), {
    getFocusedWindow: vi.fn(() => null)
  }),
  shell: {
    openExternal: vi.fn()
  }
}))

vi.mock('./windowManager', () => ({
  windowManager: {
    getWindow: vi.fn(() => undefined)
  }
}))

import { Menu, app, shell, BrowserWindow } from 'electron'
import { windowManager, type WindowInfo } from './windowManager'
import { createApplicationMenu } from './menu'

type MenuItem = {
  label?: string
  role?: string
  submenu?: MenuItem[]
  click?: (...args: any[]) => any
}

function getTemplate(): MenuItem[] {
  // eslint-disable-next-line @typescript-eslint/unbound-method
  return vi.mocked(Menu.buildFromTemplate).mock.calls[0]![0] as unknown as MenuItem[]
}

describe('menu', () => {
  const mockServer = {
    settingsOpen: vi.fn(),
    activeProcessesOpen: vi.fn()
  }

  beforeEach(() => {
    vi.clearAllMocks()
  })

  describe('createApplicationMenu', () => {
    it('builds menu from template', () => {
      createApplicationMenu(null, mockServer as unknown as IpcServer)

      // eslint-disable-next-line @typescript-eslint/unbound-method
      expect(vi.mocked(Menu.buildFromTemplate)).toHaveBeenCalled()
      // eslint-disable-next-line @typescript-eslint/unbound-method
      expect(vi.mocked(Menu.setApplicationMenu)).toHaveBeenCalledWith('built-menu')
    })

    it('includes app menu on macOS', () => {
      Object.defineProperty(process, 'platform', { value: 'darwin' })

      createApplicationMenu(null, mockServer as unknown as IpcServer)

      const template = getTemplate()
      expect(template[0]!.label).toBe('TreeTerm')
    })

    it('includes settings in File menu on non-macOS', () => {
      Object.defineProperty(process, 'platform', { value: 'linux' })

      createApplicationMenu(null, mockServer as unknown as IpcServer)

      const template = getTemplate()
      const fileMenu = template.find((item) => item.label === 'File')
      expect(fileMenu).toBeDefined()
    })

    it('calls settingsOpen when Preferences clicked', () => {
      Object.defineProperty(process, 'platform', { value: 'darwin' })

      createApplicationMenu(null, mockServer as unknown as IpcServer)

      const template = getTemplate()
      const appMenu = template[0]!
      const preferencesItem = appMenu.submenu?.find((item) => item.label === 'Preferences...')

      preferencesItem?.click?.()

      expect(mockServer.settingsOpen).toHaveBeenCalled()
    })

    it('calls onQuitAndKillDaemon callback when Exit and Kill Daemon clicked', () => {
      const onQuitAndKillDaemon = vi.fn()

      createApplicationMenu(null, mockServer as unknown as IpcServer, onQuitAndKillDaemon)

      const template = getTemplate()
      const fileMenu = template.find((item) => item.label === 'File')
      const exitItem = fileMenu?.submenu?.find((item) => item.label === 'Exit and Kill Daemon')

      exitItem?.click?.()

      expect(onQuitAndKillDaemon).toHaveBeenCalled()
    })

    it('calls app.quit when Exit and Kill Daemon clicked without callback', () => {
      Object.defineProperty(process, 'platform', { value: 'linux' })

      createApplicationMenu(null, mockServer as unknown as IpcServer)

      const template = getTemplate()
      const fileMenu = template.find((item) => item.label === 'File')
      const exitItem = fileMenu?.submenu?.find((item) => item.label === 'Exit and Kill Daemon')

      exitItem?.click?.()

      // eslint-disable-next-line @typescript-eslint/unbound-method
      expect(vi.mocked(app.quit)).toHaveBeenCalled()
    })

    it('uses focused window ipcServer when available', () => {
      const focusedServer = { settingsOpen: vi.fn(), activeProcessesOpen: vi.fn() }
      // eslint-disable-next-line @typescript-eslint/unbound-method
      vi.mocked(BrowserWindow.getFocusedWindow).mockReturnValue({ id: 1 } as unknown as BrowserWindow)
      // eslint-disable-next-line @typescript-eslint/unbound-method
      vi.mocked(windowManager.getWindow).mockReturnValue({ ipcServer: focusedServer } as unknown as WindowInfo)

      Object.defineProperty(process, 'platform', { value: 'darwin' })
      createApplicationMenu(null, mockServer as unknown as IpcServer)

      const template = getTemplate()
      const appMenu = template[0]!
      const preferencesItem = appMenu.submenu?.find((item) => item.label === 'Preferences...')
      preferencesItem?.click?.()

      expect(focusedServer.settingsOpen).toHaveBeenCalled()
      expect(mockServer.settingsOpen).not.toHaveBeenCalled()

      // eslint-disable-next-line @typescript-eslint/unbound-method
      vi.mocked(BrowserWindow.getFocusedWindow).mockReturnValue(null)
      // eslint-disable-next-line @typescript-eslint/unbound-method
      vi.mocked(windowManager.getWindow).mockReturnValue(undefined)
    })

    it('calls settingsOpen when Settings clicked on non-macOS', () => {
      Object.defineProperty(process, 'platform', { value: 'linux' })

      createApplicationMenu(null, mockServer as unknown as IpcServer)

      const template = getTemplate()
      const fileMenu = template.find((item) => item.label === 'File')
      const settingsItem = fileMenu?.submenu?.find((item) => item.label === 'Settings')
      settingsItem?.click?.()

      expect(mockServer.settingsOpen).toHaveBeenCalled()
    })

    it('calls activeProcessesOpen when Active Processes clicked', () => {
      createApplicationMenu(null, mockServer as unknown as IpcServer)

      const template = getTemplate()
      const windowMenu = template.find((item) => item.label === 'Window')
      const activeProcessesItem = windowMenu?.submenu?.find((item) => item.label === 'Active Processes')
      activeProcessesItem?.click?.()

      expect(mockServer.activeProcessesOpen).toHaveBeenCalled()
    })

    it('opens external link when Learn More clicked', async () => {
      createApplicationMenu(null, mockServer as unknown as IpcServer)

      const template = getTemplate()
      const helpMenu = template.find((item) => item.role === 'help')
      const learnMoreItem = helpMenu?.submenu?.find((item) => item.label === 'Learn More')

      await learnMoreItem?.click?.()

      // eslint-disable-next-line @typescript-eslint/unbound-method
      expect(vi.mocked(shell.openExternal)).toHaveBeenCalledWith('https://github.com')
    })
  })
})
