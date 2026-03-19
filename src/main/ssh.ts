/**
 * SSH Tunnel Manager
 * Manages SSH tunnel processes for forwarding remote daemon sockets to local.
 */

import { spawn, ChildProcess } from 'child_process'
import * as fs from 'fs'
import * as path from 'path'
import * as os from 'os'
import type { SSHConnectionConfig } from '../shared/types'
import { getRemoteForwardSocketPath } from '../daemon/socketPath'

type OutputCallback = (line: string) => void
type DisconnectCallback = (error?: string) => void

export class SSHTunnel {
  private sshProcess: ChildProcess | null = null
  private outputBuffer: string[] = []
  private outputListeners: Set<OutputCallback> = new Set()
  private disconnectListeners: Set<DisconnectCallback> = new Set()
  private localSocketPath: string
  private _connected: boolean = false

  constructor(private config: SSHConnectionConfig) {
    this.localSocketPath = getRemoteForwardSocketPath(config.id)
  }

  get connected(): boolean {
    return this._connected
  }

  async connect(): Promise<string> {
    // Ensure the socket directory exists
    const socketDir = path.dirname(this.localSocketPath)
    if (!fs.existsSync(socketDir)) {
      fs.mkdirSync(socketDir, { recursive: true })
    }

    // Clean up stale socket file
    if (fs.existsSync(this.localSocketPath)) {
      fs.unlinkSync(this.localSocketPath)
    }

    // Step 1: Bootstrap remote daemon and get socket path
    const remoteSocketPath = await this.bootstrapRemoteDaemon()

    // Step 2: Start SSH tunnel with socket forwarding
    await this.startTunnel(remoteSocketPath)

    // Step 3: Wait for local socket to appear
    await this.waitForSocket()

    this._connected = true
    return this.localSocketPath
  }

  disconnect(): void {
    this._connected = false
    if (this.sshProcess) {
      this.sshProcess.kill()
      this.sshProcess = null
    }
    // Clean up socket file
    try {
      if (fs.existsSync(this.localSocketPath)) {
        fs.unlinkSync(this.localSocketPath)
      }
    } catch {
      // Ignore cleanup errors
    }
  }

  getOutput(): string[] {
    return [...this.outputBuffer]
  }

  onOutput(cb: OutputCallback): () => void {
    this.outputListeners.add(cb)
    return () => this.outputListeners.delete(cb)
  }

  onDisconnect(cb: DisconnectCallback): () => void {
    this.disconnectListeners.add(cb)
    return () => this.disconnectListeners.delete(cb)
  }

  private appendOutput(line: string): void {
    this.outputBuffer.push(line)
    // Keep buffer bounded
    if (this.outputBuffer.length > 1000) {
      this.outputBuffer = this.outputBuffer.slice(-500)
    }
    for (const cb of this.outputListeners) {
      cb(line)
    }
  }

  private async bootstrapRemoteDaemon(): Promise<string> {
    return new Promise<string>((resolve, reject) => {
      const sshArgs = this.buildBaseSSHArgs()

      // Bootstrap script: check for treeterm, install if needed, start daemon, print socket path
      const bootstrapScript = [
        'set -e',
        // Try to find treeterm
        'if command -v treeterm >/dev/null 2>&1; then',
        '  TREETERM_CMD=treeterm',
        'elif npx --yes github:asdacap/treeterm --help >/dev/null 2>&1; then',
        '  TREETERM_CMD="npx --yes github:asdacap/treeterm"',
        'else',
        '  echo "TREETERM_ERROR: treeterm not found. Install it or ensure npx is available." >&2',
        '  exit 1',
        'fi',
        // Start daemon if not running, get socket path
        '$TREETERM_CMD status >/dev/null 2>&1 || true',
        // Print the socket path (same logic as getDefaultSocketPath)
        'echo "TREETERM_SOCKET:$(mktemp -u /tmp/treeterm-$(id -u)/daemon.sock | xargs dirname)/daemon.sock"',
      ].join('\n')

      sshArgs.push(bootstrapScript)

      this.appendOutput(`[ssh] Bootstrapping remote daemon on ${this.config.user}@${this.config.host}...`)

      const proc = spawn('ssh', sshArgs, { stdio: ['pipe', 'pipe', 'pipe'] })
      let stdout = ''
      let stderr = ''

      proc.stdout?.on('data', (data: Buffer) => {
        const text = data.toString()
        stdout += text
        for (const line of text.split('\n').filter(Boolean)) {
          this.appendOutput(`[bootstrap] ${line}`)
        }
      })

      proc.stderr?.on('data', (data: Buffer) => {
        const text = data.toString()
        stderr += text
        for (const line of text.split('\n').filter(Boolean)) {
          this.appendOutput(`[bootstrap:err] ${line}`)
        }
      })

      proc.on('close', (code) => {
        if (code !== 0) {
          reject(new Error(`SSH bootstrap failed (exit ${code}): ${stderr}`))
          return
        }

        // Parse socket path from stdout
        const match = stdout.match(/TREETERM_SOCKET:(.+)/)
        if (match) {
          resolve(match[1].trim())
        } else {
          // Fall back to default path pattern
          const uid = stdout.match(/uid=(\d+)/)
          const uidNum = uid ? uid[1] : '1000'
          resolve(`/tmp/treeterm-${uidNum}/daemon.sock`)
        }
      })

      proc.on('error', (err) => {
        reject(new Error(`Failed to spawn ssh: ${err.message}`))
      })
    })
  }

