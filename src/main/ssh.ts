/**
 * SSH Tunnel Manager
 * Manages SSH tunnel processes for forwarding remote daemon sockets to local.
 */

import { spawn, execSync, ChildProcess } from 'child_process'
import * as fs from 'fs'
import * as path from 'path'
import * as os from 'os'
import { app } from 'electron'
import type { SSHConnectionConfig } from '../shared/types'
import { getRemoteForwardSocketPath } from './socketPath'

type OutputCallback = (line: string) => void
type DisconnectCallback = (error?: string) => void

export interface SSHTunnelOptions {
  refreshDaemon?: boolean
}

export class SSHTunnel {
  private sshProcess: ChildProcess | null = null
  private outputBuffer: string[] = []
  private outputListeners: Set<OutputCallback> = new Set()
  private disconnectListeners: Set<DisconnectCallback> = new Set()
  private localSocketPath: string
  private _connected: boolean = false
  private options: SSHTunnelOptions

  constructor(private config: SSHConnectionConfig, options?: SSHTunnelOptions) {
    this.localSocketPath = getRemoteForwardSocketPath(config.id)
    this.options = options || {}
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
    console.log(line)
    this.outputBuffer.push(line)
    // Keep buffer bounded
    if (this.outputBuffer.length > 1000) {
      this.outputBuffer = this.outputBuffer.slice(-500)
    }
    for (const cb of this.outputListeners) {
      cb(line)
    }
  }

  /**
   * Get the local path to the daemon binary for the given remote architecture.
   */
  private getDaemonBinaryPath(): string {
    const daemonPath = app.isPackaged
      ? path.join(process.resourcesPath, 'daemon-rs', 'treeterm-daemon')
      : path.join(__dirname, '../daemon-rs/treeterm-daemon')
    return daemonPath
  }

  /**
   * Upload the daemon binary to the remote host via scp.
   */
  private async uploadDaemon(remotePath: string): Promise<void> {
    const localPath = this.getDaemonBinaryPath()
    if (!fs.existsSync(localPath)) {
      throw new Error(`Daemon binary not found at ${localPath}`)
    }

    const scpArgs = [
      '-o', 'StrictHostKeyChecking=accept-new',
      '-o', 'BatchMode=yes',
      '-P', String(this.config.port),
    ]
    if (this.config.identityFile) {
      scpArgs.push('-i', this.config.identityFile)
    }
    scpArgs.push(localPath, `${this.config.user}@${this.config.host}:${remotePath}`)

    this.appendOutput(`[ssh] Uploading daemon binary to ${remotePath}...`)

    return new Promise<void>((resolve, reject) => {
      const proc = spawn('scp', scpArgs, { stdio: ['pipe', 'pipe', 'pipe'] })
      let stderr = ''
      proc.stderr?.on('data', (data: Buffer) => { stderr += data.toString() })
      proc.on('close', (code) => {
        if (code !== 0) {
          reject(new Error(`scp failed (exit ${code}): ${stderr}`))
        } else {
          this.appendOutput(`[ssh] Upload complete`)
          resolve()
        }
      })
      proc.on('error', (err) => reject(new Error(`scp spawn error: ${err.message}`)))
    })
  }

