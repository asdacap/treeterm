/**
 * Connection Manager
 * Registry of all connections (local + remotes). Each connection owns a GrpcDaemonClient.
 */

import { GrpcDaemonClient } from './grpcClient'
import { SSHTunnel } from './ssh'
import { PortForwardProcess } from './portForward'
import type {
  SSHConnectionConfig,
  ConnectionTarget,
  ConnectionStatus,
  ConnectionInfo,
  PortForwardConfig,
  PortForwardInfo
} from '../shared/types'

type StatusChangeCallback = (info: ConnectionInfo) => void
type OutputCallback = (line: string) => void
type PortForwardStatusCallback = (info: PortForwardInfo) => void

class Connection {
  client: GrpcDaemonClient | null
  status: ConnectionStatus
  error?: string

  private outputWatchers: Set<OutputCallback> = new Set()
  private statusWatchers: Set<StatusChangeCallback> = new Set()
  private portForwards: Map<string, PortForwardProcess> = new Map()
  private portForwardOutputWatchers: Map<string, Set<OutputCallback>> = new Map()
  private portForwardStatusWatchers: Map<string, Set<PortForwardStatusCallback>> = new Map()

  constructor(
    readonly id: string,
    readonly target: ConnectionTarget,
    client: GrpcDaemonClient | null,
    status: ConnectionStatus,
    readonly tunnel?: SSHTunnel
  ) {
    this.client = client
    this.status = status
  }

  toInfo(): ConnectionInfo {
    if (this.status === 'error') {
      return { id: this.id, target: this.target, status: 'error', error: this.error ?? 'Unknown error' }
    }
    if (this.status === 'disconnected') {
      return { id: this.id, target: this.target, status: 'disconnected', error: this.error }
    }
    return { id: this.id, target: this.target, status: this.status }
  }

  emitOutput(line: string): void {
    for (const cb of this.outputWatchers) {
      cb(line)
    }
  }

  emitStatus(info: ConnectionInfo): void {
    for (const cb of this.statusWatchers) {
      cb(info)
    }
  }

  watchOutput(cb: OutputCallback): { scrollback: string[], unsubscribe: () => void } {
    const scrollback = this.tunnel?.getOutput() || []
    this.outputWatchers.add(cb)
    return {
      scrollback,
      unsubscribe: () => { this.outputWatchers.delete(cb) }
    }
  }

  watchStatus(cb: StatusChangeCallback): { initial: ConnectionInfo, unsubscribe: () => void } {
    this.statusWatchers.add(cb)
    return {
      initial: this.toInfo(),
      unsubscribe: () => { this.statusWatchers.delete(cb) }
    }
  }

  addPortForward(config: PortForwardConfig): PortForwardInfo {
    if (this.target.type !== 'remote') {
      throw new Error(`Connection is not remote: ${this.id}`)
    }

    const pf = new PortForwardProcess(this.target.config, config)

    pf.onOutput((line) => {
      const watchers = this.portForwardOutputWatchers.get(config.id)
      if (watchers) {
        for (const cb of watchers) cb(line)
      }
    })

    pf.onStatusChange((info) => {
      const watchers = this.portForwardStatusWatchers.get(config.id)
      if (watchers) {
        for (const cb of watchers) cb(info)
      }
    })

    this.portForwards.set(config.id, pf)
    pf.start()
    return pf.toInfo()
  }

  removePortForward(portForwardId: string): boolean {
    const pf = this.portForwards.get(portForwardId)
    if (!pf) return false
    pf.stop()
    this.portForwards.delete(portForwardId)
    this.portForwardOutputWatchers.delete(portForwardId)
    this.portForwardStatusWatchers.delete(portForwardId)
    return true
  }

  hasPortForward(portForwardId: string): boolean {
    return this.portForwards.has(portForwardId)
  }

  listPortForwards(): PortForwardInfo[] {
    return Array.from(this.portForwards.values()).map(pf => pf.toInfo())
  }

