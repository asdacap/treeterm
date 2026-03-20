/**
 * Connection Manager
 * Registry of all connections (local + remotes). Each connection owns a GrpcDaemonClient.
 */

import { GrpcDaemonClient } from './grpcClient'
import { SSHTunnel } from './ssh'
import type {
  SSHConnectionConfig,
  ConnectionTarget,
  ConnectionStatus,
  ConnectionInfo
} from '../shared/types'

interface Connection {
  target: ConnectionTarget
  client: GrpcDaemonClient
  tunnel?: SSHTunnel
  status: ConnectionStatus
  error?: string
}

type StatusChangeCallback = (info: ConnectionInfo) => void
type OutputCallback = (line: string) => void

export class ConnectionManager {
  private connections: Map<string, Connection> = new Map()
  private statusListeners: Set<StatusChangeCallback> = new Set()
  private outputWatchers: Map<string, Set<OutputCallback>> = new Map()
  private statusWatchers: Map<string, Set<StatusChangeCallback>> = new Map()

  constructor(localClient: GrpcDaemonClient) {
    // Register the local connection
    this.connections.set('local', {
      target: { type: 'local' },
      client: localClient,
      status: 'connected'
    })
  }

  getClient(connectionId: string): GrpcDaemonClient {
    const conn = this.connections.get(connectionId)
    if (!conn) {
      throw new Error(`Connection not found: ${connectionId}`)
    }
    return conn.client
  }

  getLocalClient(): GrpcDaemonClient {
    return this.getClient('local')
  }

  listConnections(): ConnectionInfo[] {
    const result: ConnectionInfo[] = []
    for (const [id, conn] of this.connections) {
      result.push({
        id,
        target: conn.target,
        status: conn.status,
        error: conn.error
      })
    }
    return result
  }

  getConnection(connectionId: string): ConnectionInfo | undefined {
    const conn = this.connections.get(connectionId)
    if (!conn) return undefined
    return {
      id: connectionId,
      target: conn.target,
      status: conn.status,
      error: conn.error
    }
  }

  watchOutput(connectionId: string, cb: OutputCallback): { scrollback: string[], unsubscribe: () => void } {
    const conn = this.connections.get(connectionId)
    const scrollback = conn?.tunnel?.getOutput() || []

    if (!this.outputWatchers.has(connectionId)) {
      this.outputWatchers.set(connectionId, new Set())
    }
    this.outputWatchers.get(connectionId)!.add(cb)

    // Subscribe to tunnel output if available
    let tunnelUnsub: (() => void) | undefined
    if (conn?.tunnel) {
      tunnelUnsub = conn.tunnel.onOutput(cb)
    }

    return {
      scrollback,
      unsubscribe: () => {
        this.outputWatchers.get(connectionId)?.delete(cb)
        tunnelUnsub?.()
      }
    }
  }

  watchConnectionStatus(connectionId: string, cb: StatusChangeCallback): { initial: ConnectionInfo | undefined, unsubscribe: () => void } {
    const initial = this.getConnection(connectionId)

    if (!this.statusWatchers.has(connectionId)) {
      this.statusWatchers.set(connectionId, new Set())
    }
    this.statusWatchers.get(connectionId)!.add(cb)

    return {
      initial,
      unsubscribe: () => {
        this.statusWatchers.get(connectionId)?.delete(cb)
      }
    }
  }

  async connectRemote(config: SSHConnectionConfig): Promise<ConnectionInfo> {
    // Check if already connected
    const existing = this.connections.get(config.id)
    if (existing && existing.status === 'connected') {
      return {
        id: config.id,
        target: existing.target,
        status: existing.status
      }
    }

    const tunnel = new SSHTunnel(config)
    const target: ConnectionTarget = { type: 'remote', config }

    // Forward tunnel output to any active watchers for this connection
    tunnel.onOutput((line: string) => {
      const watchers = this.outputWatchers.get(config.id)
      if (watchers) {
        for (const cb of watchers) {
          cb(line)
        }
      }
    })

    // Set initial connecting state
    this.connections.set(config.id, {
      target,
      client: null as unknown as GrpcDaemonClient, // Placeholder until connected
      tunnel,
      status: 'connecting'
    })
    this.emitStatus(config.id)

    try {
      // Establish SSH tunnel
      const localSocketPath = await tunnel.connect()

      // Create gRPC client pointing at the forwarded socket
      const client = new GrpcDaemonClient(localSocketPath)
      await client.connect()

      // Update connection with real client
      this.connections.set(config.id, {
        target,
        client,
        tunnel,
        status: 'connected'
      })

      // Monitor for disconnection
      tunnel.onDisconnect((error) => {
        const conn = this.connections.get(config.id)
        if (conn) {
          conn.status = 'disconnected'
          conn.error = error
          this.emitStatus(config.id)
        }
      })

      client.onDisconnect(() => {
        const conn = this.connections.get(config.id)
        if (conn && conn.status === 'connected') {
          conn.status = 'error'
          conn.error = 'gRPC connection lost'
          this.emitStatus(config.id)
        }
      })

      this.emitStatus(config.id)
      return this.getConnection(config.id)!
    } catch (error) {
      const errorMsg = error instanceof Error ? error.message : String(error)
      const conn = this.connections.get(config.id)
      if (conn) {
        conn.status = 'error'
        conn.error = errorMsg
      }
      this.emitStatus(config.id)

      // Clean up on failure
      tunnel.disconnect()

      return {
        id: config.id,
        target,
        status: 'error',
        error: errorMsg
      }
    }
  }

  disconnectRemote(connectionId: string): void {
    if (connectionId === 'local') {
      throw new Error('Cannot disconnect local connection')
    }

    const conn = this.connections.get(connectionId)
    if (!conn) return

    // Disconnect client and tunnel
    if (conn.client) {
      conn.client.disconnect()
    }
    if (conn.tunnel) {
      conn.tunnel.disconnect()
    }

    conn.status = 'disconnected'
    this.emitStatus(connectionId)
    this.connections.delete(connectionId)
  }

  getSSHTunnel(connectionId: string): SSHTunnel | undefined {
    return this.connections.get(connectionId)?.tunnel
  }

  onStatusChange(cb: StatusChangeCallback): () => void {
    this.statusListeners.add(cb)
    return () => this.statusListeners.delete(cb)
  }

  disconnectAll(): void {
    for (const [id] of this.connections) {
      if (id !== 'local') {
        this.disconnectRemote(id)
      }
    }
  }

  private emitStatus(connectionId: string): void {
    const info = this.getConnection(connectionId)
    if (!info) return
    for (const cb of this.statusListeners) {
      cb(info)
    }
    // Notify per-connection watchers
    const watchers = this.statusWatchers.get(connectionId)
    if (watchers) {
      for (const cb of watchers) {
        cb(info)
      }
    }
  }
}
