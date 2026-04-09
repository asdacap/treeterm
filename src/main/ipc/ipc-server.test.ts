import { describe, it, expect, vi, beforeEach } from 'vitest'

const { mockHandle, mockOn, mockGetAllWindows } = vi.hoisted(() => ({
  mockHandle: vi.fn<(...args: any[]) => any>(),
  mockOn: vi.fn<(...args: any[]) => any>(),
  mockGetAllWindows: vi.fn<() => any[]>(() => [])
}))

vi.mock('electron', () => ({
  ipcMain: {
    handle: mockHandle,
    on: mockOn
  },
  BrowserWindow: Object.assign(vi.fn<(...args: any[]) => any>(), {
    getAllWindows: mockGetAllWindows
  })
}))

import type { BrowserWindow } from 'electron'
import type { Session, ConnectionInfo, TTYSessionInfo, PortForwardInfo, PortForwardStatus } from '../../shared/types'
import { IpcServer } from './ipc-server'

describe('IpcServer', () => {
  let server: IpcServer

  beforeEach(() => {
    vi.clearAllMocks()
    server = new IpcServer()
  })

  describe('handle registration (invoke pattern)', () => {
    it('onPtyCreate registers handler on pty:create channel', () => {
      const handler = vi.fn<(...args: any[]) => any>()
      server.onPtyCreate(handler)
      expect(mockHandle).toHaveBeenCalledWith('pty:create', expect.any(Function))
    })

    it('onPtyCreate wrapper forwards event and args to handler', async () => {
      const handler = vi.fn<(...args: any[]) => Promise<string>>().mockResolvedValue('pty-123')
      // eslint-disable-next-line @typescript-eslint/no-unsafe-argument
      server.onPtyCreate(handler as any)
      const wrapper = mockHandle.mock.calls[0]![1] as (...args: any[]) => Promise<string>
      const fakeEvent = { sender: {} }
      const result = await wrapper(fakeEvent, '/home/user', undefined, undefined)
      expect(handler).toHaveBeenCalledWith(fakeEvent, '/home/user', undefined, undefined)
      expect(result).toBe('pty-123')
    })

    it('onSessionUpdate registers handler on session:update channel', () => {
      const handler = vi.fn<(...args: any[]) => any>()
      server.onSessionUpdate(handler)
      expect(mockHandle).toHaveBeenCalledWith('session:update', expect.any(Function))
    })

    it('onSettingsLoad registers handler on settings:load channel', () => {
      const handler = vi.fn<(...args: any[]) => any>()
      server.onSettingsLoad(handler)
      expect(mockHandle).toHaveBeenCalledWith('settings:load', expect.any(Function))
    })

    it('onFsReadFile registers handler on fs:readFile channel', () => {
      const handler = vi.fn<(...args: any[]) => any>()
      server.onFsReadFile(handler)
      expect(mockHandle).toHaveBeenCalledWith('fs:readFile', expect.any(Function))
    })

    it('onFsReadFile wrapper forwards args and returns result', async () => {
      const handler = vi.fn<(...args: any[]) => Promise<{ success: boolean }>>().mockResolvedValue({ success: true })
      // eslint-disable-next-line @typescript-eslint/no-unsafe-argument
      server.onFsReadFile(handler as any)
      const wrapper = mockHandle.mock.calls[0]![1] as (...args: any[]) => Promise<{ success: boolean }>
      const result = await wrapper({}, '/ws', '/file.txt')
      expect(handler).toHaveBeenCalledWith('/ws', '/file.txt')
      expect(result).toEqual({ success: true })
    })

    it('onAppGetWindowUuid registers handler on app:getWindowUuid channel', () => {
      const handler = vi.fn<(...args: any[]) => any>()
      server.onAppGetWindowUuid(handler)
      expect(mockHandle).toHaveBeenCalledWith('app:getWindowUuid', expect.any(Function))
    })

    it.each([
      ['onPtyAttach', 'pty:attach'],
      ['onSettingsSave', 'settings:save'],
      ['onFsReadDirectory', 'fs:readDirectory'],
      ['onFsWriteFile', 'fs:writeFile'],
      ['onFsSearchFiles', 'fs:searchFiles'],
      ['onSessionUpdate', 'session:update'],
      ['onDaemonShutdown', 'daemon:shutdown'],
      ['onDialogSelectFolder', 'dialog:selectFolder'],
      ['onDialogGetRecentDirectories', 'dialog:getRecentDirectories'],
      ['onSandboxIsAvailable', 'sandbox:isAvailable'],
      ['onAppGetInitialWorkspace', 'app:getInitialWorkspace'],
      ['onPtyCreateSession', 'pty:createSession'],
      ['onLocalConnect', 'local:connect'],
      ['onSshConnect', 'ssh:connect'],
      ['onSshDisconnect', 'ssh:disconnect'],
      ['onSshReconnect', 'ssh:reconnect'],
      ['onSshReconnectNow', 'ssh:reconnectNow'],
      ['onSshForceReconnect', 'ssh:forceReconnect'],
      ['onSshCancelReconnect', 'ssh:cancelReconnect'],
      ['onSshListConnections', 'ssh:listConnections'],
      ['onSshSaveConnection', 'ssh:saveConnection'],
      ['onSshGetSavedConnections', 'ssh:getSavedConnections'],
      ['onSshRemoveSavedConnection', 'ssh:removeSavedConnection'],
      ['onSshWatchBootstrapOutput', 'ssh:watchBootstrapOutput'],
      ['onSshUnwatchBootstrapOutput', 'ssh:unwatchBootstrapOutput'],
      ['onSshWatchTunnelOutput', 'ssh:watchTunnelOutput'],
      ['onSshUnwatchTunnelOutput', 'ssh:unwatchTunnelOutput'],
      ['onSshWatchDaemonOutput', 'ssh:watchDaemonOutput'],
      ['onSshUnwatchDaemonOutput', 'ssh:unwatchDaemonOutput'],
      ['onSshWatchConnectionStatus', 'ssh:watchConnectionStatus'],
      ['onSshUnwatchConnectionStatus', 'ssh:unwatchConnectionStatus'],
      ['onPtyList', 'pty:list'],
      ['onSessionLock', 'session:lock'],
      ['onSessionUnlock', 'session:unlock'],
      ['onSessionForceUnlock', 'session:forceUnlock'],
      ['onSshAddPortForward', 'ssh:addPortForward'],
      ['onSshRemovePortForward', 'ssh:removePortForward'],
      ['onSshListPortForwards', 'ssh:listPortForwards'],
      ['onSshWatchPortForwardOutput', 'ssh:watchPortForwardOutput'],
      ['onSshUnwatchPortForwardOutput', 'ssh:unwatchPortForwardOutput'],
      ['onClipboardReadText', 'clipboard:readText'],
      ['onExecStart', 'exec:start'],
    ] as const)('%s registers handler on %s channel', (method, channel) => {
      const handler = vi.fn<(...args: any[]) => any>()
      // eslint-disable-next-line @typescript-eslint/no-unsafe-call, @typescript-eslint/no-unsafe-member-access
      ;(server as any)[method](handler)
      expect(mockHandle).toHaveBeenCalledWith(channel, expect.any(Function))
    })

    it.each([
      ['onSettingsSave', 'settings:save'],
      ['onFsReadDirectory', 'fs:readDirectory'],
      ['onSessionUpdate', 'session:update'],
      ['onDaemonShutdown', 'daemon:shutdown'],
      ['onDialogSelectFolder', 'dialog:selectFolder'],
      ['onSandboxIsAvailable', 'sandbox:isAvailable'],
      ['onAppGetInitialWorkspace', 'app:getInitialWorkspace'],
    // eslint-disable-next-line @typescript-eslint/no-unused-vars
    ] as const)('%s wrapper forwards args to handler and returns result', async (method, _channel) => {
      const handler = vi.fn<(...args: any[]) => Promise<string>>().mockResolvedValue('result')
      // eslint-disable-next-line @typescript-eslint/no-unsafe-call, @typescript-eslint/no-unsafe-member-access
      ;(server as any)[method](handler)
      const wrapper = mockHandle.mock.calls[0]![1] as (...args: any[]) => Promise<string>
      const result = await wrapper({}, 'arg1', 'arg2')
      expect(handler).toHaveBeenCalledWith('arg1', 'arg2')
      expect(result).toBe('result')
    })

    it.each([
      ['onPtyCreate', 'pty:create'],
      ['onPtyAttach', 'pty:attach'],
    // eslint-disable-next-line @typescript-eslint/no-unused-vars
    ] as const)('%s wrapper forwards event and args to handler', async (method, _channel) => {
      const handler = vi.fn<(...args: any[]) => Promise<string>>().mockResolvedValue('result')
      // eslint-disable-next-line @typescript-eslint/no-unsafe-call, @typescript-eslint/no-unsafe-member-access
      ;(server as any)[method](handler)
      const wrapper = mockHandle.mock.calls[0]![1] as (...args: any[]) => Promise<string>
      const mockEvent = { sender: {} }
      const result = await wrapper(mockEvent, 'arg1', 'arg2')
      expect(handler).toHaveBeenCalledWith(mockEvent, 'arg1', 'arg2')
      expect(result).toBe('result')
    })
  })

  describe('on registration (send pattern)', () => {
    it('onAppCloseConfirmed registers handler on app:close-confirmed channel', () => {
      const handler = vi.fn<(...args: any[]) => void>()
      server.onAppCloseConfirmed(handler)
      expect(mockOn).toHaveBeenCalledWith('app:close-confirmed', expect.any(Function))
    })

    it('onAppCloseCancelled registers handler on app:close-cancelled channel', () => {
      const handler = vi.fn<(...args: any[]) => void>()
      server.onAppCloseCancelled(handler)
      expect(mockOn).toHaveBeenCalledWith('app:close-cancelled', expect.any(Function))
    })

    it.each([
      ['onPtyWrite', 'pty:write'],
      ['onPtyResize', 'pty:resize'],
      ['onPtyKill', 'pty:kill'],
      ['onClipboardWriteText', 'clipboard:writeText'],
      ['onExecKill', 'exec:kill'],
    ] as const)('%s registers handler on %s channel', (method, channel) => {
      const handler = vi.fn<(...args: any[]) => void>()
      // eslint-disable-next-line @typescript-eslint/no-unsafe-call, @typescript-eslint/no-unsafe-member-access
      ;(server as any)[method](handler)
      expect(mockOn).toHaveBeenCalledWith(channel, expect.any(Function))
    })
  })

  describe('per-window event emitters', () => {
    it('ptyEventTo sends to specific window with correct channel and args', () => {
      const mockSend = vi.fn<(...args: any[]) => void>()
      const mockWindow = { webContents: { send: mockSend } } as unknown as BrowserWindow

      const dataBytes = new TextEncoder().encode('hello-data')
      server.ptyEventTo(mockWindow, 'pty-1', { type: 'data', data: dataBytes })
      expect(mockSend).toHaveBeenCalledWith('pty:event', 'pty-1', { type: 'data', data: dataBytes })
    })

    it('ptyEventTo sends exit to specific window with correct channel and args', () => {
      const mockSend = vi.fn<(...args: any[]) => void>()
      const mockWindow = { webContents: { send: mockSend } } as unknown as BrowserWindow

      server.ptyEventTo(mockWindow, 'pty-1', { type: 'exit', exitCode: 0 })
      expect(mockSend).toHaveBeenCalledWith('pty:event', 'pty-1', { type: 'exit', exitCode: 0 })
    })

    it('settingsOpenTo sends to specific window with correct channel', () => {
      const mockSend = vi.fn<(...args: any[]) => void>()
      const mockWindow = { webContents: { send: mockSend } } as unknown as BrowserWindow

      server.settingsOpenTo(mockWindow)
      expect(mockSend).toHaveBeenCalledWith('settings:open')
    })

    it('appReadyTo sends to specific window', () => {
      const mockSend = vi.fn<(...args: any[]) => void>()
      const mockWindow = { webContents: { send: mockSend } } as unknown as BrowserWindow

      server.appReadyTo(mockWindow)
      expect(mockSend).toHaveBeenCalledWith('app:ready')
    })

    it('capsLockEventTo sends to specific window', () => {
      const mockSend = vi.fn<(...args: any[]) => void>()
      const mockWindow = { webContents: { send: mockSend } } as unknown as BrowserWindow

      server.capsLockEventTo(mockWindow, true as unknown as { type: string; key: string; code: string })
      expect(mockSend).toHaveBeenCalledWith('capslock-event', true)
    })

    it('activeProcessesOpenTo sends to specific window', () => {
      const mockSend = vi.fn<(...args: any[]) => void>()
      const mockWindow = { webContents: { send: mockSend } } as unknown as BrowserWindow

      server.activeProcessesOpenTo(mockWindow)
      expect(mockSend).toHaveBeenCalledWith('active-processes:open')
    })
  })

  describe('broadcast event emitters', () => {
    it('sessionSync broadcasts to all windows', () => {
      const mockSend1 = vi.fn<(...args: any[]) => void>()
      const mockSend2 = vi.fn<(...args: any[]) => void>()
      mockGetAllWindows.mockReturnValue([
        { webContents: { send: mockSend1 } },
        { webContents: { send: mockSend2 } },
      ])

      const sessionData = { id: 'session-1' } as unknown as Session
      server.sessionSync('local', sessionData)
      expect(mockSend1).toHaveBeenCalledWith('session:sync', 'local', sessionData)
      expect(mockSend2).toHaveBeenCalledWith('session:sync', 'local', sessionData)
    })

    it('daemonDisconnected broadcasts to all windows', () => {
      const mockSend = vi.fn<(...args: any[]) => void>()
      mockGetAllWindows.mockReturnValue([{ webContents: { send: mockSend } }])

      server.daemonDisconnected()
      expect(mockSend).toHaveBeenCalledWith('daemon:disconnected')
    })

    it('daemonSessions broadcasts to all windows', () => {
      const mockSend = vi.fn<(...args: any[]) => void>()
      mockGetAllWindows.mockReturnValue([{ webContents: { send: mockSend } }])

      const sessions = [{ id: 's1' }] as unknown as TTYSessionInfo[]
      server.daemonSessions(sessions)
      expect(mockSend).toHaveBeenCalledWith('daemon:sessions', sessions)
    })

    it('sshConnectionStatus broadcasts to all windows', () => {
      const mockSend = vi.fn<(...args: any[]) => void>()
      mockGetAllWindows.mockReturnValue([{ webContents: { send: mockSend } }])

      const info = { id: 'conn-1', status: 'connected' } as unknown as ConnectionInfo
      server.sshConnectionStatus(info)
      expect(mockSend).toHaveBeenCalledWith('ssh:connectionStatus', info)
    })

    it('sshBootstrapOutput broadcasts to all windows', () => {
      const mockSend = vi.fn<(...args: any[]) => void>()
      mockGetAllWindows.mockReturnValue([{ webContents: { send: mockSend } }])

      server.sshBootstrapOutput('conn-1', 'log line')
      expect(mockSend).toHaveBeenCalledWith('ssh:bootstrapOutput', 'conn-1', 'log line')
    })

    it('sshTunnelOutput broadcasts to all windows', () => {
      const mockSend = vi.fn<(...args: any[]) => void>()
      mockGetAllWindows.mockReturnValue([{ webContents: { send: mockSend } }])

      server.sshTunnelOutput('conn-1', 'log line')
      expect(mockSend).toHaveBeenCalledWith('ssh:tunnelOutput', 'conn-1', 'log line')
    })

    it('sshDaemonOutput broadcasts to all windows', () => {
      const mockSend = vi.fn<(...args: any[]) => void>()
      mockGetAllWindows.mockReturnValue([{ webContents: { send: mockSend } }])

      server.sshDaemonOutput('conn-1', 'log line')
      expect(mockSend).toHaveBeenCalledWith('ssh:daemonOutput', 'conn-1', 'log line')
    })

    it('broadcasts to no windows when none exist', () => {
      mockGetAllWindows.mockReturnValue([])
      expect(() => { server.daemonDisconnected() }).not.toThrow()
    })

    it('execEvent broadcasts to all windows', () => {
      const mockSend = vi.fn<(...args: any[]) => void>()
      mockGetAllWindows.mockReturnValue([{ webContents: { send: mockSend } }])

      server.execEvent('exec-1', { type: 'stdout', data: 'output' })
      expect(mockSend).toHaveBeenCalledWith('exec:event', 'exec-1', { type: 'stdout', data: 'output' })
    })

    it('sshAutoConnected broadcasts to all windows', () => {
      const mockSend = vi.fn<(...args: any[]) => void>()
      mockGetAllWindows.mockReturnValue([{ webContents: { send: mockSend } }])

      const session = { id: 's1' } as unknown as Session
      const info = { id: 'conn-1' } as unknown as ConnectionInfo
      server.sshAutoConnected(session, info)
      expect(mockSend).toHaveBeenCalledWith('ssh:autoConnected', session, info)
    })

    it('connectionReconnected broadcasts to all windows', () => {
      const mockSend = vi.fn<(...args: any[]) => void>()
      mockGetAllWindows.mockReturnValue([{ webContents: { send: mockSend } }])

      const session = { id: 's1' } as unknown as Session
      const info = { id: 'conn-1' } as unknown as ConnectionInfo
      server.connectionReconnected(session, info)
      expect(mockSend).toHaveBeenCalledWith('connection:reconnected', session, info)
    })

    it('sshPortForwardStatus broadcasts to all windows', () => {
      const mockSend = vi.fn<(...args: any[]) => void>()
      mockGetAllWindows.mockReturnValue([{ webContents: { send: mockSend } }])

      const info = { id: 'pf-1', connectionId: 'conn-1', localPort: 8080, remoteHost: 'localhost', remotePort: 80, status: 'active' as PortForwardStatus } as PortForwardInfo
      server.sshPortForwardStatus(info)
      expect(mockSend).toHaveBeenCalledWith('ssh:portForwardStatus', info)
    })

    it('sshPortForwardOutput broadcasts to all windows', () => {
      const mockSend = vi.fn<(...args: any[]) => void>()
      mockGetAllWindows.mockReturnValue([{ webContents: { send: mockSend } }])

      server.sshPortForwardOutput('pf-1', 'log line')
      expect(mockSend).toHaveBeenCalledWith('ssh:portForwardOutput', 'pf-1', 'log line')
    })
  })
})
