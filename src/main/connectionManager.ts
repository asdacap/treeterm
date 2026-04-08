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
  ConnectPhase,
  PortForwardConfig,
  PortForwardInfo
} from '../shared/types'

type StatusChangeCallback = (info: ConnectionInfo) => void
type OutputCallback = (line: string) => void
type PortForwardStatusCallback = (info: PortForwardInfo) => void

class Connection {
  client: GrpcDaemonClient | null
  status: ConnectionStatus
  connectPhase?: ConnectPhase
  error?: string
  tunnel?: SSHTunnel

  private static HEARTBEAT_TIMEOUT_MS = 50_000
  private static RECONNECT_MAX_DELAY_MS = 30_000
  private heartbeatTimer: ReturnType<typeof setTimeout> | null = null
  private heartbeatUnsub: (() => void) | null = null
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null
  private reconnectAttempt: number = 0

  private bootstrapWatchers: Set<OutputCallback> = new Set()
  private tunnelWatchers: Set<OutputCallback> = new Set()
  private daemonWatchers: Set<OutputCallback> = new Set()
  private daemonBuffer: string[] = []
  private statusWatchers: Set<StatusChangeCallback> = new Set()
  private portForwards: Map<string, PortForwardProcess> = new Map()
  private portForwardOutputWatchers: Map<string, Set<OutputCallback>> = new Map()
  private portForwardStatusWatchers: Map<string, Set<PortForwardStatusCallback>> = new Map()

  constructor(
    readonly id: string,
    readonly target: ConnectionTarget,
    client: GrpcDaemonClient | null,
    status: ConnectionStatus,
    tunnel?: SSHTunnel
  ) {
    this.client = client
    this.status = status
    this.tunnel = tunnel
  }

  toInfo(): ConnectionInfo {
    if (this.status === 'error') {
      return { id: this.id, target: this.target, status: 'error', error: this.error ?? 'Unknown error' }
    }
    if (this.status === 'reconnecting') {
      return { id: this.id, target: this.target, status: 'reconnecting', error: this.error ?? 'Reconnecting...', attempt: this.reconnectAttempt }
    }
    if (this.status === 'disconnected') {
      return { id: this.id, target: this.target, status: 'disconnected', error: this.error }
    }
    if (this.status === 'connecting') {
      return { id: this.id, target: this.target, status: 'connecting', connectPhase: this.connectPhase }
    }
    return { id: this.id, target: this.target, status: this.status }
  }

  emitBootstrapOutput(line: string): void {
    for (const cb of this.bootstrapWatchers) {
      cb(line)
    }
  }

  emitTunnelOutput(line: string): void {
    for (const cb of this.tunnelWatchers) {
      cb(line)
    }
  }

  emitDaemonOutput(line: string): void {
    this.daemonBuffer.push(line)
    if (this.daemonBuffer.length > 500) {
      this.daemonBuffer = this.daemonBuffer.slice(-250)
    }
    for (const cb of this.daemonWatchers) {
      cb(line)
    }
  }

  emitStatus(info: ConnectionInfo): void {
    for (const cb of this.statusWatchers) {
      cb(info)
    }
  }

  watchBootstrapOutput(cb: OutputCallback): { scrollback: string[], unsubscribe: () => void } {
    const scrollback = this.tunnel?.getBootstrapOutput() || []
    this.bootstrapWatchers.add(cb)
    return {
      scrollback,
      unsubscribe: () => { this.bootstrapWatchers.delete(cb) }
    }
  }

  watchTunnelOutput(cb: OutputCallback): { scrollback: string[], unsubscribe: () => void } {
    const scrollback = this.tunnel?.getTunnelOutput() || []
    this.tunnelWatchers.add(cb)
    return {
      scrollback,
      unsubscribe: () => { this.tunnelWatchers.delete(cb) }
    }
  }

