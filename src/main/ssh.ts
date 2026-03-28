/**
 * SSH Tunnel Manager
 * Manages SSH tunnel processes for forwarding remote daemon sockets to local.
 */

import { spawn, ChildProcess } from 'child_process'
import * as fs from 'fs'
import * as path from 'path'

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
   * Requires an arch-specific binary (e.g., treeterm-daemon-x86_64-linux) to exist —
   * never falls back to the local (macOS) binary, which would silently upload the wrong arch.
   */
  private getDaemonBinaryPath(remoteArch: string): string {
    const baseDir = app.isPackaged
      ? path.join(process.resourcesPath, 'daemon-rs')
      : path.join(__dirname, '../daemon-rs')

    const archBinary = path.join(baseDir, `treeterm-daemon-${remoteArch}-linux`)
    if (fs.existsSync(archBinary)) {
      return archBinary
    }

    throw new Error(
      `No daemon binary found for remote arch "${remoteArch}" (looked for ${archBinary}). ` +
        `Run \`npm run build:daemon-rs:remote\` to cross-compile for Linux.`,
    )
  }

  /**
   * Upload the daemon binary to the remote host via scp.
   */
  private async uploadDaemon(remotePath: string, remoteArch: string): Promise<void> {
    const localPath = this.getDaemonBinaryPath(remoteArch)

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
        '# Report system architecture for binary selection',
        'echo "TREETERM_ARCH:$(uname -m)"',
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
        'DAEMON_LOG="$HOME/.treeterm/daemon.log"',
        'DAEMON_PID=""',
        'if [ -x "$DAEMON_BIN" ] && [ ! -S "$DAEMON_SOCKET" ]; then',
        '  "$DAEMON_BIN" >> "$DAEMON_LOG" 2>&1 &',
        '  DAEMON_PID=$!',
        'fi',
        '',
        '# Wait for socket to appear (only if binary exists)',
        'if [ -x "$DAEMON_BIN" ]; then',
        '  for i in $(seq 1 40); do',
        '    [ -S "$DAEMON_SOCKET" ] && break',
        '    # Check if daemon process died early',
        '    if [ -n "$DAEMON_PID" ] && ! kill -0 "$DAEMON_PID" 2>/dev/null; then',
        '      break',
        '    fi',
        '    sleep 0.25',
        '  done',
        'fi',
        '',
        'if [ ! -S "$DAEMON_SOCKET" ]; then',
        '  # Log daemon output if binary existed but failed (possibly wrong architecture)',
        '  if [ "$NEEDS_UPLOAD" = "0" ] && [ -f "$DAEMON_LOG" ]; then',
        '    echo "Daemon binary exists but failed to start (possibly wrong architecture)" >&2',
        '    tail -20 "$DAEMON_LOG" >&2',
        '  fi',
        '  echo "TREETERM_SOCKET:NEEDS_UPLOAD"',
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
          reject(new Error(`SSH bootstrap failed (exit ${code}). See log output for details.`))
          return
        }

        // Parse socket path from stdout
        const match = stdout.match(/TREETERM_SOCKET:(.+)/)
        if (match) {
          const socketPath = match[1].trim()

          if (socketPath === 'NEEDS_UPLOAD') {
            // Binary not on remote or wrong architecture — upload and retry
            try {
              const archMatch = stdout.match(/TREETERM_ARCH:(\S+)/)
              const remoteArch = archMatch ? archMatch[1].trim() : undefined
              if (!remoteArch) {
                reject(new Error('Could not detect remote architecture (TREETERM_ARCH not reported)'))
                return
              }
              this.appendOutput(`[ssh] Uploading daemon binary (remote arch: ${remoteArch})...`)
              await this.uploadDaemon('~/.treeterm/treeterm-daemon', remoteArch)

              // Make executable and start via ssh, waiting for socket to be ready
              const startArgs = this.buildBaseSSHArgs()
              const startScript = [
                'chmod +x ~/.treeterm/treeterm-daemon',
                'DAEMON_SOCKET="/tmp/treeterm-$(id -u)/daemon.sock"',
                'mkdir -p "/tmp/treeterm-$(id -u)"',
                '~/.treeterm/treeterm-daemon >> ~/.treeterm/daemon.log 2>&1 &',
                'DAEMON_PID=$!',
                'for i in $(seq 1 40); do',
                '  [ -S "$DAEMON_SOCKET" ] && break',
                '  kill -0 "$DAEMON_PID" 2>/dev/null || break',
                '  sleep 0.25',
                'done',
                'if [ -S "$DAEMON_SOCKET" ]; then',
                '  echo "TREETERM_SOCKET:$DAEMON_SOCKET"',
                'else',
                '  echo "Daemon failed to start after upload." >&2',
                '  if [ -f ~/.treeterm/daemon.log ]; then',
                '    echo "Last 20 lines of daemon.log:" >&2',
                '    tail -20 ~/.treeterm/daemon.log >&2',
                '  fi',
                '  if [ -n "$DAEMON_PID" ] && ! kill -0 "$DAEMON_PID" 2>/dev/null; then',
                '    wait "$DAEMON_PID" 2>/dev/null',
                '    echo "Daemon process $DAEMON_PID exited (died early)." >&2',
                '  fi',
                '  exit 1',
                'fi',
              ].join('\n')
              startArgs.push(startScript)
              const startResult = await this.runSSHCommand(startArgs, 'start')
              const startMatch = startResult.match(/TREETERM_SOCKET:(.+)/)
              if (!startMatch) {
                reject(new Error('Daemon failed to start after upload — check daemon log on remote'))
                return
              }
              resolve(startMatch[1].trim())
            } catch (uploadErr) {
              reject(new Error(`Failed to start remote daemon: ${uploadErr instanceof Error ? uploadErr.message : String(uploadErr)}`))
            }
            return
          }

          resolve(socketPath)
        } else {
          // Fall back to default socket path using uid from output
          const uidMatch = stdout.match(/uid=(\d+)/)
          if (uidMatch) {
            resolve(`/tmp/treeterm-${uidMatch[1]}/daemon.sock`)
          } else {
            reject(new Error('Failed to parse daemon socket path from bootstrap output'))
          }
        }
      })

      proc.on('error', (err) => {
        reject(new Error(`Failed to spawn ssh: ${err.message}`))
      })
    })
  }

  private runSSHCommand(sshArgs: string[], prefix = 'ssh'): Promise<string> {
    return new Promise((resolve, reject) => {
      const proc = spawn('ssh', sshArgs, { stdio: ['pipe', 'pipe', 'pipe'] })
      let stdout = ''
      let stderr = ''
      proc.stdout?.on('data', (d: Buffer) => {
        const text = d.toString()
        stdout += text
        for (const line of text.split('\n').filter(Boolean)) {
          this.appendOutput(`[${prefix}] ${line}`)
        }
      })
      proc.stderr?.on('data', (d: Buffer) => {
        const text = d.toString()
        stderr += text
        for (const line of text.split('\n').filter(Boolean)) {
          this.appendOutput(`[${prefix}:err] ${line}`)
        }
      })
      proc.on('close', (code) => {
        if (code !== 0) {
          const firstLine = stderr.split('\n').find(l => l.trim()) ?? 'non-zero exit'
          reject(new Error(`SSH command failed (exit ${code}): ${firstLine}`))
        } else {
          resolve(stdout)
        }
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
