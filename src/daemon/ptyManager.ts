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

export interface PtySession {
  id: string
  pty: pty.IPty
  cwd: string
  env: Record<string, string>
  cols: number
  rows: number
  scrollback: string[]
  scrollbackLimit: number
  createdAt: number
  lastActivity: number
  sandbox?: SandboxConfig
  attachedClients: Set<string>
}

type DataCallback = (sessionId: string, data: string) => void
type ExitCallback = (sessionId: string, exitCode: number, signal?: number) => void

function isBwrapAvailable(): boolean {
  try {
    execSync('which bwrap', { stdio: 'ignore' })
    return true
  } catch {
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
  private orphanCleanupInterval: NodeJS.Timeout | null = null
  private orphanTimeout: number

  constructor(orphanTimeout: number = 0, private scrollbackLimit: number = 50000) {
    this.orphanTimeout = orphanTimeout
    if (orphanTimeout > 0) {
      this.startOrphanCleanup()
    }
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
      args = ['-p', profile, process.env.SHELL || '/bin/zsh']
      env.TREETERM_SANDBOXED = '1'
      env.PS1 = '[SANDBOX] ' + (env.PS1 || '\\$ ')
    } else if (isSandboxed && process.platform === 'linux') {
      if (isBwrapAvailable()) {
        const bwrapArgs = generateBwrapArgs(cwd, config.sandbox!)
        shell = 'bwrap'
        args = [...bwrapArgs, '--', process.env.SHELL || '/bin/bash']
        env.TREETERM_SANDBOXED = '1'
        env.PS1 = '[SANDBOX] ' + (env.PS1 || '\\$ ')
      } else {
        console.warn('[daemon] bwrap not found, sandbox not available')
        shell = process.env.SHELL || '/bin/bash'
      }
    } else if (isSandboxed) {
      console.warn('[daemon] sandbox not available on this platform')
      shell = process.platform === 'win32' ? 'powershell.exe' : process.env.SHELL || '/bin/zsh'
    } else {
      shell = process.platform === 'win32' ? 'powershell.exe' : process.env.SHELL || '/bin/zsh'
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
      createdAt: Date.now(),
      lastActivity: Date.now(),
      sandbox: config.sandbox,
      attachedClients: new Set()
    }

    ptyProcess.onData((data) => {
      session.lastActivity = Date.now()
      this.appendScrollback(session, data)
      this.broadcastData(id, data)
    })

    ptyProcess.onExit(({ exitCode, signal }) => {
      this.broadcastExit(id, exitCode, signal)
      console.log(`[daemon] session ${id} exited with code ${exitCode} (worktree: ${cwd})`)
      console.log(`[daemon] worktree removed: ${cwd} <- session ${id}`)
      this.sessions.delete(id)
    })

    this.sessions.set(id, session)

    // Execute startup command if provided
    if (config.startupCommand && config.startupCommand.trim()) {
      setTimeout(() => {
        ptyProcess.write(config.startupCommand!.trim() + '\n')
      }, 100)
    }

    console.log(`[daemon] created session ${id} (cwd: ${cwd}, sandbox: ${isSandboxed})`)
    console.log(`[daemon] worktree added: ${cwd} -> session ${id}`)
    return id
  }

  attach(sessionId: string, clientId: string): { scrollback: string[]; session: SessionInfo } {
    const session = this.sessions.get(sessionId)
    if (!session) {
      throw new Error(`Session ${sessionId} not found`)
    }

    session.attachedClients.add(clientId)
    session.lastActivity = Date.now()

    console.log(
      `[daemon] client ${clientId} attached to session ${sessionId} (${session.attachedClients.size} clients)`
    )

    return {
      scrollback: [...session.scrollback],
      session: this.getSessionInfo(session)
    }
  }

  detach(sessionId: string, clientId: string): void {
    const session = this.sessions.get(sessionId)
    if (!session) {
      return
    }

    session.attachedClients.delete(clientId)
    console.log(
      `[daemon] client ${clientId} detached from session ${sessionId} (${session.attachedClients.size} clients remaining)`
    )
  }

  write(sessionId: string, data: string): void {
    const session = this.sessions.get(sessionId)
    if (!session) {
      throw new Error(`Session ${sessionId} not found`)
    }

    session.pty.write(data)
    session.lastActivity = Date.now()
  }

  resize(sessionId: string, cols: number, rows: number): void {
    const session = this.sessions.get(sessionId)
    if (!session) {
      throw new Error(`Session ${sessionId} not found`)
    }

    session.pty.resize(cols, rows)
    session.cols = cols
    session.rows = rows
    session.lastActivity = Date.now()
  }

  kill(sessionId: string): void {
    const session = this.sessions.get(sessionId)
    if (!session) {
      return
    }

    console.log(`[daemon] killing session ${sessionId} (worktree: ${session.cwd})`)
    console.log(`[daemon] worktree removed: ${session.cwd} <- session ${sessionId}`)
    session.pty.kill()
    this.sessions.delete(sessionId)
  }

  listSessions(): SessionInfo[] {
    const sessions = Array.from(this.sessions.values()).map((session) => this.getSessionInfo(session))
    console.log(`[daemon] listSessions called - returning ${sessions.length} sessions`)
    sessions.forEach((session) => {
      console.log(`  - ${session.id}: worktree=${session.cwd}`)
    })
    return sessions
  }

  getScrollback(sessionId: string): string[] {
    const session = this.sessions.get(sessionId)
    if (!session) {
      throw new Error(`Session ${sessionId} not found`)
    }

    return [...session.scrollback]
  }

  onData(callback: DataCallback): () => void {
    this.dataCallbacks.add(callback)
    return () => this.dataCallbacks.delete(callback)
  }

  onExit(callback: ExitCallback): () => void {
    this.exitCallbacks.add(callback)
    return () => this.exitCallbacks.delete(callback)
  }

  private appendScrollback(session: PtySession, data: string): void {
    session.scrollback.push(data)

    // Truncate if exceeds limit
    if (session.scrollback.length > session.scrollbackLimit) {
      const excess = session.scrollback.length - session.scrollbackLimit
      session.scrollback.splice(0, excess)
    }
  }

  private broadcastData(sessionId: string, data: string): void {
    for (const callback of this.dataCallbacks) {
      callback(sessionId, data)
    }
  }

  private broadcastExit(sessionId: string, exitCode: number, signal?: number): void {
    for (const callback of this.exitCallbacks) {
      callback(sessionId, exitCode, signal)
    }
  }

  private getSessionInfo(session: PtySession): SessionInfo {
    return {
      id: session.id,
      cwd: session.cwd,
      cols: session.cols,
      rows: session.rows,
      createdAt: session.createdAt,
      lastActivity: session.lastActivity,
      attachedClients: session.attachedClients.size
    }
  }

  private startOrphanCleanup(): void {
    // Run cleanup check every minute
    this.orphanCleanupInterval = setInterval(() => {
      this.cleanupOrphanedSessions()
    }, 60000)
  }

  private cleanupOrphanedSessions(): void {
    if (this.orphanTimeout === 0) return

    const now = Date.now()
    const timeoutMs = this.orphanTimeout * 60 * 1000 // Convert minutes to ms

    for (const [id, session] of this.sessions) {
      const isOrphan = session.attachedClients.size === 0
      const idleTime = now - session.lastActivity

      if (isOrphan && idleTime > timeoutMs) {
        console.log(
          `[daemon] cleaning up orphaned session ${id} (idle for ${Math.round(idleTime / 1000)}s)`
        )
        this.kill(id)
      }
    }
  }

  shutdown(): void {
    console.log('[daemon] shutting down PTY manager')
    if (this.orphanCleanupInterval) {
      clearInterval(this.orphanCleanupInterval)
    }

    // Kill all sessions
    for (const [id] of this.sessions) {
      this.kill(id)
    }
  }
}