  private async bootstrapRemoteDaemon(): Promise<string> {
    return new Promise<string>((resolve, reject) => {
      const sshArgs = this.buildBaseSSHArgs()

      const refreshDaemon = this.options.refreshDaemon ? '1' : '0'
      const bootstrapScript = [
        'set -e',
        'DAEMON_BIN="$HOME/.treeterm/treeterm-daemon"',
        'DAEMON_SOCKET="/tmp/treeterm-$(id -u)/daemon.sock"',
        `REFRESH_DAEMON=${refreshDaemon}`,
        '',
        '# Check if daemon binary exists and get its version',
        'NEEDS_UPLOAD=0',
        'if [ ! -x "$DAEMON_BIN" ]; then',
        '  NEEDS_UPLOAD=1',
        'fi',
        '',
        '# Kill old daemon if refresh requested',
        'if [ "$REFRESH_DAEMON" = "1" ] && [ -S "$DAEMON_SOCKET" ]; then',
        '  echo "Refreshing daemon: killing old process..."',
        '  pkill -f "treeterm-daemon" 2>/dev/null || true',
        '  rm -f "$DAEMON_SOCKET"',
        '  sleep 0.5',
        'fi',
        '',
        '# Report if upload is needed',
        'echo "TREETERM_NEEDS_UPLOAD:$NEEDS_UPLOAD"',
        '',
        '# Start daemon if not already running',
        'mkdir -p "$HOME/.treeterm"',
        'mkdir -p "/tmp/treeterm-$(id -u)"',
        'if [ -x "$DAEMON_BIN" ] && [ ! -S "$DAEMON_SOCKET" ]; then',
        '  DAEMON_LOG="$HOME/.treeterm/daemon.log"',
        '  "$DAEMON_BIN" >> "$DAEMON_LOG" 2>&1 &',
        'fi',
        '',
        '# Wait for socket to appear (only if binary exists)',
        'if [ -x "$DAEMON_BIN" ]; then',
        '  for i in $(seq 1 40); do',
        '    [ -S "$DAEMON_SOCKET" ] && break',
        '    sleep 0.25',
        '  done',
        'fi',
        '',
        'if [ ! -S "$DAEMON_SOCKET" ]; then',
        '  if [ "$NEEDS_UPLOAD" = "1" ]; then',
        '    echo "TREETERM_SOCKET:NEEDS_UPLOAD"',
        '  else',
        '    echo "TREETERM_ERROR: Daemon failed to start" >&2',
        '    exit 1',
        '  fi',
        'else',
        '  echo "TREETERM_SOCKET:$DAEMON_SOCKET"',
        'fi',
      ].join('\n')

      sshArgs.push(bootstrapScript)

      this.appendOutput(`[ssh] Bootstrapping remote daemon on ${this.config.user}@${this.config.host}...`)
      this.appendOutput(`[ssh] $ ssh ${sshArgs.join(' ')}`)

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

      proc.on('close', async (code) => {
        if (code !== 0) {
          reject(new Error(`SSH bootstrap failed (exit ${code}): ${stderr}`))
          return
        }

        // Parse socket path from stdout
        const match = stdout.match(/TREETERM_SOCKET:(.+)/)
        if (match) {
          const socketPath = match[1].trim()

          if (socketPath === 'NEEDS_UPLOAD') {
            // Binary not on remote — upload it and retry
            try {
              this.appendOutput('[ssh] Daemon binary not found on remote, uploading...')
              await this.uploadDaemon('~/.treeterm/treeterm-daemon')

              // Make executable and start via ssh
              const startArgs = this.buildBaseSSHArgs()
              startArgs.push('chmod +x ~/.treeterm/treeterm-daemon && ~/.treeterm/treeterm-daemon >> ~/.treeterm/daemon.log 2>&1 & sleep 1 && echo "TREETERM_SOCKET:/tmp/treeterm-$(id -u)/daemon.sock"')
              const startResult = await this.runSSHCommand(startArgs)
              const startMatch = startResult.match(/TREETERM_SOCKET:(.+)/)
              resolve(startMatch ? startMatch[1].trim() : `/tmp/treeterm-${1000}/daemon.sock`)
            } catch (uploadErr) {
              reject(new Error(`Failed to upload daemon: ${uploadErr instanceof Error ? uploadErr.message : String(uploadErr)}`))
            }
            return
          }

          resolve(socketPath)
        } else {
          reject(new Error('Failed to parse daemon socket path from bootstrap output'))
        }
      })

      proc.on('error', (err) => {
        reject(new Error(`Failed to spawn ssh: ${err.message}`))
      })
    })
  }

  private runSSHCommand(sshArgs: string[]): Promise<string> {
    return new Promise((resolve, reject) => {
      const proc = spawn('ssh', sshArgs, { stdio: ['pipe', 'pipe', 'pipe'] })
      let stdout = ''
      let stderr = ''
      proc.stdout?.on('data', (d: Buffer) => {
        const text = d.toString()
        stdout += text
        for (const line of text.split('\n').filter(Boolean)) {
          this.appendOutput(`[ssh] ${line}`)
        }
      })
      proc.stderr?.on('data', (d: Buffer) => { stderr += d.toString() })
      proc.on('close', (code) => {
        if (code !== 0) reject(new Error(`SSH command failed (exit ${code}): ${stderr}`))
        else resolve(stdout)
      })
      proc.on('error', (err) => reject(err))
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
    this.appendOutput(`[ssh] $ ssh ${sshArgs.join(' ')}`)

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
