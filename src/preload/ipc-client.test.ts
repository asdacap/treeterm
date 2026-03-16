import { describe, it, expect, vi, beforeEach } from 'vitest'

const { mockInvoke, mockSend, mockOn, mockRemoveListener } = vi.hoisted(() => ({
  mockInvoke: vi.fn(),
  mockSend: vi.fn(),
  mockOn: vi.fn(),
  mockRemoveListener: vi.fn()
}))

vi.mock('electron', () => ({
  ipcRenderer: {
    invoke: mockInvoke,
    send: mockSend,
    on: mockOn,
    removeListener: mockRemoveListener
  }
}))

import { IpcClient } from './ipc-client'

describe('IpcClient', () => {
  let client: IpcClient

  beforeEach(() => {
    vi.clearAllMocks()
    client = new IpcClient()
  })

  describe('invoke pattern (request methods)', () => {
    it('ptyCreate calls ipcRenderer.invoke with correct channel and args', async () => {
      mockInvoke.mockResolvedValue('pty-123')
      const result = await client.ptyCreate('/home/user')
      expect(mockInvoke).toHaveBeenCalledWith('pty:create', '/home/user')
      expect(result).toBe('pty-123')
    })

    it('gitGetInfo calls ipcRenderer.invoke with correct channel and args', async () => {
      mockInvoke.mockResolvedValue({ branch: 'main' })
      const result = await client.gitGetInfo('/repo')
      expect(mockInvoke).toHaveBeenCalledWith('git:getInfo', '/repo')
      expect(result).toEqual({ branch: 'main' })
    })

    it('sessionCreate calls ipcRenderer.invoke with correct channel and args', async () => {
      const workspaces = [{ id: 'ws-1', path: '/test' }]
      mockInvoke.mockResolvedValue({ id: 'session-1' })
      const result = await client.sessionCreate(workspaces as any)
      expect(mockInvoke).toHaveBeenCalledWith('session:create', workspaces)
      expect(result).toEqual({ id: 'session-1' })
    })

    it('fsReadFile calls ipcRenderer.invoke with correct channel and args', async () => {
      mockInvoke.mockResolvedValue({ success: true, content: 'hello' })
      const result = await client.fsReadFile('/ws', '/file.txt')
      expect(mockInvoke).toHaveBeenCalledWith('fs:readFile', '/ws', '/file.txt')
      expect(result).toEqual({ success: true, content: 'hello' })
    })

    it('settingsLoad calls ipcRenderer.invoke with correct channel', async () => {
      mockInvoke.mockResolvedValue({ theme: 'dark' })
      const result = await client.settingsLoad()
      expect(mockInvoke).toHaveBeenCalledWith('settings:load')
      expect(result).toEqual({ theme: 'dark' })
    })

    it('appGetWindowUuid calls ipcRenderer.invoke with correct channel', async () => {
      mockInvoke.mockResolvedValue('uuid-abc')
      const result = await client.appGetWindowUuid()
      expect(mockInvoke).toHaveBeenCalledWith('app:getWindowUuid')
      expect(result).toBe('uuid-abc')
    })

    it('sessionList calls ipcRenderer.invoke with correct channel', async () => {
      mockInvoke.mockResolvedValue([{ id: 'session-1' }])
      const result = await client.sessionList()
      expect(mockInvoke).toHaveBeenCalledWith('session:list')
      expect(result).toEqual([{ id: 'session-1' }])
    })
  })

  describe('send pattern (fire-and-forget methods)', () => {
    it('ptyWrite calls ipcRenderer.send with correct channel and args', () => {
      client.ptyWrite('pty-123', 'hello')
      expect(mockSend).toHaveBeenCalledWith('pty:write', 'pty-123', 'hello')
    })

    it('ptyResize calls ipcRenderer.send with correct channel and args', () => {
      client.ptyResize('pty-123', 80, 24)
      expect(mockSend).toHaveBeenCalledWith('pty:resize', 'pty-123', 80, 24)
    })

    it('ptyKill calls ipcRenderer.send with correct channel and args', () => {
      client.ptyKill('pty-123')
      expect(mockSend).toHaveBeenCalledWith('pty:kill', 'pty-123')
    })

    it('appCloseConfirmed calls ipcRenderer.send with correct channel', () => {
      client.appCloseConfirmed()
      expect(mockSend).toHaveBeenCalledWith('app:close-confirmed')
    })

    it('appCloseCancelled calls ipcRenderer.send with correct channel', () => {
      client.appCloseCancelled()
      expect(mockSend).toHaveBeenCalledWith('app:close-cancelled')
    })
  })

  describe('event listener pattern (on methods)', () => {
    it('onPtyData registers listener on correct channel', () => {
      const callback = vi.fn()
      client.onPtyData(callback)
      expect(mockOn).toHaveBeenCalledWith('pty:data', expect.any(Function))
    })

    it('onPtyData callback strips IpcRendererEvent arg', () => {
      const callback = vi.fn()
      client.onPtyData(callback)
      const registeredHandler = mockOn.mock.calls[0][1]
      const fakeEvent = {} // IpcRendererEvent
      registeredHandler(fakeEvent, 'pty-1', 'data-chunk')
      expect(callback).toHaveBeenCalledWith('pty-1', 'data-chunk')
    })

    it('onPtyData unsubscribe calls removeListener', () => {
      const callback = vi.fn()
      const unsubscribe = client.onPtyData(callback)
      const registeredHandler = mockOn.mock.calls[0][1]
      unsubscribe()
      expect(mockRemoveListener).toHaveBeenCalledWith('pty:data', registeredHandler)
    })

    it('onPtyExit registers listener on correct channel', () => {
      const callback = vi.fn()
      client.onPtyExit(callback)
      expect(mockOn).toHaveBeenCalledWith('pty:exit', expect.any(Function))
    })

    it('onPtyExit callback strips IpcRendererEvent arg', () => {
      const callback = vi.fn()
      client.onPtyExit(callback)
      const registeredHandler = mockOn.mock.calls[0][1]
      registeredHandler({}, 'pty-1', 0)
      expect(callback).toHaveBeenCalledWith('pty-1', 0)
    })

    it('onSessionSync registers and forwards correctly', () => {
      const callback = vi.fn()
      client.onSessionSync(callback)
      expect(mockOn).toHaveBeenCalledWith('session:sync', expect.any(Function))
      const handler = mockOn.mock.calls[0][1]
      handler({}, { id: 'session-1' })
      expect(callback).toHaveBeenCalledWith({ id: 'session-1' })
    })

    it('onDaemonDisconnected registers on correct channel', () => {
      const callback = vi.fn()
      client.onDaemonDisconnected(callback)
      expect(mockOn).toHaveBeenCalledWith('daemon:disconnected', expect.any(Function))
    })

    it('onDaemonDisconnected unsubscribe works', () => {
      const callback = vi.fn()
      const unsub = client.onDaemonDisconnected(callback)
      const handler = mockOn.mock.calls[0][1]
      unsub()
      expect(mockRemoveListener).toHaveBeenCalledWith('daemon:disconnected', handler)
    })

    it('onSettingsOpen registers and fires callback', () => {
      const callback = vi.fn()
      client.onSettingsOpen(callback)
      expect(mockOn).toHaveBeenCalledWith('settings:open', expect.any(Function))
      const handler = mockOn.mock.calls[0][1]
      handler()
      expect(callback).toHaveBeenCalled()
    })

    it('onAppConfirmClose registers and fires callback', () => {
      const callback = vi.fn()
      client.onAppConfirmClose(callback)
      expect(mockOn).toHaveBeenCalledWith('app:confirm-close', expect.any(Function))
      const handler = mockOn.mock.calls[0][1]
      handler()
      expect(callback).toHaveBeenCalled()
    })
  })
})
