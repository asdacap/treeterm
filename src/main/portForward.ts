/**
 * SSH Port Forward Process Manager
 * Manages a single `ssh -N -L` process for local port forwarding.
 */

import { spawn, ChildProcess } from 'child_process'
import { PortForwardStatus } from '../shared/types'
import type { SSHConnectionConfig, PortForwardConfig, PortForwardInfo } from '../shared/types'

type OutputCallback = (line: string) => void
type StatusCallback = (info: PortForwardInfo) => void

export class PortForwardProcess {
  private process: ChildProcess | null = null
  private outputBuffer: string[] = []
  private outputListeners: Set<OutputCallback> = new Set()
  private statusListeners: Set<StatusCallback> = new Set()
  private _status: PortForwardStatus = PortForwardStatus.Connecting
  private _error?: string
  private stabilizationTimer: ReturnType<typeof setTimeout> | null = null

  constructor(
    private sshConfig: SSHConnectionConfig,
    private config: PortForwardConfig
  ) {}

  get status(): PortForwardStatus {
    return this._status
  }

  start(): void {
    const args = this.buildSSHArgs()

    this.appendOutput(`[portfwd] Starting port forward: localhost:${String(this.config.localPort)} -> ${this.config.remoteHost}:${String(this.config.remotePort)}`)
    this.appendOutput(`[portfwd] $ ssh ${args.join(' ')}`)

    const proc = spawn('ssh', args, { stdio: ['pipe', 'pipe', 'pipe'] })
    this.process = proc

    // After 2 seconds without exit, consider the forward active
    this.stabilizationTimer = setTimeout(() => {
      if (this._status === PortForwardStatus.Connecting && this.process !== null) {
        this.setStatus(PortForwardStatus.Active)
      }
    }, 2000)

    proc.stdout.on('data', (data: Buffer) => {
      for (const line of data.toString().split('\n').filter(Boolean)) {
        this.appendOutput(`[portfwd] ${line}`)
      }
    })

    proc.stderr.on('data', (data: Buffer) => {
      for (const line of data.toString().split('\n').filter(Boolean)) {
        this.appendOutput(`[portfwd:err] ${line}`)
      }
    })

    // `close`/`error` fire asynchronously after `stop()`/`restart()` have already
    // dropped or replaced `this.process`. Only the current process may change state;
    // a late event from a killed process must not clobber its replacement.
    proc.on('close', (code) => {
      if (this.process !== proc) return
      this.clearStabilizationTimer()
      this.process = null
      const msg = `Port forward process exited (code ${String(code)})`
      this.appendOutput(`[portfwd] ${msg}`)
      this.setStatus(PortForwardStatus.Error, msg)
    })

    proc.on('error', (err) => {
      if (this.process !== proc) return
      this.clearStabilizationTimer()
      this.process = null
      const msg = `Port forward error: ${err.message}`
      this.appendOutput(`[portfwd] ${msg}`)
      this.setStatus(PortForwardStatus.Error, msg)
    })
  }

  /**
   * Kill the current ssh process (if any) and spawn a fresh one. Used after the
   * parent connection reconnects: the forward's own ssh session died with the
   * network, or is about to, so it is rebuilt along with the tunnel.
   */
  restart(): void {
    this.killProcess()
    this.appendOutput('[portfwd] Restarting port forward')
    this.setStatus(PortForwardStatus.Connecting)
    this.start()
  }

  stop(): void {
    this.killProcess()
    this.setStatus(PortForwardStatus.Stopped)
  }

  getOutput(): string[] {
    return [...this.outputBuffer]
  }

  onOutput(cb: OutputCallback): () => void {
    this.outputListeners.add(cb)
    return () => { this.outputListeners.delete(cb) }
  }

  onStatusChange(cb: StatusCallback): () => void {
    this.statusListeners.add(cb)
    return () => { this.statusListeners.delete(cb) }
  }

  toInfo(): PortForwardInfo {
    const base = {
      id: this.config.id,
      connectionId: this.config.connectionId,
      localPort: this.config.localPort,
      remoteHost: this.config.remoteHost,
      remotePort: this.config.remotePort,
    }
    if (this._status === PortForwardStatus.Error) {
      return { ...base, status: PortForwardStatus.Error, error: this._error ?? 'Unknown error' }
    }
    return { ...base, status: this._status }
  }

  private appendOutput(line: string): void {
    console.log(line)
    this.outputBuffer.push(line)
    if (this.outputBuffer.length > 1000) {
      this.outputBuffer = this.outputBuffer.slice(-500)
    }
    for (const cb of this.outputListeners) {
      cb(line)
    }
  }

  private setStatus(status: PortForwardStatus, error?: string): void {
    this._status = status
    this._error = error
    const info = this.toInfo()
    for (const cb of this.statusListeners) {
      cb(info)
    }
  }

  private killProcess(): void {
    this.clearStabilizationTimer()
    if (this.process) {
      this.process.kill()
      this.process = null
    }
  }

  private clearStabilizationTimer(): void {
    if (this.stabilizationTimer !== null) {
      clearTimeout(this.stabilizationTimer)
      this.stabilizationTimer = null
    }
  }

  private buildSSHArgs(): string[] {
    const { host, user, port, identityFile } = this.sshConfig
    const { localPort, remoteHost, remotePort } = this.config

    const args = [
      '-L', `${String(localPort)}:${remoteHost}:${String(remotePort)}`,
      '-o', 'StrictHostKeyChecking=accept-new',
      '-o', 'BatchMode=yes',
      '-o', 'ExitOnForwardFailure=yes',
      '-o', 'ServerAliveInterval=10',
      '-o', 'ServerAliveCountMax=3',
      '-p', String(port),
    ]

    if (identityFile) {
      args.push('-i', identityFile)
    }

    args.push(`${user}@${host}`)
    args.push('cat')

    return args
  }
}
