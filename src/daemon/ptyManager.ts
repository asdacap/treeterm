/**
 * PTY Manager for Daemon
 * Manages PTY sessions that persist independently of Electron app lifecycle
 */

import * as pty from 'node-pty'
import * as path from 'path'
import * as os from 'os'
import * as fs from 'fs'
import { execSync } from 'child_process'
import { Terminal } from '@xterm/headless'
import type { CreateSessionConfig, SessionInfo } from './protocol'
import type { SandboxConfig } from '../main/pty'
import { createModuleLogger } from './logger'

const log = createModuleLogger('ptyManager')

export interface PtySession {
  id: string
  pty: pty.IPty
  cwd: string
  env: Record<string, string>
  cols: number
  rows: number
  buffer1: string[]      // compacted scrollback buffer
  buffer1Size: number    // bytes
  buffer2: string[]      // accumulator buffer
  buffer2Size: number    // bytes
  createdAt: number
  lastActivity: number
  sandbox?: SandboxConfig
  exitCode?: number
}

type DataCallback = (sessionId: string, data: string) => void
type ExitCallback = (sessionId: string, exitCode: number, signal?: number) => void

function isBwrapAvailable(): boolean {
  try {
    execSync('which bwrap', { stdio: 'ignore' })
    return true
  } catch (error) {
    log.debug({ err: error }, 'bwrap check failed')
    return false
  }
}

function getGitRoot(workspacePath: string): string | null {
  const gitPath = path.join(workspacePath, '.git')
  const stat = fs.statSync(gitPath, { throwIfNoEntry: false })
  if (!stat) return null

  if (stat.isFile()) {
    const content = fs.readFileSync(gitPath, 'utf-8')
    const match = content.match(/^gitdir:\s*(.+)$/m)
    if (match) {
      const worktreeGitDir = path.resolve(workspacePath, match[1].trim())
      return path.resolve(worktreeGitDir, '../../../')
    }
  }
  return null
}

function generateBwrapArgs(cwd: string, sandbox: SandboxConfig): string[] {
  const args: string[] = ['--die-with-parent', '--unshare-pid', '--unshare-uts', '--unshare-ipc']

  if (!sandbox.allowNetwork) {
    args.push('--unshare-net')
  }

  const roBinds = ['/usr', '/bin', '/lib', '/lib64', '/etc', '/opt']
  for (const p of roBinds) {
    if (fs.existsSync(p)) {
      args.push('--ro-bind', p, p)
    }
  }

  args.push('--proc', '/proc')
  args.push('--dev', '/dev')
  args.push('--tmpfs', '/tmp')
  args.push('--bind', cwd, cwd)
  args.push('--chdir', cwd)

  const gitRoot = getGitRoot(cwd)
  if (gitRoot) {
    args.push('--ro-bind', gitRoot, gitRoot)
  }

  const home = os.homedir()
  const homeFiles = ['.bashrc', '.zshrc', '.profile', '.gitconfig']
  for (const f of homeFiles) {
    const p = path.join(home, f)
    if (fs.existsSync(p)) {
      args.push('--ro-bind', p, p)
    }
  }

  for (const p of sandbox.allowedPaths) {
    if (fs.existsSync(p)) {
      args.push('--ro-bind', p, p)
    }
  }

  return args
}

function generateSandboxProfile(workspacePath: string, sandbox: SandboxConfig): string {
  const allowedPaths = [
    workspacePath,
    os.tmpdir(),
    '/usr',
    '/bin',
    '/sbin',
    '/Library/Frameworks',
    '/System',
    '/private/var/folders',
    '/dev',
    ...sandbox.allowedPaths
  ]

  const pathRules = allowedPaths
    .map(
      (p) => `
    (allow file-read* file-write* (subpath "${p}"))
    (allow file-read* (literal "${p}"))`
    )
    .join('\n')

  const networkRule = sandbox.allowNetwork ? '(allow network*)' : '(deny network*)'

  return `
(version 1)
(deny default)

;; Allow basic process operations
(allow process-fork)
(allow process-exec)
(allow signal)
(allow sysctl-read)

;; Allow reading system files
(allow file-read*
  (literal "/")
  (subpath "/usr")
  (subpath "/bin")
  (subpath "/sbin")
  (subpath "/Library")
  (subpath "/System")
  (subpath "/private/etc")
  (subpath "/private/var/db")
  (subpath "/dev")
)

;; Workspace access
${pathRules}

;; Home directory read for shell config
(allow file-read* (subpath "${os.homedir()}"))

;; Network
${networkRule}

;; Allow mach services for basic functionality
(allow mach-lookup)
(allow ipc-posix-shm)
`
}

