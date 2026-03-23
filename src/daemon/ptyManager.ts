/**
 * PTY Manager for Daemon
 * Manages PTY sessions that persist independently of Electron app lifecycle
 */

import * as pty from 'node-pty'
import * as path from 'path'
import * as os from 'os'
import * as fs from 'fs'
import { execSync } from 'child_process'
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
  scrollback: string[]
  scrollbackLimit: number
  scrollbackSize: number // Track current size in bytes
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
  constructor(private scrollbackLimit: number = 1024 * 1024) {
    // scrollbackLimit is now in bytes (default 1 MB)
  }

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
      args = ['-p', profile, process.env.SHELL || '/bin/zsh', '-l']
      env.TREETERM_SANDBOXED = '1'
      env.PS1 = '[SANDBOX] ' + (env.PS1 || '\\$ ')
    } else if (isSandboxed && process.platform === 'linux') {
      if (isBwrapAvailable()) {
        const bwrapArgs = generateBwrapArgs(cwd, config.sandbox!)
        shell = 'bwrap'
        args = [...bwrapArgs, '--', process.env.SHELL || '/bin/bash', '-l']
        env.TREETERM_SANDBOXED = '1'
        env.PS1 = '[SANDBOX] ' + (env.PS1 || '\\$ ')
      } else {
        log.warn('bwrap not found, sandbox not available')
        shell = process.env.SHELL || '/bin/bash'
        args = ['-l']
      }
    } else if (isSandboxed) {
      log.warn({ platform: process.platform }, 'sandbox not available on this platform')
      shell = process.platform === 'win32' ? 'powershell.exe' : process.env.SHELL || '/bin/zsh'
      if (process.platform !== 'win32') args = ['-l']
    } else {
      shell = process.platform === 'win32' ? 'powershell.exe' : process.env.SHELL || '/bin/zsh'
      if (process.platform !== 'win32') args = ['-l']
    }

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
      scrollback: [],
      scrollbackLimit: this.scrollbackLimit,
      scrollbackSize: 0,
      createdAt: Date.now(),
      lastActivity: Date.now(),
      sandbox: config.sandbox
    }

    ptyProcess.onData((data) => {
      session.lastActivity = Date.now()
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
      scrollback: [...session.scrollback],
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

  private appendScrollback(session: PtySession, data: string): void {
    session.scrollback.push(data)
    session.scrollbackSize += Buffer.byteLength(data, 'utf-8')

    // Truncate if exceeds size limit (in bytes)
    while (session.scrollbackSize > session.scrollbackLimit && session.scrollback.length > 0) {
      const removed = session.scrollback.shift()!
      session.scrollbackSize -= Buffer.byteLength(removed, 'utf-8')
    }
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
