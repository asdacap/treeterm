import { describe, it, expect, vi, beforeEach } from 'vitest'

vi.mock('electron', () => ({
  Menu: {
    buildFromTemplate: vi.fn(() => 'built-menu'),
    setApplicationMenu: vi.fn()
  },
  app: {
    name: 'TreeTerm',
    quit: vi.fn()
  },
  BrowserWindow: vi.fn(),
  shell: {
    openExternal: vi.fn()
  }
}))

import { Menu, app, shell } from 'electron'
import { createApplicationMenu } from './menu'

describe('menu', () => {
  const mockServer = {
    settingsOpen: vi.fn(),
    terminalNew: vi.fn(),
    sessionShowSessions: vi.fn()
  }

  beforeEach(() => {
    vi.clearAllMocks()
  })

  describe('createApplicationMenu', () => {
    it('builds menu from template', () => {
      createApplicationMenu(null, mockServer as any)
      
      expect(Menu.buildFromTemplate).toHaveBeenCalled()
      expect(Menu.setApplicationMenu).toHaveBeenCalledWith('built-menu')
    })

    it('includes app menu on macOS', () => {
      Object.defineProperty(process, 'platform', { value: 'darwin' })
      
      createApplicationMenu(null, mockServer as any)
      
      const template = (Menu.buildFromTemplate as any).mock.calls[0][0]
      expect(template[0].label).toBe('TreeTerm')
    })

    it('includes settings in File menu on non-macOS', () => {
      Object.defineProperty(process, 'platform', { value: 'linux' })
      
      createApplicationMenu(null, mockServer as any)
      
      const template = (Menu.buildFromTemplate as any).mock.calls[0][0]
      const fileMenu = template.find((item: any) => item.label === 'File')
      expect(fileMenu).toBeDefined()
    })

    it('calls settingsOpen when Preferences clicked', () => {
      Object.defineProperty(process, 'platform', { value: 'darwin' })
      
      createApplicationMenu(null, mockServer as any)
      
      const template = (Menu.buildFromTemplate as any).mock.calls[0][0]
      const appMenu = template[0]
      const preferencesItem = appMenu.submenu.find((item: any) => item.label === 'Preferences...')
      
      preferencesItem.click()
      
      expect(mockServer.settingsOpen).toHaveBeenCalled()
    })

    it('calls terminalNew when New Terminal clicked', () => {
      createApplicationMenu(null, mockServer as any)
      
      const template = (Menu.buildFromTemplate as any).mock.calls[0][0]
      const workspaceMenu = template.find((item: any) => item.label === 'Workspace')
      const newTerminalItem = workspaceMenu.submenu.find((item: any) => item.label === 'New Terminal')
      
      newTerminalItem.click()
      
      expect(mockServer.terminalNew).toHaveBeenCalled()
    })

    it('calls sessionShowSessions when Browse Sessions clicked', () => {
      createApplicationMenu(null, mockServer as any)
      
      const template = (Menu.buildFromTemplate as any).mock.calls[0][0]
      const workspaceMenu = template.find((item: any) => item.label === 'Workspace')
      const browseSessionsItem = workspaceMenu.submenu.find((item: any) => item.label === 'Browse Sessions...')
      
      browseSessionsItem.click()
      
      expect(mockServer.sessionShowSessions).toHaveBeenCalled()
    })

    it('calls onQuitAndKillDaemon callback when Exit and Kill Daemon clicked', () => {
      const onQuitAndKillDaemon = vi.fn()
      
      createApplicationMenu(null, mockServer as any, onQuitAndKillDaemon)
      
      const template = (Menu.buildFromTemplate as any).mock.calls[0][0]
      const fileMenu = template.find((item: any) => item.label === 'File')
      const exitItem = fileMenu.submenu.find((item: any) => item.label === 'Exit and Kill Daemon')
      
      exitItem.click()
      
      expect(onQuitAndKillDaemon).toHaveBeenCalled()
    })

    it('calls app.quit when Exit and Kill Daemon clicked without callback', () => {
      Object.defineProperty(process, 'platform', { value: 'linux' })
      
      createApplicationMenu(null, mockServer as any)
      
      const template = (Menu.buildFromTemplate as any).mock.calls[0][0]
      const fileMenu = template.find((item: any) => item.label === 'File')
      const exitItem = fileMenu.submenu.find((item: any) => item.label === 'Exit and Kill Daemon')
      
      exitItem.click()
      
      expect(app.quit).toHaveBeenCalled()
    })

    it('opens external link when Learn More clicked', async () => {
      createApplicationMenu(null, mockServer as any)
      
      const template = (Menu.buildFromTemplate as any).mock.calls[0][0]
      const helpMenu = template.find((item: any) => item.role === 'help')
      const learnMoreItem = helpMenu.submenu.find((item: any) => item.label === 'Learn More')
      
      await learnMoreItem.click()
      
      expect(shell.openExternal).toHaveBeenCalledWith('https://github.com')
    })
  })
})