  private async startTunnel(remoteSocketPath: string): Promise<void> {
    const sshArgs = this.buildBaseSSHArgs()

    // Socket forwarding: local socket -> remote socket
    sshArgs.push(
      '-L', `${this.localSocketPath}:${remoteSocketPath}`,
      '-o', 'ExitOnForwardFailure=yes',
      '-o', 'ServerAliveInterval=30',
      '-o', 'ServerAliveCountMax=3',
      // Keep connection alive with a shell (no -N so we can see output)
      'cat' // Simple command that keeps the connection open
    )

    this.appendOutput(`[ssh] Starting tunnel: ${this.localSocketPath} -> ${remoteSocketPath}`)

    this.sshProcess = spawn('ssh', sshArgs, { stdio: ['pipe', 'pipe', 'pipe'] })

    this.sshProcess.stdout?.on('data', (data: Buffer) => {
      for (const line of data.toString().split('\n').filter(Boolean)) {
        this.appendOutput(`[tunnel] ${line}`)
      }
    })

    this.sshProcess.stderr?.on('data', (data: Buffer) => {
      for (const line of data.toString().split('\n').filter(Boolean)) {
        this.appendOutput(`[tunnel:err] ${line}`)
      }
    })

    this.sshProcess.on('close', (code) => {
      const wasConnected = this._connected
      this._connected = false
      this.sshProcess = null

      const msg = `SSH tunnel closed (exit ${code})`
      this.appendOutput(`[ssh] ${msg}`)

      if (wasConnected) {
        for (const cb of this.disconnectListeners) {
          cb(msg)
        }
      }
    })

    this.sshProcess.on('error', (err) => {
      this._connected = false
      this.sshProcess = null
      const msg = `SSH tunnel error: ${err.message}`
      this.appendOutput(`[ssh] ${msg}`)

      for (const cb of this.disconnectListeners) {
        cb(msg)
      }
    })
  }

  private async waitForSocket(): Promise<void> {
    const maxWait = 15000 // 15 seconds
    const pollInterval = 200
    const start = Date.now()

    while (Date.now() - start < maxWait) {
      if (fs.existsSync(this.localSocketPath)) {
        this.appendOutput(`[ssh] Local socket ready: ${this.localSocketPath}`)
        return
      }

      // Check if SSH process died
      if (this.sshProcess === null) {
        throw new Error('SSH process exited before socket was ready')
      }

      await new Promise(resolve => setTimeout(resolve, pollInterval))
    }

    throw new Error(`Timed out waiting for local socket: ${this.localSocketPath}`)
  }

  private buildBaseSSHArgs(): string[] {
    const args = [
      '-o', 'StrictHostKeyChecking=accept-new',
      '-o', 'BatchMode=yes',
      '-p', String(this.config.port),
    ]

    if (this.config.identityFile) {
      args.push('-i', this.config.identityFile)
    }

    args.push(`${this.config.user}@${this.config.host}`)

    return args
  }
}
