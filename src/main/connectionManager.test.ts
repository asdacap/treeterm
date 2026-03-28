import { describe, it, expect, vi, beforeEach } from 'vitest'

// Mock ssh module before importing connectionManager
const mockTunnelInstance = {
  connect: vi.fn().mockResolvedValue('/tmp/test.sock'),
  disconnect: vi.fn(),
  onOutput: vi.fn().mockReturnValue(() => {}),
  onDisconnect: vi.fn(),
  getOutput: vi.fn().mockReturnValue([]),
}

vi.mock('./ssh', () => {
  return {
    SSHTunnel: vi.fn().mockImplementation(function() {
      return mockTunnelInstance
    })
  }
})

const mockRemoteClient = {
  connect: vi.fn().mockResolvedValue(undefined),
  disconnect: vi.fn(),
  onDisconnect: vi.fn(),
}

vi.mock('./grpcClient', () => ({
  GrpcDaemonClient: vi.fn().mockImplementation(function() {
    return mockRemoteClient
  })
}))

import { ConnectionManager } from './connectionManager'
import { SSHTunnel } from './ssh'
import type { GrpcDaemonClient } from './grpcClient'

function mockClient(overrides: Partial<GrpcDaemonClient> = {}): GrpcDaemonClient {
  return {
    connect: vi.fn().mockResolvedValue(undefined),
    disconnect: vi.fn(),
    onDisconnect: vi.fn(),
    ...overrides,
  } as unknown as GrpcDaemonClient
}

describe('ConnectionManager', () => {
  let localClient: GrpcDaemonClient
  let manager: ConnectionManager

  beforeEach(() => {
    vi.clearAllMocks()
    localClient = mockClient()
    manager = new ConnectionManager(localClient)
  })

  describe('constructor', () => {
    it('registers local connection as connected', () => {
      const connections = manager.listConnections()
      expect(connections).toHaveLength(1)
      expect(connections[0].id).toBe('local')
      expect(connections[0].status).toBe('connected')
      expect(connections[0].target).toEqual({ type: 'local' })
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
        error: undefined,
      })
    })
  })

  describe('getConnection', () => {
    it('returns connection info for existing connection', () => {
      const info = manager.getConnection('local')
      expect(info).toBeDefined()
      expect(info!.id).toBe('local')
      expect(info!.status).toBe('connected')
    })

    it('returns undefined for missing connection', () => {
      expect(manager.getConnection('nonexistent')).toBeUndefined()
    })
  })

  describe('watchOutput', () => {
    it('sets up watcher and returns scrollback', () => {
      const cb = vi.fn()
      const result = manager.watchOutput('local', cb)
      expect(result.scrollback).toEqual([])
      expect(typeof result.unsubscribe).toBe('function')
    })

    it('unsubscribe removes the watcher', () => {
      const cb = vi.fn()
      const { unsubscribe } = manager.watchOutput('local', cb)
      unsubscribe()
      // Should not throw
    })

    it('creates new watcher set for connection without existing watchers', () => {
      const cb1 = vi.fn()
      const cb2 = vi.fn()
      manager.watchOutput('local', cb1)
      manager.watchOutput('local', cb2)
      // Both should be registered without error
    })
  })

  describe('watchConnectionStatus', () => {
    it('returns initial connection info and unsubscribe', () => {
      const cb = vi.fn()
      const result = manager.watchConnectionStatus('local', cb)
      expect(result.initial).toBeDefined()
      expect(result.initial!.id).toBe('local')
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
    const config = {
      id: 'remote-1',
      host: 'example.com',
      user: 'test',
      port: 22,
    }

    beforeEach(() => {
      mockTunnelInstance.connect.mockResolvedValue('/tmp/test.sock')
      mockTunnelInstance.disconnect.mockClear()
      mockRemoteClient.connect.mockResolvedValue(undefined)
      mockRemoteClient.disconnect.mockClear()
      mockRemoteClient.onDisconnect.mockClear()
    })

    it('returns existing connection if already connected', async () => {
      // First connect
      const result1 = await manager.connectRemote(config)
      expect(result1.status).toBe('connected')

      // Second connect should return the existing one
      const result2 = await manager.connectRemote(config)
      expect(result2.status).toBe('connected')
    })

    it('handles connection failure', async () => {
      mockTunnelInstance.connect.mockRejectedValue(new Error('SSH failed'))

      const result = await manager.connectRemote(config)
      expect(result.status).toBe('error')
      expect(result.error).toBe('SSH failed')
      expect(mockTunnelInstance.disconnect).toHaveBeenCalled()
    })
  })

  describe('disconnectRemote', () => {
    it('throws when trying to disconnect local', () => {
      expect(() => manager.disconnectRemote('local')).toThrow('Cannot disconnect local connection')
    })

    it('no-ops for missing connection', () => {
      // Should not throw
      manager.disconnectRemote('nonexistent')
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

  describe('disconnectAll', () => {
    it('does not disconnect local connection', () => {
      manager.disconnectAll()
      // Local should still be accessible
      expect(manager.getClient('local')).toBe(localClient)
    })
  })

  describe('getSSHTunnel', () => {
    it('returns undefined for local connection', () => {
      expect(manager.getSSHTunnel('local')).toBeUndefined()
    })

    it('returns undefined for missing connection', () => {
      expect(manager.getSSHTunnel('nonexistent')).toBeUndefined()
    })
  })
})