export class DaemonPtyManager {
  private sessions: Map<string, PtySession> = new Map()
  private counter = 0
  private dataCallbacks: Set<DataCallback> = new Set()
  private exitCallbacks: Set<ExitCallback> = new Set()
  private sessionDataCallbacks: Map<string, Set<(data: string) => void>> = new Map()
  private sessionExitCallbacks: Map<string, Set<(exitCode: number, signal?: number) => void>> = new Map()
  private sessionResizeCallbacks: Map<string, Set<(cols: number, rows: number) => void>> = new Map()
  constructor(
    private mergeThreshold: number = 50 * 1024,         // 50KB - triggers compaction when buffer2 exceeds this
    private compactedLimit: number = 1024 * 1024,        // 1MB - max buffer1 size after compaction
    private scrollbackLines: number = 10000              // headless xterm line limit for compaction measurement
  ) {}

  create(config: CreateSessionConfig): string {
    const id = `pty-${++this.counter}`
    const isSandboxed = config.sandbox?.enabled ?? false

    let shell: string
    let args: string[] = []
    let env = { ...process.env, ...config.env } as { [key: string]: string }

    const cwd = config.cwd
    const cols = config.cols ?? 80
    const rows = config.rows ?? 24

    if (isSandboxed && process.platform === 'darwin') {
      shell = '/usr/bin/sandbox-exec'
      const profile = generateSandboxProfile(cwd, config.sandbox!)
      args = ['-p', profile, process.env.SHELL || 'zsh', '-l']
      env.TREETERM_SANDBOXED = '1'
      env.PS1 = '[SANDBOX] ' + (env.PS1 || '\\$ ')
    } else if (isSandboxed && process.platform === 'linux') {
      if (isBwrapAvailable()) {
        const bwrapArgs = generateBwrapArgs(cwd, config.sandbox!)
        shell = 'bwrap'
        args = [...bwrapArgs, '--', process.env.SHELL || 'bash', '-l']
        env.TREETERM_SANDBOXED = '1'
        env.PS1 = '[SANDBOX] ' + (env.PS1 || '\\$ ')
      } else {
        log.warn('bwrap not found, sandbox not available')
        shell = process.env.SHELL || 'bash'
        args = ['-l']
      }
    } else if (isSandboxed) {
      log.warn({ platform: process.platform }, 'sandbox not available on this platform')
      shell = process.platform === 'win32' ? 'powershell.exe' : process.env.SHELL || 'zsh'
      if (process.platform !== 'win32') args = ['-l']
    } else {
      shell = process.platform === 'win32' ? 'powershell.exe' : process.env.SHELL || 'zsh'
      if (process.platform !== 'win32') args = ['-l']
    }

    log.info({
      shell,
      args,
      envShell: env.SHELL,
      envTerm: env.TERM,
      envPS1: env.PS1,
      envPromptCommand: env.PROMPT_COMMAND,
      envBashEnv: env.BASH_ENV,
      envHome: env.HOME,
      envLang: env.LANG,
    }, 'PTY spawn environment')

    const ptyProcess = pty.spawn(shell, args, {
      name: 'xterm-256color',
      cols,
      rows,
      cwd,
      env
    })

    const session: PtySession = {
      id,
      pty: ptyProcess,
      cwd,
      env,
      cols,
      rows,
      buffer1: [],
      buffer1Size: 0,
      buffer2: [],
      buffer2Size: 0,
      createdAt: Date.now(),
      lastActivity: Date.now(),
      sandbox: config.sandbox
    }

    // Debug: log first few data chunks to see raw PTY output including prompt
    let debugChunks = 0
    ptyProcess.onData((data) => {
      session.lastActivity = Date.now()
      if (debugChunks < 5) {
        debugChunks++
        const hex = Buffer.from(data).toString('hex')
        const printable = data.replace(/[\x00-\x1f]/g, (c: string) => {
          const code = c.charCodeAt(0)
          if (code === 0x1b) return '\\x1b'
          if (code === 0x0a) return '\\n'
          if (code === 0x0d) return '\\r'
          return `\\x${code.toString(16).padStart(2, '0')}`
        })
        log.info({ sessionId: id, chunk: debugChunks, printable, hex: hex.slice(0, 400) }, 'PTY raw output')
      }
      this.appendScrollback(session, data)
      this.broadcastData(id, data)
    })

    ptyProcess.onExit(({ exitCode, signal }) => {
      session.exitCode = exitCode
      this.broadcastExit(id, exitCode, signal)
      log.info({ sessionId: id, exitCode, signal, cwd }, 'session exited')
    })

    this.sessions.set(id, session)

    // Execute startup command if provided
    if (config.startupCommand && config.startupCommand.trim()) {
      setTimeout(() => {
        // Use 'exec' on Unix platforms to replace the shell process
        // This ensures the PTY exits when the command exits (e.g., AI terminal closes when AI exits)
        const cmd = process.platform === 'win32'
          ? config.startupCommand!.trim()
          : `exec ${config.startupCommand!.trim()}`
        ptyProcess.write(cmd + '\n')
      }, 100)
    }

    log.info({ sessionId: id, cwd, sandbox: isSandboxed }, 'session created')
    return id
  }

