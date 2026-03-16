import { describe, it, expect, vi, beforeEach } from 'vitest'

const { mockHandle, mockOn } = vi.hoisted(() => ({
  mockHandle: vi.fn(),
  mockOn: vi.fn()
}))

vi.mock('electron', () => ({
  ipcMain: {
    handle: mockHandle,
    on: mockOn
  },
  BrowserWindow: vi.fn()
}))

import { IpcServer } from './ipc-server'

describe('IpcServer', () => {
  let server: IpcServer

  beforeEach(() => {
    vi.clearAllMocks()
    server = new IpcServer()
  })

  describe('handle registration (invoke pattern)', () => {
    it('onPtyCreate registers handler on pty:create channel', () => {
      const handler = vi.fn()
      server.onPtyCreate(handler)
      expect(mockHandle).toHaveBeenCalledWith('pty:create', expect.any(Function))
    })

    it('onPtyCreate wrapper forwards args to handler and returns result', async () => {
      const handler = vi.fn().mockResolvedValue('pty-123')
      server.onPtyCreate(handler)
      const wrapper = mockHandle.mock.calls[0][1]
      const fakeEvent = {}
      const result = await wrapper(fakeEvent, '/home/user', undefined, undefined)
      expect(handler).toHaveBeenCalledWith('/home/user', undefined, undefined)
      expect(result).toBe('pty-123')
    })

    it('onGitGetInfo registers handler on git:getInfo channel', () => {
      const handler = vi.fn()
      server.onGitGetInfo(handler)
      expect(mockHandle).toHaveBeenCalledWith('git:getInfo', expect.any(Function))
    })

    it('onSessionCreate registers handler on session:create channel', () => {
      const handler = vi.fn()
      server.onSessionCreate(handler)
      expect(mockHandle).toHaveBeenCalledWith('session:create', expect.any(Function))
    })

    it('onSessionCreate wrapper forwards args', async () => {
      const handler = vi.fn().mockResolvedValue({ id: 'session-1' })
      server.onSessionCreate(handler)
      const wrapper = mockHandle.mock.calls[0][1]
      const workspaces = [{ id: 'ws-1' }]
      const result = await wrapper({}, workspaces)
      expect(handler).toHaveBeenCalledWith(workspaces)
      expect(result).toEqual({ id: 'session-1' })
    })

    it('onSettingsLoad registers handler on settings:load channel', () => {
      const handler = vi.fn()
      server.onSettingsLoad(handler)
      expect(mockHandle).toHaveBeenCalledWith('settings:load', expect.any(Function))
    })

    it('onFsReadFile registers handler on fs:readFile channel', () => {
      const handler = vi.fn()
      server.onFsReadFile(handler)
      expect(mockHandle).toHaveBeenCalledWith('fs:readFile', expect.any(Function))
    })

    it('onFsReadFile wrapper forwards args and returns result', async () => {
      const handler = vi.fn().mockResolvedValue({ success: true })
      server.onFsReadFile(handler)
      const wrapper = mockHandle.mock.calls[0][1]
      const result = await wrapper({}, '/ws', '/file.txt')
      expect(handler).toHaveBeenCalledWith('/ws', '/file.txt')
      expect(result).toEqual({ success: true })
    })

    it('onAppGetWindowUuid registers handler on app:getWindowUuid channel', () => {
      const handler = vi.fn()
      server.onAppGetWindowUuid(handler)
      expect(mockHandle).toHaveBeenCalledWith('app:getWindowUuid', expect.any(Function))
    })
  })

  describe('on registration (send pattern)', () => {
    it('onPtyWrite registers handler on pty:write channel', () => {
      const handler = vi.fn()
      server.onPtyWrite(handler)
      expect(mockOn).toHaveBeenCalledWith('pty:write', expect.any(Function))
    })

    it('onPtyWrite wrapper forwards args to handler', () => {
      const handler = vi.fn()
      server.onPtyWrite(handler)
      const wrapper = mockOn.mock.calls[0][1]
      wrapper({}, 'pty-123', 'data')
      expect(handler).toHaveBeenCalledWith('pty-123', 'data')
    })

    it('onPtyResize registers handler on pty:resize channel', () => {
      const handler = vi.fn()
      server.onPtyResize(handler)
      expect(mockOn).toHaveBeenCalledWith('pty:resize', expect.any(Function))
    })

    it('onPtyKill registers handler on pty:kill channel', () => {
      const handler = vi.fn()
      server.onPtyKill(handler)
      expect(mockOn).toHaveBeenCalledWith('pty:kill', expect.any(Function))
    })

    it('onAppCloseConfirmed registers handler on app:close-confirmed channel', () => {
      const handler = vi.fn()
      server.onAppCloseConfirmed(handler)
      expect(mockOn).toHaveBeenCalledWith('app:close-confirmed', expect.any(Function))
    })

    it('onAppCloseCancelled registers handler on app:close-cancelled channel', () => {
      const handler = vi.fn()
      server.onAppCloseCancelled(handler)
      expect(mockOn).toHaveBeenCalledWith('app:close-cancelled', expect.any(Function))
    })
  })

  describe('event emitters (main → renderer)', () => {
    it('ptyData sends to window webContents with correct channel and args', () => {
      const mockSend = vi.fn()
      const mockWindow = { webContents: { send: mockSend } } as any
      server.setWindow(mockWindow)

      server.ptyData('pty-1', 'hello-data')
      expect(mockSend).toHaveBeenCalledWith('pty:data', 'pty-1', 'hello-data')
    })

    it('ptyExit sends to window webContents with correct channel and args', () => {
      const mockSend = vi.fn()
      const mockWindow = { webContents: { send: mockSend } } as any
      server.setWindow(mockWindow)

      server.ptyExit('pty-1', 0)
      expect(mockSend).toHaveBeenCalledWith('pty:exit', 'pty-1', 0)
    })

    it('settingsOpen sends to window webContents with correct channel', () => {
      const mockSend = vi.fn()
      const mockWindow = { webContents: { send: mockSend } } as any
      server.setWindow(mockWindow)

      server.settingsOpen()
      expect(mockSend).toHaveBeenCalledWith('settings:open')
    })

    it('appConfirmClose sends to window webContents', () => {
      const mockSend = vi.fn()
      const mockWindow = { webContents: { send: mockSend } } as any
      server.setWindow(mockWindow)

      server.appConfirmClose()
      expect(mockSend).toHaveBeenCalledWith('app:confirm-close')
    })

    it('sessionSync sends to window with correct args', () => {
      const mockSend = vi.fn()
      const mockWindow = { webContents: { send: mockSend } } as any
      server.setWindow(mockWindow)

      const sessionData = { id: 'session-1' }
      server.sessionSync(sessionData as any)
      expect(mockSend).toHaveBeenCalledWith('session:sync', sessionData)
    })

    it('daemonDisconnected sends to window', () => {
      const mockSend = vi.fn()
      const mockWindow = { webContents: { send: mockSend } } as any
      server.setWindow(mockWindow)

      server.daemonDisconnected()
      expect(mockSend).toHaveBeenCalledWith('daemon:disconnected')
    })

    it('terminalNew sends to window', () => {
      const mockSend = vi.fn()
      const mockWindow = { webContents: { send: mockSend } } as any
      server.setWindow(mockWindow)

      server.terminalNew()
      expect(mockSend).toHaveBeenCalledWith('terminal:new')
    })
  })

  describe('setWindow', () => {
    it('sets the window for event emission', () => {
      const mockSend = vi.fn()
      const mockWindow = { webContents: { send: mockSend } } as any
      server.setWindow(mockWindow)

      server.ptyData('pty-1', 'data')
      expect(mockSend).toHaveBeenCalled()
    })

    it('null window does not throw on emit', () => {
      server.setWindow(null)
      expect(() => server.ptyData('pty-1', 'data')).not.toThrow()
      expect(() => server.settingsOpen()).not.toThrow()
      expect(() => server.daemonDisconnected()).not.toThrow()
    })

    it('no window set (default) does not throw on emit', () => {
      expect(() => server.ptyData('pty-1', 'data')).not.toThrow()
    })
  })
})
