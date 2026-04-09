import { describe, it, expect, vi, beforeEach } from 'vitest'

const { mockInvoke, mockSend, mockOn, mockRemoveListener } = vi.hoisted(() => ({
  mockInvoke: vi.fn<(...args: any[]) => any>(),
  mockSend: vi.fn<(...args: any[]) => void>(),
  mockOn: vi.fn<(...args: any[]) => any>(),
  mockRemoveListener: vi.fn<(...args: any[]) => void>()
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
      mockInvoke.mockResolvedValue({ success: true, sessionId: 'pty-123' })
      const result = await client.ptyCreate('local', 'handle-1', '/home/user')
      expect(mockInvoke).toHaveBeenCalledWith('pty:create', 'local', 'handle-1', '/home/user')
      expect(result).toEqual({ success: true, sessionId: 'pty-123' })
    })

    it('sessionUpdate calls ipcRenderer.invoke with correct channel and args', async () => {
      const workspaces = [{ id: 'ws-1', path: '/test' }]
      mockInvoke.mockResolvedValue({ success: true, session: { id: 'session-1' } })
      // eslint-disable-next-line @typescript-eslint/no-unsafe-argument
      const result = await client.sessionUpdate('session-1', workspaces as any, 'uuid-1', 5)
      expect(mockInvoke).toHaveBeenCalledWith('session:update', 'session-1', workspaces, 'uuid-1', 5)
      expect(result).toEqual({ success: true, session: { id: 'session-1' } })
    })

    it('fsReadFile calls ipcRenderer.invoke with correct channel and args', async () => {
      mockInvoke.mockResolvedValue({ success: true, content: 'hello' })
      const result = await client.fsReadFile('local', '/ws', '/file.txt')
      expect(mockInvoke).toHaveBeenCalledWith('fs:readFile', 'local', '/ws', '/file.txt')
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

    it('clipboardReadText calls ipcRenderer.invoke with correct channel', async () => {
      mockInvoke.mockResolvedValue('clipboard content')
      const result = await client.clipboardReadText()
      expect(mockInvoke).toHaveBeenCalledWith('clipboard:readText')
      expect(result).toBe('clipboard content')
    })

    it.each([
      ['ptyAttach', 'pty:attach'],
      ['ptyList', 'pty:list'],
      ['settingsSave', 'settings:save'],
      ['fsReadDirectory', 'fs:readDirectory'],
      ['fsWriteFile', 'fs:writeFile'],
      ['fsSearchFiles', 'fs:searchFiles'],
      ['sessionUpdate', 'session:update'],
      ['daemonShutdown', 'daemon:shutdown'],
      ['dialogSelectFolder', 'dialog:selectFolder'],
      ['dialogGetRecentDirectories', 'dialog:getRecentDirectories'],
      ['sandboxIsAvailable', 'sandbox:isAvailable'],
      ['appGetInitialWorkspace', 'app:getInitialWorkspace'],
      ['ptyCreateSession', 'pty:createSession'],
      ['sessionLock', 'session:lock'],
      ['sessionUnlock', 'session:unlock'],
      ['sessionForceUnlock', 'session:forceUnlock'],
      ['sshConnect', 'ssh:connect'],
      ['sshDisconnect', 'ssh:disconnect'],
      ['sshReconnect', 'ssh:reconnect'],
      ['sshReconnectNow', 'ssh:reconnectNow'],
      ['sshForceReconnect', 'ssh:forceReconnect'],
      ['sshCancelReconnect', 'ssh:cancelReconnect'],
      ['sshListConnections', 'ssh:listConnections'],
      ['sshSaveConnection', 'ssh:saveConnection'],
      ['sshGetSavedConnections', 'ssh:getSavedConnections'],
      ['sshRemoveSavedConnection', 'ssh:removeSavedConnection'],
      ['sshWatchBootstrapOutput', 'ssh:watchBootstrapOutput'],
      ['sshUnwatchBootstrapOutput', 'ssh:unwatchBootstrapOutput'],
      ['sshWatchTunnelOutput', 'ssh:watchTunnelOutput'],
      ['sshUnwatchTunnelOutput', 'ssh:unwatchTunnelOutput'],
      ['sshWatchDaemonOutput', 'ssh:watchDaemonOutput'],
      ['sshUnwatchDaemonOutput', 'ssh:unwatchDaemonOutput'],
      ['sshWatchConnectionStatus', 'ssh:watchConnectionStatus'],
      ['sshUnwatchConnectionStatus', 'ssh:unwatchConnectionStatus'],
      ['sshAddPortForward', 'ssh:addPortForward'],
      ['sshRemovePortForward', 'ssh:removePortForward'],
      ['sshListPortForwards', 'ssh:listPortForwards'],
      ['sshWatchPortForwardOutput', 'ssh:watchPortForwardOutput'],
      ['sshUnwatchPortForwardOutput', 'ssh:unwatchPortForwardOutput'],
      ['execStart', 'exec:start'],
    ] as const)('%s calls ipcRenderer.invoke with %s channel', async (method, channel) => {
      mockInvoke.mockResolvedValue('test-result')
      // eslint-disable-next-line @typescript-eslint/no-unsafe-call, @typescript-eslint/no-unsafe-member-access
      const result = await (client as any)[method]('arg1', 'arg2') as string
      expect(mockInvoke).toHaveBeenCalledWith(channel, 'arg1', 'arg2')
      expect(result).toBe('test-result')
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
      client.ptyKill('local', 'pty-123')
      expect(mockSend).toHaveBeenCalledWith('pty:kill', 'local', 'pty-123')
    })

    it('appCloseConfirmed calls ipcRenderer.send with correct channel', () => {
      client.appCloseConfirmed()
      expect(mockSend).toHaveBeenCalledWith('app:close-confirmed')
    })

    it('appCloseCancelled calls ipcRenderer.send with correct channel', () => {
      client.appCloseCancelled()
      expect(mockSend).toHaveBeenCalledWith('app:close-cancelled')
    })

    it('clipboardWriteText calls ipcRenderer.send with correct channel and args', () => {
      client.clipboardWriteText('copied text')
      expect(mockSend).toHaveBeenCalledWith('clipboard:writeText', 'copied text')
    })

    it('execKill calls ipcRenderer.send with correct channel and args', () => {
      client.execKill('exec-1')
      expect(mockSend).toHaveBeenCalledWith('exec:kill', 'exec-1')
    })
  })

  describe('event listener pattern (on methods)', () => {
    it('onPtyEvent registers listener on correct channel', () => {
      const callback = vi.fn<(...args: any[]) => void>()
      client.onPtyEvent(callback)
      expect(mockOn).toHaveBeenCalledWith('pty:event', expect.any(Function))
    })

    it('onPtyEvent callback strips IpcRendererEvent arg', () => {
      const callback = vi.fn<(...args: any[]) => void>()
      client.onPtyEvent(callback)
      const registeredHandler = mockOn.mock.calls[0]![1] as (...args: any[]) => void
      const fakeEvent = {} // IpcRendererEvent
      registeredHandler(fakeEvent, 'pty-1', { type: 'data', data: 'data-chunk' })
      expect(callback).toHaveBeenCalledWith('pty-1', { type: 'data', data: 'data-chunk' })
    })

    it('onPtyEvent unsubscribe calls removeListener', () => {
      const callback = vi.fn<(...args: any[]) => void>()
      const unsubscribe = client.onPtyEvent(callback)
      const registeredHandler = mockOn.mock.calls[0]![1] as (...args: any[]) => void
      unsubscribe()
      expect(mockRemoveListener).toHaveBeenCalledWith('pty:event', registeredHandler)
    })

    it('onSessionSync registers and forwards correctly', () => {
      const callback = vi.fn<(...args: any[]) => void>()
      client.onSessionSync(callback)
      expect(mockOn).toHaveBeenCalledWith('session:sync', expect.any(Function))
      const handler = mockOn.mock.calls[0]![1] as (...args: any[]) => void
      handler({}, { id: 'session-1' })
      expect(callback).toHaveBeenCalledWith({ id: 'session-1' })
    })

    it('onDaemonDisconnected registers on correct channel', () => {
      const callback = vi.fn<() => void>()
      client.onDaemonDisconnected(callback)
      expect(mockOn).toHaveBeenCalledWith('daemon:disconnected', expect.any(Function))
    })

    it('onDaemonDisconnected unsubscribe works', () => {
      const callback = vi.fn<() => void>()
      const unsub = client.onDaemonDisconnected(callback)
      const handler = mockOn.mock.calls[0]![1] as (...args: any[]) => void
      unsub()
      expect(mockRemoveListener).toHaveBeenCalledWith('daemon:disconnected', handler)
    })

    it('onSettingsOpen registers and fires callback', () => {
      const callback = vi.fn<() => void>()
      client.onSettingsOpen(callback)
      expect(mockOn).toHaveBeenCalledWith('settings:open', expect.any(Function))
      const handler = mockOn.mock.calls[0]![1] as (...args: any[]) => void
      handler()
      expect(callback).toHaveBeenCalled()
    })

    it('onAppConfirmClose registers and fires callback', () => {
      const callback = vi.fn<() => void>()
      client.onAppConfirmClose(callback)
      expect(mockOn).toHaveBeenCalledWith('app:confirm-close', expect.any(Function))
      const handler = mockOn.mock.calls[0]![1] as (...args: any[]) => void
      handler()
      expect(callback).toHaveBeenCalled()
    })

    it.each([
      ['onAppReady', 'app:ready'],
      ['onCapsLockEvent', 'capslock-event'],
      ['onDaemonSessions', 'daemon:sessions'],
      ['onSshAutoConnected', 'ssh:autoConnected'],
      ['onConnectionReconnected', 'connection:reconnected'],
      ['onActiveProcessesOpen', 'active-processes:open'],
      ['onSshConnectionStatus', 'ssh:connectionStatus'],
      ['onSshBootstrapOutput', 'ssh:bootstrapOutput'],
      ['onSshTunnelOutput', 'ssh:tunnelOutput'],
      ['onSshDaemonOutput', 'ssh:daemonOutput'],
      ['onSshPortForwardStatus', 'ssh:portForwardStatus'],
      ['onSshPortForwardOutput', 'ssh:portForwardOutput'],
      ['onExecEvent', 'exec:event'],
    ] as const)('%s registers listener on %s and unsubscribe works', (method, channel) => {
      const callback = vi.fn<(...args: any[]) => void>()
      // eslint-disable-next-line @typescript-eslint/no-unsafe-call, @typescript-eslint/no-unsafe-member-access
      const unsub = (client as any)[method](callback) as () => void
      expect(mockOn).toHaveBeenCalledWith(channel, expect.any(Function))
      const handler = mockOn.mock.calls[0]![1] as (...args: any[]) => void
      unsub()
      expect(mockRemoveListener).toHaveBeenCalledWith(channel, handler)
    })
  })
})