  attach(sessionId: string): { scrollback: string[]; session: SessionInfo; exitCode?: number } {
    const session = this.sessions.get(sessionId)
    if (!session) {
      throw new Error(`Session ${sessionId} not found`)
    }

    session.lastActivity = Date.now()

    return {
      scrollback: [...session.buffer1, ...session.buffer2],
      session: this.getSessionInfo(session),
      exitCode: session.exitCode
    }
  }

  write(sessionId: string, data: string): void {
    const session = this.sessions.get(sessionId)
    if (!session) {
      throw new Error(`Session ${sessionId} not found`)
    }

    // No-op if session has exited
    if (session.exitCode !== undefined) return

    session.pty.write(data)
    session.lastActivity = Date.now()
  }

  resize(sessionId: string, cols: number, rows: number): void {
    const session = this.sessions.get(sessionId)
    if (!session) {
      throw new Error(`Session ${sessionId} not found`)
    }

    // No-op if session has exited
    if (session.exitCode !== undefined) return

    session.pty.resize(cols, rows)
    session.cols = cols
    session.rows = rows
    session.lastActivity = Date.now()
    this.broadcastResize(sessionId, cols, rows)
  }

  kill(sessionId: string): void {
    const session = this.sessions.get(sessionId)
    if (!session) {
      log.warn({ sessionId }, 'kill: session not found')
      return
    }

    log.info({ sessionId, cwd: session.cwd }, 'killing session')
    session.pty.kill()
    this.sessions.delete(sessionId)
  }

  listSessions(): SessionInfo[] {
    const sessions = Array.from(this.sessions.values()).map((session) => this.getSessionInfo(session))
    log.debug({ count: sessions.length }, 'listSessions called')
    return sessions
  }

  onData(callback: DataCallback): () => void {
    this.dataCallbacks.add(callback)
    return () => this.dataCallbacks.delete(callback)
  }

  onExit(callback: ExitCallback): () => void {
    this.exitCallbacks.add(callback)
    return () => this.exitCallbacks.delete(callback)
  }

  onSessionData(sessionId: string, callback: (data: string) => void): () => void {
    if (!this.sessionDataCallbacks.has(sessionId)) {
      this.sessionDataCallbacks.set(sessionId, new Set())
    }
    this.sessionDataCallbacks.get(sessionId)!.add(callback)
    return () => {
      const cbs = this.sessionDataCallbacks.get(sessionId)
      if (cbs) {
        cbs.delete(callback)
        if (cbs.size === 0) this.sessionDataCallbacks.delete(sessionId)
      }
    }
  }

  onSessionExit(sessionId: string, callback: (exitCode: number, signal?: number) => void): () => void {
    if (!this.sessionExitCallbacks.has(sessionId)) {
      this.sessionExitCallbacks.set(sessionId, new Set())
    }
    this.sessionExitCallbacks.get(sessionId)!.add(callback)
    return () => {
      const cbs = this.sessionExitCallbacks.get(sessionId)
      if (cbs) {
        cbs.delete(callback)
        if (cbs.size === 0) this.sessionExitCallbacks.delete(sessionId)
      }
    }
  }