  watchPortForwardOutput(portForwardId: string, cb: OutputCallback): { scrollback: string[], unsubscribe: () => void } {
    const pf = this.portForwards.get(portForwardId)
    const scrollback = pf?.getOutput() ?? []

    if (!this.portForwardOutputWatchers.has(portForwardId)) {
      this.portForwardOutputWatchers.set(portForwardId, new Set())
    }
    this.portForwardOutputWatchers.get(portForwardId)!.add(cb)

    return {
      scrollback,
      unsubscribe: () => { this.portForwardOutputWatchers.get(portForwardId)?.delete(cb) }
    }
  }

  watchPortForwardStatus(portForwardId: string, cb: PortForwardStatusCallback): { initial: PortForwardInfo | undefined, unsubscribe: () => void } {
    const initial = this.portForwards.get(portForwardId)?.toInfo()

    if (!this.portForwardStatusWatchers.has(portForwardId)) {
      this.portForwardStatusWatchers.set(portForwardId, new Set())
    }
    this.portForwardStatusWatchers.get(portForwardId)!.add(cb)

    return {
      initial,
      unsubscribe: () => { this.portForwardStatusWatchers.get(portForwardId)?.delete(cb) }
    }
  }

  disconnect(): void {
    // Stop all port forwards
    for (const pf of this.portForwards.values()) {
      pf.stop()
    }
    this.portForwards.clear()
    this.portForwardOutputWatchers.clear()
    this.portForwardStatusWatchers.clear()

    // Disconnect client and tunnel
    this.client?.disconnect()
    this.client = null
    if (this.tunnel) {
      this.tunnel.disconnect()
    }

    this.status = 'disconnected'
  }
}

export class ConnectionManager {
  private connections: Map<string, Connection> = new Map()
  private connectingPromises: Map<string, Promise<ConnectionInfo>> = new Map()
  private statusListeners: Set<StatusChangeCallback> = new Set()

  constructor(localClient: GrpcDaemonClient) {
    this.connections.set('local', new Connection('local', { type: 'local' }, localClient, 'connected'))
  }

  getClient(connectionId: string): GrpcDaemonClient {
    const conn = this.connections.get(connectionId)
    if (!conn) {
      throw new Error(`Connection not found: ${connectionId}`)
    }
    if (!conn.client || conn.status !== 'connected') {
      throw new Error(`Connection ${connectionId} is ${conn.status}${conn.error ? ': ' + conn.error : ''}`)
    }
    return conn.client
  }

  getLocalClient(): GrpcDaemonClient {
    return this.getClient('local')
  }

  listConnections(): ConnectionInfo[] {
    return Array.from(this.connections.values()).map(conn => conn.toInfo())
  }

  getConnection(connectionId: string): ConnectionInfo | undefined {
    return this.connections.get(connectionId)?.toInfo()
  }

  watchOutput(connectionId: string, cb: OutputCallback): { scrollback: string[], unsubscribe: () => void } {
    const conn = this.connections.get(connectionId)
    if (!conn) return { scrollback: [], unsubscribe: () => {} }
    return conn.watchOutput(cb)
  }

  watchConnectionStatus(connectionId: string, cb: StatusChangeCallback): { initial: ConnectionInfo | undefined, unsubscribe: () => void } {
    const conn = this.connections.get(connectionId)
    if (!conn) return { initial: undefined, unsubscribe: () => {} }
    return conn.watchStatus(cb)
  }

  async connectRemote(config: SSHConnectionConfig, options?: { refreshDaemon?: boolean }): Promise<ConnectionInfo> {
    // Check if already connected
    const existing = this.connections.get(config.id)
    if (existing && existing.status === 'connected') {
      return existing.toInfo()
    }

    // Deduplicate concurrent connection attempts for the same host
    const pending = this.connectingPromises.get(config.id)
    if (pending) {
      return pending
    }

    const promise = this.doConnectRemote(config, options).finally(() => {
      this.connectingPromises.delete(config.id)
    })
    this.connectingPromises.set(config.id, promise)
    return promise
  }

