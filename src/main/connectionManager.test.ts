import { describe, it, expect, vi, beforeEach } from 'vitest'
import type { PortForwardConfig } from '../shared/types'

// Mock ssh module before importing connectionManager
let tunnelBootstrapOutputCallback: ((line: string) => void) | null = null
let tunnelOutputCallback: ((line: string) => void) | null = null
let tunnelDisconnectCallback: ((error?: string) => void) | null = null

const mockTunnelInstance = {
  connect: vi.fn().mockResolvedValue('/tmp/test.sock'),
  disconnect: vi.fn(),
  onBootstrapOutput: vi.fn().mockImplementation((cb: (line: string) => void) => {
    tunnelBootstrapOutputCallback = cb
    return () => { tunnelBootstrapOutputCallback = null }
  }),
  onTunnelOutput: vi.fn().mockImplementation((cb: (line: string) => void) => {
    tunnelOutputCallback = cb
    return () => { tunnelOutputCallback = null }
  }),
  onDisconnect: vi.fn().mockImplementation((cb: (error?: string) => void) => {
    tunnelDisconnectCallback = cb
    return () => { tunnelDisconnectCallback = null }
  }),
  getBootstrapOutput: vi.fn().mockReturnValue(['scrollback-line']),
  getTunnelOutput: vi.fn().mockReturnValue([]),
}

vi.mock('./ssh', () => {
  return {
    SSHTunnel: vi.fn().mockImplementation(function() {
      return { ...mockTunnelInstance }
    })
  }
})

let grpcDisconnectCallback: (() => void) | null = null

const mockWatchSessionUnsubscribe = vi.fn()

const mockRemoteClient = {
  connect: vi.fn().mockResolvedValue(undefined),
  disconnect: vi.fn(),
  onDisconnect: vi.fn().mockImplementation((cb: () => void) => {
    grpcDisconnectCallback = cb
    return () => { grpcDisconnectCallback = null }
  }),
  watchSession: vi.fn().mockReturnValue({
    initial: Promise.resolve({ id: 'test', workspaces: [], createdAt: 0, lastActivity: 0, version: 1, lock: null }),
    unsubscribe: mockWatchSessionUnsubscribe,
  }),
}

vi.mock('./grpcClient', () => ({
  GrpcDaemonClient: vi.fn().mockImplementation(function() {
    return { ...mockRemoteClient }
  })
}))

const mockPortForwardInstance = {
  start: vi.fn(),
  stop: vi.fn(),
  getOutput: vi.fn().mockReturnValue(['pf-line']),
  onOutput: vi.fn().mockReturnValue(() => {}),
  onStatusChange: vi.fn().mockReturnValue(() => {}),
  toInfo: vi.fn(),
}

vi.mock('./portForward', () => ({
  PortForwardProcess: vi.fn().mockImplementation(function(_ssh: unknown, config: PortForwardConfig) {
    mockPortForwardInstance.toInfo.mockReturnValue({
      id: config.id,
      connectionId: config.connectionId,
      localPort: config.localPort,
      remoteHost: config.remoteHost,
      remotePort: config.remotePort,
      status: 'connecting' as const,
    })
    return { ...mockPortForwardInstance }
  })
}))

import { ConnectionManager } from './connectionManager'
import type { GrpcDaemonClient } from './grpcClient'

function mockClient(overrides: Partial<GrpcDaemonClient> = {}): GrpcDaemonClient {
  return {
    connect: vi.fn().mockResolvedValue(undefined),
    disconnect: vi.fn(),
    onDisconnect: vi.fn(),
    socketPath: '/tmp/test.sock',
    watchSession: vi.fn().mockReturnValue({
      initial: Promise.resolve({ id: 'test', workspaces: [], createdAt: 0, lastActivity: 0, version: 1, lock: null }),
      unsubscribe: vi.fn(),
    }),
    ...overrides,
  } as unknown as GrpcDaemonClient
}