  onSessionResize(sessionId: string, callback: (cols: number, rows: number) => void): () => void {
    if (!this.sessionResizeCallbacks.has(sessionId)) {
      this.sessionResizeCallbacks.set(sessionId, new Set())
    }
    this.sessionResizeCallbacks.get(sessionId)!.add(callback)
    return () => {
      const cbs = this.sessionResizeCallbacks.get(sessionId)
      if (cbs) {
        cbs.delete(callback)
        if (cbs.size === 0) this.sessionResizeCallbacks.delete(sessionId)
      }
    }
  }

  private appendScrollback(session: PtySession, data: string): void {
    session.buffer2.push(data)
    session.buffer2Size += Buffer.byteLength(data, 'utf-8')

    if (session.buffer2Size > this.mergeThreshold) {
      this.compactScrollback(session)
    }
  }

  private countTerminalLines(chunks: string[], cols: number, rows: number): number {
    const terminal = new Terminal({ cols, rows, scrollback: this.scrollbackLines, allowProposedApi: true })
    for (const chunk of chunks) {
      terminal.write(chunk)
    }
    // Force synchronous flush of all pending writes
    // Terminal.write() is async internally; we need to read buffer after all data is parsed
    // Use a synchronous write to flush the queue
    terminal.write('', () => {})
    const lines = terminal.buffer.normal.length
    terminal.dispose()
    return lines
  }

  private compactScrollback(session: PtySession): void {
    const { cols, rows } = session

    // Measure combined buffer1 + buffer2
    const combinedLines = this.countTerminalLines(
      [...session.buffer1, ...session.buffer2], cols, rows
    )

    // Measure buffer2 alone
    const buffer2Lines = this.countTerminalLines(session.buffer2, cols, rows)

    if (buffer2Lines < combinedLines) {
      // buffer1 contributes meaningful scrollback history — merge both into buffer1
      session.buffer1 = [...session.buffer1, ...session.buffer2]
      session.buffer1Size += session.buffer2Size
    } else {
      // buffer1 is redundant — buffer2 alone captures all visible state
      session.buffer1 = [...session.buffer2]
      session.buffer1Size = session.buffer2Size
    }

    // Clear buffer2
    session.buffer2 = []
    session.buffer2Size = 0

    // Truncate buffer1 if exceeds compacted limit
    while (session.buffer1Size > this.compactedLimit && session.buffer1.length > 0) {
      const removed = session.buffer1.shift()!
      session.buffer1Size -= Buffer.byteLength(removed, 'utf-8')
    }

    log.debug({
      sessionId: session.id,
      buffer1Chunks: session.buffer1.length,
      buffer1Size: session.buffer1Size,
      combinedLines,
      buffer2Lines,
    }, 'scrollback compacted')
  }

  private broadcastData(sessionId: string, data: string): void {
    for (const callback of this.dataCallbacks) {
      callback(sessionId, data)
    }
    const sessionCbs = this.sessionDataCallbacks.get(sessionId)
    if (sessionCbs) {
      for (const callback of sessionCbs) {
        callback(data)
      }
    }
  }

  private broadcastExit(sessionId: string, exitCode: number, signal?: number): void {
    for (const callback of this.exitCallbacks) {
      callback(sessionId, exitCode, signal)
    }
    const sessionCbs = this.sessionExitCallbacks.get(sessionId)
    if (sessionCbs) {
      for (const callback of sessionCbs) {
        callback(exitCode, signal)
      }
    }
  }

  private broadcastResize(sessionId: string, cols: number, rows: number): void {
    const sessionCbs = this.sessionResizeCallbacks.get(sessionId)
    if (sessionCbs) {
      for (const callback of sessionCbs) {
        callback(cols, rows)
      }
    }
  }

  private getSessionInfo(session: PtySession): SessionInfo {
    return {
      id: session.id,
      cwd: session.cwd,
      cols: session.cols,
      rows: session.rows,
      createdAt: session.createdAt,
      lastActivity: session.lastActivity
    }
  }

  shutdown(): void {
    log.info('shutting down PTY manager')

    // Kill all sessions
    for (const [id] of this.sessions) {
      this.kill(id)
    }
  }
}