  watchDaemonOutput(cb: OutputCallback): { scrollback: string[], unsubscribe: () => void } {
    const scrollback = [...this.daemonBuffer]
    this.daemonWatchers.add(cb)
    return {
      scrollback,
      unsubscribe: () => { this.daemonWatchers.delete(cb) }
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
    const outputWatchers = this.portForwardOutputWatchers.get(portForwardId)
    if (outputWatchers) {
      outputWatchers.add(cb)
    }

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
    const statusWatchers = this.portForwardStatusWatchers.get(portForwardId)
    if (statusWatchers) {
      statusWatchers.add(cb)
    }

    return {
      initial,
      unsubscribe: () => { this.portForwardStatusWatchers.get(portForwardId)?.delete(cb) }
    }
  }

  startHeartbeatMonitor(onTimeout: () => void): void {
    if (!this.client) return

    const resetTimer = (): void => {
      if (this.heartbeatTimer) clearTimeout(this.heartbeatTimer)
      this.heartbeatTimer = setTimeout(() => {
        if (this.status === 'connected') {
          onTimeout()
        }
      }, Connection.HEARTBEAT_TIMEOUT_MS)
    }

    const listenerId = `heartbeat-${this.id}-${String(Date.now())}`
    const { initial, unsubscribe } = this.client.watchSession(
      listenerId,
      () => { resetTimer() },
      (error) => {
        console.error(`[connection] heartbeat stream error for ${this.id}:`, error)
        if (this.status === 'connected') {
          onTimeout()
        }
      }
    )
    this.heartbeatUnsub = unsubscribe

    // Initial data also counts as a heartbeat
    initial.then(() => { resetTimer() }).catch(() => {
      // Error handled by onError callback
    })

    // Start the first timer
    resetTimer()
  }

  stopHeartbeat(): void {
    if (this.heartbeatTimer) {
      clearTimeout(this.heartbeatTimer)
      this.heartbeatTimer = null
    }
    if (this.heartbeatUnsub) {
      this.heartbeatUnsub()
      this.heartbeatUnsub = null
    }
  }

  startReconnect(onStatusChanged: () => void): void {
    this.stopHeartbeat()
    this.reconnectAttempt = 0
    this.status = 'reconnecting'
    onStatusChanged()
    this.scheduleReconnectAttempt(onStatusChanged)
  }

  reconnectNow(onStatusChanged: () => void): void {
    if (this.reconnectTimer) {
      clearTimeout(this.reconnectTimer)
      this.reconnectTimer = null
    }
    void this.doReconnectAttempt(onStatusChanged)
  }

  cancelReconnect(): void {
    if (this.reconnectTimer) {
      clearTimeout(this.reconnectTimer)
      this.reconnectTimer = null
    }
    this.status = 'error'
    this.error = this.error ?? 'Reconnection cancelled'
  }

  private scheduleReconnectAttempt(onStatusChanged: () => void): void {
    const delay = Math.min(1000 * Math.pow(2, this.reconnectAttempt), Connection.RECONNECT_MAX_DELAY_MS)
    console.log(`[connection] scheduling reconnect attempt ${String(this.reconnectAttempt + 1)} in ${String(delay)}ms for ${this.id}`)
    this.reconnectTimer = setTimeout(() => {
      void this.doReconnectAttempt(onStatusChanged)
    }, delay)
  }

  private async doReconnectAttempt(onStatusChanged: () => void): Promise<void> {
    if (this.status !== 'reconnecting') return

    this.reconnectAttempt++
    onStatusChanged()

    try {
      if (this.target.type === 'local') {
        await this.reconnectLocal()
      } else {
        await this.reconnectRemote()
      }

      // Success
      this.status = 'connected'
      this.error = undefined
      this.reconnectAttempt = 0
      onStatusChanged()

      // Restart heartbeat — on next failure, reconnect again
      this.startHeartbeatMonitor(() => {
        if (this.status === 'connected') {
          this.startReconnect(onStatusChanged)
        }
      })
    } catch (err) {
      if (this.status !== 'reconnecting') return // cancelled during attempt
      const msg = err instanceof Error ? err.message : String(err)
      console.error(`[connection] reconnect attempt ${String(this.reconnectAttempt)} failed for ${this.id}: ${msg}`)
      this.error = msg
      onStatusChanged()
      this.scheduleReconnectAttempt(onStatusChanged)
    }
  }

  private async reconnectLocal(): Promise<void> {
    const socketPath = this.client?.socketPath
    this.client?.disconnect()
    this.client = null

    if (!socketPath) {
      throw new Error('No socket path available for local reconnect')
    }

    const newClient = new GrpcDaemonClient(socketPath)
    await newClient.connect()
    this.client = newClient
  }

  private async reconnectRemote(): Promise<void> {
    if (this.target.type !== 'remote') {
      throw new Error('reconnectRemote called on non-remote connection')
    }

    // Tear down old client and tunnel
    this.client?.disconnect()
    this.client = null
    if (this.tunnel) {
      this.tunnel.disconnect()
      this.tunnel = undefined
    }

    // Create new tunnel with original config
    const tunnel = new SSHTunnel(this.target.config)
    this.tunnel = tunnel

    // Forward tunnel output to connection's watchers
    tunnel.onBootstrapOutput((line: string) => {
      this.emitBootstrapOutput(line)
    })
    tunnel.onTunnelOutput((line: string) => {
      this.emitTunnelOutput(line)
    })

    const localSocketPath = await tunnel.connect()

    // Connect gRPC client through the new forwarded socket
    const client = new GrpcDaemonClient(localSocketPath)
    await client.connect()
    this.client = client
  }

  disconnect(): void {
    // Stop heartbeat monitor and any reconnect attempts
    this.stopHeartbeat()
    if (this.reconnectTimer) {
      clearTimeout(this.reconnectTimer)
      this.reconnectTimer = null
    }

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
    const localConn = new Connection('local', { type: 'local' }, localClient, 'connected')
    this.connections.set('local', localConn)
    localConn.startHeartbeatMonitor(() => {
      const c = this.connections.get('local')
      if (c && c.status === 'connected') {
        c.error = 'Connection lost (no heartbeat from daemon)'
        c.startReconnect(() => { this.emitStatus('local') })
      }
    })
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

  watchBootstrapOutput(connectionId: string, cb: OutputCallback): { scrollback: string[], unsubscribe: () => void } {
    const conn = this.connections.get(connectionId)
    if (!conn) return { scrollback: [], unsubscribe: () => {} }
    return conn.watchBootstrapOutput(cb)
  }

  watchTunnelOutput(connectionId: string, cb: OutputCallback): { scrollback: string[], unsubscribe: () => void } {
    const conn = this.connections.get(connectionId)
    if (!conn) return { scrollback: [], unsubscribe: () => {} }
    return conn.watchTunnelOutput(cb)
  }

  watchDaemonOutput(connectionId: string, cb: OutputCallback): { scrollback: string[], unsubscribe: () => void } {
    const conn = this.connections.get(connectionId)
    if (!conn) return { scrollback: [], unsubscribe: () => {} }
    return conn.watchDaemonOutput(cb)
  }

  watchConnectionStatus(connectionId: string, cb: StatusChangeCallback): { initial: ConnectionInfo | undefined, unsubscribe: () => void } {
    const conn = this.connections.get(connectionId)
    if (!conn) return { initial: undefined, unsubscribe: () => {} }
    return conn.watchStatus(cb)
  }

  async connectRemote(config: SSHConnectionConfig, options?: { refreshDaemon?: boolean; allowOutdatedDaemon?: boolean }): Promise<ConnectionInfo> {
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

  private async doConnectRemote(config: SSHConnectionConfig, options?: { refreshDaemon?: boolean; allowOutdatedDaemon?: boolean }): Promise<ConnectionInfo> {
    const tunnel = new SSHTunnel(config, { refreshDaemon: options?.refreshDaemon, allowOutdatedDaemon: options?.allowOutdatedDaemon })
    const target: ConnectionTarget = { type: 'remote', config }

    const conn = new Connection(config.id, target, null, 'connecting', tunnel)
    conn.connectPhase = 'bootstrap'
    this.connections.set(config.id, conn)

    // Forward tunnel output to connection's watchers
    tunnel.onBootstrapOutput((line: string) => {
      conn.emitBootstrapOutput(line)
    })
    tunnel.onTunnelOutput((line: string) => {
      conn.emitTunnelOutput(line)
    })

    this.emitStatus(config.id)

    try {
      // Establish SSH tunnel (bootstrap + tunnel phases happen inside)
      console.log(`[connectionManager] SSH tunnel connecting to ${config.host}:${String(config.port)} (id=${config.id})`)
      conn.connectPhase = 'tunnel'
      this.emitStatus(config.id)
      const localSocketPath = await tunnel.connect()
      console.log(`[connectionManager] SSH tunnel connected, local socket: ${localSocketPath}`)

      // Connect gRPC client through the forwarded socket
      conn.connectPhase = 'daemon'
      this.emitStatus(config.id)
      conn.emitDaemonOutput('Connecting to daemon via gRPC...')
      const client = new GrpcDaemonClient(localSocketPath)
      console.log(`[connectionManager] Connecting gRPC daemon client via forwarded socket...`)
      await client.connect()
      console.log(`[connectionManager] gRPC daemon client connected successfully`)
      conn.emitDaemonOutput('Connected to daemon')

      // Update connection with real client
      conn.client = client
      conn.status = 'connected'

      // Start heartbeat monitor for end-to-end health checking
      conn.startHeartbeatMonitor(() => {
        const c = this.connections.get(config.id)
        if (c && c.status === 'connected') {
          c.error = 'Connection lost (no heartbeat from daemon)'
          c.startReconnect(() => { this.emitStatus(config.id) })
        }
      })

      // Monitor for disconnection — trigger reconnect for auto-recovery
      tunnel.onDisconnect((error) => {
        const c = this.connections.get(config.id)
        if (c && (c.status === 'connected' || c.status === 'reconnecting')) {
          c.client?.disconnect()
          c.client = null
          c.error = error ?? 'SSH tunnel disconnected'
          c.startReconnect(() => { this.emitStatus(config.id) })
        }
      })

      client.onDisconnect(() => {
        const c = this.connections.get(config.id)
        if (c && c.status === 'connected') {
          c.error = 'gRPC connection lost'
          c.startReconnect(() => { this.emitStatus(config.id) })
        }
      })

      this.emitStatus(config.id)
      const connInfo = this.getConnection(config.id)
      if (!connInfo) {
        throw new Error(`Connection not found after connect: ${config.id}`)
      }
      return connInfo
    } catch (error) {
      const rawMsg = error instanceof Error ? error.message : String(error)
      const phase = conn.connectPhase ?? 'bootstrap'
      const errorMsg = phase === 'daemon'
        ? `SSH tunnel OK, but daemon not responding: ${rawMsg}`
        : rawMsg
      console.error(`[connectionManager] Connection failed at ${phase} phase (id=${config.id}): ${errorMsg}`)
      if (phase === 'daemon') {
        conn.emitDaemonOutput(`Failed: ${rawMsg}`)
      }
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

  reconnect(connectionId: string): void {
    const conn = this.connections.get(connectionId)
    if (!conn) return
    if (conn.status === 'error' || conn.status === 'disconnected') {
      conn.error = conn.error ?? 'Manual reconnect'
      conn.startReconnect(() => { this.emitStatus(connectionId) })
    }
  }

  reconnectNow(connectionId: string): void {
    const conn = this.connections.get(connectionId)
    if (conn?.status === 'reconnecting') {
      conn.reconnectNow(() => { this.emitStatus(connectionId) })
    }
  }

  cancelReconnect(connectionId: string): void {
    const conn = this.connections.get(connectionId)
    if (conn?.status === 'reconnecting') {
      conn.cancelReconnect()
      this.emitStatus(connectionId)
    }
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
