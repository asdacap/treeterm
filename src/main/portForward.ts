/**
 * SSH Port Forward Process Manager
 * Manages a single `ssh -N -L` process for local port forwarding.
 */

import { spawn, ChildProcess } from 'child_process'
import type { SSHConnectionConfig, PortForwardConfig, PortForwardInfo, PortForwardStatus } from '../shared/types'

type OutputCallback = (line: string) => void
type StatusCallback = (info: PortForwardInfo) => void

export class PortForwardProcess {
  private process: ChildProcess | null = null
  private outputBuffer: string[] = []
  private outputListeners: Set<OutputCallback> = new Set()
  private statusListeners: Set<StatusCallback> = new Set()
  private _status: PortForwardStatus = 'connecting'
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

    this.appendOutput(`[portfwd] Starting port forward: localhost:${this.config.localPort} -> ${this.config.remoteHost}:${this.config.remotePort}`)
    this.appendOutput(`[portfwd] $ ssh ${args.join(' ')}`)

    this.process = spawn('ssh', args, { stdio: ['pipe', 'pipe', 'pipe'] })

    // After 2 seconds without exit, consider the forward active
    this.stabilizationTimer = setTimeout(() => {
      if (this._status === 'connecting' && this.process !== null) {
        this.setStatus('active')
      }
    }, 2000)

    this.process.stdout?.on('data', (data: Buffer) => {
      for (const line of data.toString().split('\n').filter(Boolean)) {
        this.appendOutput(`[portfwd] ${line}`)
      }
    })

    this.process.stderr?.on('data', (data: Buffer) => {
      for (const line of data.toString().split('\n').filter(Boolean)) {
        this.appendOutput(`[portfwd:err] ${line}`)
      }
    })

    this.process.on('close', (code) => {
      this.clearStabilizationTimer()
      this.process = null
      if (this._status !== 'stopped') {
        const msg = `Port forward process exited (code ${code})`
        this.appendOutput(`[portfwd] ${msg}`)
        this.setStatus('error', msg)
      }
    })

    this.process.on('error', (err) => {
      this.clearStabilizationTimer()
      this.process = null
      const msg = `Port forward error: ${err.message}`
      this.appendOutput(`[portfwd] ${msg}`)
      this.setStatus('error', msg)
    })
  }

  stop(): void {
    this.clearStabilizationTimer()
    this.setStatus('stopped')
    if (this.process) {
      this.process.kill()
      this.process = null
    }
  }

  getOutput(): string[] {
    return [...this.outputBuffer]
  }

  onOutput(cb: OutputCallback): () => void {
    this.outputListeners.add(cb)
    return () => this.outputListeners.delete(cb)
  }

  onStatusChange(cb: StatusCallback): () => void {
    this.statusListeners.add(cb)
    return () => this.statusListeners.delete(cb)
  }

  toInfo(): PortForwardInfo {
    const base = {
      id: this.config.id,
      connectionId: this.config.connectionId,
      localPort: this.config.localPort,
      remoteHost: this.config.remoteHost,
      remotePort: this.config.remotePort,
    }
    if (this._status === 'error') {
      return { ...base, status: 'error', error: this._error ?? 'Unknown error' }
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
      '-L', `${localPort}:${remoteHost}:${remotePort}`,
      '-o', 'StrictHostKeyChecking=accept-new',
      '-o', 'BatchMode=yes',
      '-o', 'ExitOnForwardFailure=yes',
      '-o', 'ServerAliveInterval=30',
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