describe('ConnectionManager', () => {
  let localClient: GrpcDaemonClient
  let manager: ConnectionManager

  const remoteConfig = {
    id: 'remote-1',
    host: 'example.com',
    user: 'test',
    port: 22,
    portForwards: [],
  }

  beforeEach(() => {
    vi.useFakeTimers()
    vi.clearAllMocks()
    tunnelBootstrapOutputCallback = null
    tunnelOutputCallback = null
    tunnelDisconnectCallback = null
    grpcDisconnectCallback = null
    localClient = mockClient()
    manager = new ConnectionManager(localClient)
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  describe('constructor', () => {
    it('registers local connection as connected', () => {
      const connections = manager.listConnections()
      expect(connections).toHaveLength(1)
      expect(connections[0]!.id).toBe('local')
      expect(connections[0]!.status).toBe('connected')
      expect(connections[0]!.target).toEqual({ type: 'local' })
    })
  })

  describe('getClient', () => {
    it('returns client for existing connection', () => {
      expect(manager.getClient('local')).toBe(localClient)
    })

    it('throws for missing connection', () => {
      expect(() => manager.getClient('nonexistent')).toThrow('Connection not found: nonexistent')
    })
  })

  describe('getLocalClient', () => {
    it('returns the local client', () => {
      expect(manager.getLocalClient()).toBe(localClient)
    })
  })

  describe('listConnections', () => {
    it('returns all connection infos', () => {
      const connections = manager.listConnections()
      expect(connections).toHaveLength(1)
      expect(connections[0]).toEqual({
        id: 'local',
        target: { type: 'local' },
        status: 'connected',
      })
    })
  })

  describe('getConnection', () => {
    it('returns connection info for existing connection', () => {
      const info = manager.getConnection('local')
      expect(info).toBeDefined()
      expect(info?.id).toBe('local')
      expect(info?.status).toBe('connected')
    })

    it('returns undefined for missing connection', () => {
      expect(manager.getConnection('nonexistent')).toBeUndefined()
    })
  })

  describe('watchBootstrapOutput', () => {
    it('sets up watcher and returns scrollback', () => {
      const cb = vi.fn()
      const result = manager.watchBootstrapOutput('local', cb)
      expect(result.scrollback).toEqual([])
      expect(typeof result.unsubscribe).toBe('function')
    })

    it('unsubscribe removes the watcher', () => {
      const cb = vi.fn()
      const { unsubscribe } = manager.watchBootstrapOutput('local', cb)
      unsubscribe()
      // Should not throw
    })

    it('creates new watcher set for connection without existing watchers', () => {
      const cb1 = vi.fn()
      const cb2 = vi.fn()
      manager.watchBootstrapOutput('local', cb1)
      manager.watchBootstrapOutput('local', cb2)
      // Both should be registered without error
    })
  })

  describe('watchConnectionStatus', () => {
    it('returns initial connection info and unsubscribe', () => {
      const cb = vi.fn()
      const result = manager.watchConnectionStatus('local', cb)
      expect(result.initial).toBeDefined()
      expect(result.initial?.id).toBe('local')
      expect(typeof result.unsubscribe).toBe('function')
    })

    it('returns undefined initial for missing connection', () => {
      const cb = vi.fn()
      const result = manager.watchConnectionStatus('nonexistent', cb)
      expect(result.initial).toBeUndefined()
    })

    it('unsubscribe removes the watcher', () => {
      const cb = vi.fn()
      const { unsubscribe } = manager.watchConnectionStatus('local', cb)
      unsubscribe()
    })
  })

  describe('connectRemote', () => {
    beforeEach(() => {
      mockTunnelInstance.connect.mockResolvedValue('/tmp/test.sock')
      mockTunnelInstance.disconnect.mockClear()
      mockRemoteClient.connect.mockResolvedValue(undefined)
      mockRemoteClient.disconnect.mockClear()
      mockRemoteClient.onDisconnect.mockClear()
    })

    it('connects successfully and lists the connection', async () => {
      const result = await manager.connectRemote(remoteConfig)
      expect(result.status).toBe('connected')
      expect(result.id).toBe('remote-1')
      expect(manager.listConnections()).toHaveLength(2)
    })

    it('returns existing connection if already connected', async () => {
      const result1 = await manager.connectRemote(remoteConfig)
      expect(result1.status).toBe('connected')

      const result2 = await manager.connectRemote(remoteConfig)
      expect(result2.status).toBe('connected')
    })

    it('deduplicates concurrent connection attempts', async () => {
      const p1 = manager.connectRemote(remoteConfig)
      const p2 = manager.connectRemote(remoteConfig)
      const [r1, r2] = await Promise.all([p1, p2])
      expect(r1).toEqual(r2)
    })

    it('handles connection failure', async () => {
      mockTunnelInstance.connect.mockRejectedValue(new Error('SSH failed'))

      const result = await manager.connectRemote(remoteConfig)
      expect(result).toMatchObject({ status: 'error', error: 'SSH failed' })
      expect(mockTunnelInstance.disconnect).toHaveBeenCalled()
    })

    it('handles non-Error thrown values', async () => {
      mockTunnelInstance.connect.mockRejectedValue('string error')

      const result = await manager.connectRemote(remoteConfig)
      expect(result).toMatchObject({ status: 'error', error: 'string error' })
    })

    it('emits status changes during connection', async () => {
      const cb = vi.fn()
      manager.onStatusChange(cb)
      await manager.connectRemote(remoteConfig)

      const statuses = cb.mock.calls.map((c: unknown[]) => (c[0] as { status: string }).status)
      expect(statuses).toContain('connecting')
      expect(statuses).toContain('connected')
    })

    it('notifies per-connection status watchers', async () => {
      await manager.connectRemote(remoteConfig)
      const cb = vi.fn()
      manager.watchConnectionStatus('remote-1', cb)

      // Trigger a status change via tunnel disconnect — now triggers reconnecting
      tunnelDisconnectCallback?.('lost connection')
      expect(cb).toHaveBeenCalled()
      const statuses = cb.mock.calls.map((c: unknown[]) => (c[0] as { status: string }).status)
      expect(statuses).toContain('reconnecting')
    })

    it('forwards bootstrap output to watchers', async () => {
      await manager.connectRemote(remoteConfig)
      const outputCb = vi.fn()
      manager.watchBootstrapOutput('remote-1', outputCb)

      // Simulate bootstrap output via the captured callback
      tunnelBootstrapOutputCallback?.('hello from bootstrap')
      expect(outputCb).toHaveBeenCalledWith('hello from bootstrap')
    })

    it('forwards tunnel output to watchers', async () => {
      await manager.connectRemote(remoteConfig)
      const outputCb = vi.fn()
      manager.watchTunnelOutput('remote-1', outputCb)

      // Simulate tunnel output via the captured callback
      tunnelOutputCallback?.('hello from tunnel')
      expect(outputCb).toHaveBeenCalledWith('hello from tunnel')
    })

    it('monitors tunnel disconnection and triggers reconnect', async () => {
      const statusCb = vi.fn()
      manager.onStatusChange(statusCb)
      await manager.connectRemote(remoteConfig)

      // Simulate tunnel disconnect
      tunnelDisconnectCallback?.('lost connection')
      const info = manager.getConnection('remote-1')
      expect(info?.status).toBe('reconnecting')
    })

    it('monitors gRPC client disconnection and triggers reconnect', async () => {
      await manager.connectRemote(remoteConfig)

      grpcDisconnectCallback?.()
      const info = manager.getConnection('remote-1')
      expect(info?.status).toBe('reconnecting')
    })
  })

  describe('disconnectRemote', () => {
    it('throws when trying to disconnect local', () => {
      expect(() => { manager.disconnectRemote('local'); }).toThrow('Cannot disconnect local connection')
    })

    it('no-ops for missing connection', () => {
      manager.disconnectRemote('nonexistent')
    })

    it('disconnects client and tunnel and removes connection', async () => {
      await manager.connectRemote(remoteConfig)
      expect(manager.listConnections()).toHaveLength(2)

      manager.disconnectRemote('remote-1')
      expect(manager.listConnections()).toHaveLength(1)
      expect(manager.getConnection('remote-1')).toBeUndefined()
    })

    it('emits disconnected status', async () => {
      await manager.connectRemote(remoteConfig)
      const cb = vi.fn()
      manager.onStatusChange(cb)

      manager.disconnectRemote('remote-1')
      expect(cb).toHaveBeenCalledWith(expect.objectContaining({ status: 'disconnected' }))
    })

    it('cleans up port forwards for the connection', async () => {
      await manager.connectRemote(remoteConfig)
      manager.addPortForward({
        id: 'pf-1',
        connectionId: 'remote-1',
        localPort: 8080,
        remoteHost: 'localhost',
        remotePort: 3000,
      })
      expect(manager.listPortForwards('remote-1')).toHaveLength(1)

      manager.disconnectRemote('remote-1')
      expect(manager.listPortForwards('remote-1')).toHaveLength(0)
    })
  })

  describe('onStatusChange', () => {
    it('registers listener and returns unsubscribe', () => {
      const cb = vi.fn()
      const unsubscribe = manager.onStatusChange(cb)
      expect(typeof unsubscribe).toBe('function')
      unsubscribe()
    })
  })

  describe('port forwarding', () => {
    const pfConfig: PortForwardConfig = {
      id: 'pf-1',
      connectionId: 'remote-1',
      localPort: 8080,
      remoteHost: 'localhost',
      remotePort: 3000,
    }

    it('addPortForward creates and starts a port forward', async () => {
      await manager.connectRemote(remoteConfig)
      const info = manager.addPortForward(pfConfig)
      expect(info).toEqual(expect.objectContaining({ id: 'pf-1', status: 'connecting' }))
      expect(mockPortForwardInstance.start).toHaveBeenCalled()
    })

    it('addPortForward throws for local connection', () => {
      expect(() => manager.addPortForward({ ...pfConfig, connectionId: 'local' })).toThrow()
    })

    it('addPortForward throws for unknown connection', () => {
      expect(() => manager.addPortForward({ ...pfConfig, connectionId: 'unknown' })).toThrow()
    })

    it('listPortForwards returns forwards for a connection', async () => {
      await manager.connectRemote(remoteConfig)
      manager.addPortForward(pfConfig)
      const list = manager.listPortForwards('remote-1')
      expect(list).toHaveLength(1)
      expect(list[0]!.id).toBe('pf-1')
    })

    it('listPortForwards returns empty for connection with no forwards', async () => {
      await manager.connectRemote(remoteConfig)
      expect(manager.listPortForwards('remote-1')).toHaveLength(0)
    })

    it('removePortForward stops and removes the forward', async () => {
      await manager.connectRemote(remoteConfig)
      manager.addPortForward(pfConfig)
      manager.removePortForward('pf-1')
      expect(mockPortForwardInstance.stop).toHaveBeenCalled()
      expect(manager.listPortForwards('remote-1')).toHaveLength(0)
    })

    it('removePortForward no-ops for unknown id', () => {
      expect(() => { manager.removePortForward('unknown'); }).not.toThrow()
    })

    it('watchPortForwardOutput returns scrollback and subscribes', async () => {
      await manager.connectRemote(remoteConfig)
      manager.addPortForward(pfConfig)
      const cb = vi.fn()
      const { scrollback, unsubscribe } = manager.watchPortForwardOutput('pf-1', cb)
      expect(scrollback).toEqual(['pf-line'])
      unsubscribe()
    })

    it('watchPortForwardOutput returns empty for unknown forward', () => {
      const cb = vi.fn()
      const { scrollback } = manager.watchPortForwardOutput('unknown', cb)
      expect(scrollback).toEqual([])
    })

    it('watchPortForwardStatus returns initial info', async () => {
      await manager.connectRemote(remoteConfig)
      manager.addPortForward(pfConfig)
      const cb = vi.fn()
      const { initial, unsubscribe } = manager.watchPortForwardStatus('pf-1', cb)
      expect(initial).toEqual(expect.objectContaining({ id: 'pf-1', status: 'connecting' }))
      unsubscribe()
    })

    it('watchPortForwardStatus returns undefined for unknown forward', () => {
      const cb = vi.fn()
      const { initial } = manager.watchPortForwardStatus('unknown', cb)
      expect(initial).toBeUndefined()
    })
  })

  describe('disconnectAll', () => {
    it('disconnects all remote connections but keeps local', async () => {
      await manager.connectRemote(remoteConfig)
      expect(manager.listConnections()).toHaveLength(2)

      manager.disconnectAll()
      expect(manager.listConnections()).toHaveLength(1)
      expect(manager.getClient('local')).toBe(localClient)
    })
  })

  describe('getSSHTunnel', () => {
    it('returns tunnel for remote connection', async () => {
      await manager.connectRemote(remoteConfig)
      expect(manager.getSSHTunnel('remote-1')).toBeDefined()
    })

    it('returns undefined for local connection', () => {
      expect(manager.getSSHTunnel('local')).toBeUndefined()
    })

    it('returns undefined for missing connection', () => {
      expect(manager.getSSHTunnel('nonexistent')).toBeUndefined()
    })
  })

  describe('heartbeat monitoring', () => {
    it('starts heartbeat for local connection on construction', () => {
      // eslint-disable-next-line @typescript-eslint/unbound-method
      expect(localClient.watchSession).toHaveBeenCalledWith(
        expect.stringContaining('heartbeat-local-'),
        expect.any(Function),
        expect.any(Function)
      )
    })

    it('starts heartbeat for remote connection after connect', async () => {
      mockRemoteClient.watchSession.mockClear()
      await manager.connectRemote(remoteConfig)
      expect(mockRemoteClient.watchSession).toHaveBeenCalledWith(
        expect.stringContaining('heartbeat-remote-1-'),
        expect.any(Function),
        expect.any(Function)
      )
    })

    it('heartbeat timeout triggers reconnecting for local connection', () => {
      const cb = vi.fn()
      manager.onStatusChange(cb)

      // Advance past heartbeat timeout (50s)
      vi.advanceTimersByTime(50_000)

      const info = manager.getConnection('local')
      expect(info?.status).toBe('reconnecting')
      expect(info).toHaveProperty('error', 'Connection lost (no heartbeat from daemon)')
    })

    it('heartbeat timeout triggers reconnecting for remote connection', async () => {
      await manager.connectRemote(remoteConfig)
      const cb = vi.fn()
      manager.onStatusChange(cb)

      vi.advanceTimersByTime(50_000)

      const info = manager.getConnection('remote-1')
      expect(info?.status).toBe('reconnecting')
      expect(info).toHaveProperty('error', 'Connection lost (no heartbeat from daemon)')
    })

    it('heartbeat stream error triggers reconnecting', async () => {
      await manager.connectRemote(remoteConfig)

      // Get the onError callback passed to watchSession
      const watchCall = mockRemoteClient.watchSession.mock.calls.find(
        (c: unknown[]) => (c[0] as string).startsWith('heartbeat-remote-1-')
      ) as [string, (session: unknown) => void, (error: Error) => void] | undefined
      expect(watchCall).toBeDefined()
      const onError = watchCall![2]

      onError(new Error('stream broken'))

      const info = manager.getConnection('remote-1')
      expect(info?.status).toBe('reconnecting')
    })

    it('tunnel disconnect triggers reconnecting', async () => {
      mockWatchSessionUnsubscribe.mockClear()
      await manager.connectRemote(remoteConfig)

      tunnelDisconnectCallback?.('lost connection')

      expect(mockWatchSessionUnsubscribe).toHaveBeenCalled()
      const info = manager.getConnection('remote-1')
      expect(info?.status).toBe('reconnecting')
    })

    it('grpc disconnect triggers reconnecting', async () => {
      mockWatchSessionUnsubscribe.mockClear()
      await manager.connectRemote(remoteConfig)

      grpcDisconnectCallback?.()

      expect(mockWatchSessionUnsubscribe).toHaveBeenCalled()
      const info = manager.getConnection('remote-1')
      expect(info?.status).toBe('reconnecting')
    })

    it('manual disconnect stops heartbeat', async () => {
      mockWatchSessionUnsubscribe.mockClear()
      await manager.connectRemote(remoteConfig)

      manager.disconnectRemote('remote-1')

      expect(mockWatchSessionUnsubscribe).toHaveBeenCalled()
    })
  })

  describe('reconnect', () => {
    it('successful reconnect restores connected status', async () => {
      // Trigger heartbeat timeout to enter reconnecting state
      vi.advanceTimersByTime(50_000)

      const info = manager.getConnection('local')
      expect(info?.status).toBe('reconnecting')
      expect(info).toHaveProperty('attempt', 0)

      // Advance past first backoff delay (1s) and flush microtasks
      await vi.advanceTimersByTimeAsync(1_000)

      const info2 = manager.getConnection('local')
      expect(info2?.status).toBe('connected')
    })

    it('cancelReconnect stops retries and sets error', () => {
      vi.advanceTimersByTime(50_000)

      expect(manager.getConnection('local')?.status).toBe('reconnecting')

      manager.cancelReconnect('local')

      const info = manager.getConnection('local')
      expect(info?.status).toBe('error')
    })

    it('reconnectNow skips backoff delay', async () => {
      vi.advanceTimersByTime(50_000)

      expect(manager.getConnection('local')?.status).toBe('reconnecting')

      // reconnectNow should immediately attempt — need to flush the async connect promise
      manager.reconnectNow('local')
      // Flush microtasks for the async reconnect
      // eslint-disable-next-line @typescript-eslint/unbound-method
      await new Promise(process.nextTick)

      expect(manager.getConnection('local')?.status).toBe('connected')
    })

    it('reconnect from error state starts reconnecting', () => {
      vi.advanceTimersByTime(50_000)

      manager.cancelReconnect('local')
      expect(manager.getConnection('local')?.status).toBe('error')

      manager.reconnect('local')
      expect(manager.getConnection('local')?.status).toBe('reconnecting')
    })

    it('failed reconnect attempt schedules retry with backoff', async () => {
      // Make the gRPC client connect fail
      const { GrpcDaemonClient } = await import('./grpcClient')
      const MockGrpc = vi.mocked(GrpcDaemonClient)
      MockGrpc.mockImplementation(function() {
        return {
          ...mockRemoteClient,
          socketPath: '/tmp/test.sock',
          connect: vi.fn().mockRejectedValue(new Error('connection refused')),
        } as unknown as GrpcDaemonClient
      })

      // Trigger heartbeat timeout
      vi.advanceTimersByTime(50_000)

      expect(manager.getConnection('local')?.status).toBe('reconnecting')

      // First attempt after 1s delay — should fail
      await vi.advanceTimersByTimeAsync(1_000)

      const info = manager.getConnection('local')
      expect(info?.status).toBe('reconnecting')
      expect(info).toHaveProperty('attempt', 1)

      // Restore mock for cleanup
      MockGrpc.mockImplementation(function() {
        return { ...mockRemoteClient, socketPath: '/tmp/test.sock' } as unknown as GrpcDaemonClient
      })
    })
  })

  describe('forceReconnect', () => {
    it('from connected state enters reconnecting', () => {
      expect(manager.getConnection('local')?.status).toBe('connected')
      manager.forceReconnect('local')
      expect(manager.getConnection('local')?.status).toBe('reconnecting')
    })

    it('throws for unknown connection', () => {
      expect(() => { manager.forceReconnect('nonexistent') }).toThrow('Connection not found')
    })

    it('is no-op when already reconnecting', () => {
      vi.advanceTimersByTime(50_000)
      expect(manager.getConnection('local')?.status).toBe('reconnecting')
      manager.forceReconnect('local')
      expect(manager.getConnection('local')?.status).toBe('reconnecting')
    })
  })
})