  private async doConnectRemote(config: SSHConnectionConfig, options?: { refreshDaemon?: boolean }): Promise<ConnectionInfo> {
    const tunnel = new SSHTunnel(config, { refreshDaemon: options?.refreshDaemon })
    const target: ConnectionTarget = { type: 'remote', config }

    const conn = new Connection(config.id, target, null, 'connecting', tunnel)
    this.connections.set(config.id, conn)

    // Forward tunnel output to connection's watchers
    tunnel.onOutput((line: string) => {
      conn.emitOutput(line)
    })

    this.emitStatus(config.id)

    try {
      // Establish SSH tunnel
      console.log(`[connectionManager] SSH tunnel connecting to ${config.host}:${config.port} (id=${config.id})`)
      const localSocketPath = await tunnel.connect()
      console.log(`[connectionManager] SSH tunnel connected, local socket: ${localSocketPath}`)

      // Create gRPC client pointing at the forwarded socket
      const client = new GrpcDaemonClient(localSocketPath)
      console.log(`[connectionManager] Connecting gRPC daemon client via forwarded socket...`)
      await client.connect()
      console.log(`[connectionManager] gRPC daemon client connected successfully`)

      // Update connection with real client
      conn.client = client
      conn.status = 'connected'

      // Monitor for disconnection
      tunnel.onDisconnect((error) => {
        const c = this.connections.get(config.id)
        if (c) {
          c.client?.disconnect()
          c.client = null
          c.status = 'disconnected'
          c.error = error
          this.emitStatus(config.id)
        }
      })

      client.onDisconnect(() => {
        const c = this.connections.get(config.id)
        if (c && c.status === 'connected') {
          c.status = 'error'
          c.error = 'gRPC connection lost'
          this.emitStatus(config.id)
        }
      })

      this.emitStatus(config.id)
      return this.getConnection(config.id)!
    } catch (error) {
      const errorMsg = error instanceof Error ? error.message : String(error)
      console.error(`[connectionManager] SSH connection failed (id=${config.id}): ${errorMsg}`)
      conn.status = 'error'
      conn.error = errorMsg
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

    conn.disconnect()
    this.emitStatus(connectionId)
    this.connections.delete(connectionId)
  }

  addPortForward(config: PortForwardConfig): PortForwardInfo {
    const conn = this.connections.get(config.connectionId)
    if (!conn || conn.target.type !== 'remote') {
      throw new Error(`Connection not found or not remote: ${config.connectionId}`)
    }
    return conn.addPortForward(config)
  }

  removePortForward(portForwardId: string): void {
    for (const conn of this.connections.values()) {
      if (conn.removePortForward(portForwardId)) return
    }
  }

  listPortForwards(connectionId: string): PortForwardInfo[] {
    const conn = this.connections.get(connectionId)
    if (!conn) return []
    return conn.listPortForwards()
  }

  watchPortForwardOutput(portForwardId: string, cb: OutputCallback): { scrollback: string[], unsubscribe: () => void } {
    for (const conn of this.connections.values()) {
      if (conn.hasPortForward(portForwardId)) {
        return conn.watchPortForwardOutput(portForwardId, cb)
      }
    }
    return { scrollback: [], unsubscribe: () => {} }
  }

  watchPortForwardStatus(portForwardId: string, cb: PortForwardStatusCallback): { initial: PortForwardInfo | undefined, unsubscribe: () => void } {
    for (const conn of this.connections.values()) {
      if (conn.hasPortForward(portForwardId)) {
        return conn.watchPortForwardStatus(portForwardId, cb)
      }
    }
    return { initial: undefined, unsubscribe: () => {} }
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
    const conn = this.connections.get(connectionId)
    if (!conn) return
    const info = conn.toInfo()
    for (const cb of this.statusListeners) {
      cb(info)
    }
    conn.emitStatus(info)
  }
}
