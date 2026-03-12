import * as pty from 'node-pty'
import { BrowserWindow } from 'electron'
import * as path from 'path'
import * as os from 'os'
import * as fs from 'fs'
import { execSync } from 'child_process'
import type { SandboxConfig } from '../shared/types'

// Re-export for backward compatibility
export type { SandboxConfig }

interface PtyInstance {
  pty: pty.IPty
  id: string
  sandboxed: boolean
}

function isBwrapAvailable(): boolean {
  try {
    execSync('which bwrap', { stdio: 'ignore' })
    return true
  } catch (error) {
    console.log('[pty] bwrap check failed:', error)
    return false
  }
}

function getGitRoot(workspacePath: string): string | null {
  const gitPath = path.join(workspacePath, '.git')
  const stat = fs.statSync(gitPath, { throwIfNoEntry: false })
  if (!stat) return null

  if (stat.isFile()) {
    // Worktree: parse "gitdir: <path>" to find main .git
    const content = fs.readFileSync(gitPath, 'utf-8')
    const match = content.match(/^gitdir:\s*(.+)$/m)
    if (match) {
      const worktreeGitDir = path.resolve(workspacePath, match[1].trim())
      // Main .git is at ../../ from worktree gitdir
      return path.resolve(worktreeGitDir, '../../')
    }
  }
  return null // Regular repo, .git is inside workspace
}

function generateBwrapArgs(cwd: string, sandbox: SandboxConfig): string[] {
  const args: string[] = ['--die-with-parent', '--unshare-pid', '--unshare-uts', '--unshare-ipc']

  // Network
  if (!sandbox.allowNetwork) {
    args.push('--unshare-net')
  }

  // System directories (read-only)
  const roBinds = ['/usr', '/bin', '/lib', '/lib64', '/etc', '/opt']
  for (const p of roBinds) {
    if (fs.existsSync(p)) {
      args.push('--ro-bind', p, p)
    }
  }

  // Required mounts
  args.push('--proc', '/proc')
  args.push('--dev', '/dev')
  args.push('--tmpfs', '/tmp')

  // Workspace (read-write)
  args.push('--bind', cwd, cwd)
  args.push('--chdir', cwd)

  // Git worktree support: add parent .git as read-only
  const gitRoot = getGitRoot(cwd)
  if (gitRoot) {
    args.push('--ro-bind', gitRoot, gitRoot)
  }

  // Home directory essentials (read-only)
  const home = os.homedir()
  const homeFiles = ['.bashrc', '.zshrc', '.profile', '.gitconfig']
  for (const f of homeFiles) {
    const p = path.join(home, f)
    if (fs.existsSync(p)) {
      args.push('--ro-bind', p, p)
    }
  }

  // Additional allowed paths (read-only)
  for (const p of sandbox.allowedPaths) {
    if (fs.existsSync(p)) {
      args.push('--ro-bind', p, p)
    }
  }

  return args
}

class PtyManager {
  private ptys: Map<string, PtyInstance> = new Map()
  private counter = 0

  create(cwd: string, window: BrowserWindow, sandbox?: SandboxConfig, startupCommand?: string): string {
    const id = `pty-${++this.counter}`
    const isSandboxed = sandbox?.enabled ?? false

    let shell: string
    let args: string[] = []
    let env = { ...process.env } as { [key: string]: string }

    if (isSandboxed && process.platform === 'darwin') {
      // On macOS, use sandbox-exec with a restrictive profile
      shell = '/usr/bin/sandbox-exec'
      const profile = this.generateSandboxProfile(cwd, sandbox!)
      args = ['-p', profile, process.env.SHELL || '/bin/zsh']

      // Add sandbox indicator to prompt
      env.TREETERM_SANDBOXED = '1'
      env.PS1 = '[SANDBOX] ' + (env.PS1 || '\\$ ')
    } else if (isSandboxed && process.platform === 'linux') {
      if (isBwrapAvailable()) {
        // Use bubblewrap for real sandboxing
        const bwrapArgs = generateBwrapArgs(cwd, sandbox!)
        shell = 'bwrap'
        args = [...bwrapArgs, '--', process.env.SHELL || '/bin/bash']
        env.TREETERM_SANDBOXED = '1'
        env.PS1 = '[SANDBOX] ' + (env.PS1 || '\\$ ')
      } else {
        // No sandbox available - warn and run unsandboxed
        console.warn('[sandbox] bwrap not found, sandbox not available on this system')
        shell = process.env.SHELL || '/bin/bash'
        // Do NOT set TREETERM_SANDBOXED - not actually sandboxed
      }
    } else if (isSandboxed) {
      // Windows or other platforms: no sandbox support, run unsandboxed
      console.warn('[sandbox] sandbox not available on this platform')
      shell = process.platform === 'win32' ? 'powershell.exe' : process.env.SHELL || '/bin/zsh'
    } else {
      shell = process.platform === 'win32' ? 'powershell.exe' : process.env.SHELL || '/bin/zsh'
    }

    const ptyProcess = pty.spawn(shell, args, {
      name: 'xterm-256color',
      cols: 80,
      rows: 24,
      cwd,
      env
    })

    ptyProcess.onData((data) => {
      if (!window.isDestroyed() && !window.webContents.isDestroyed()) {
        window.webContents.send('pty:data', id, data)
      }
    })

    ptyProcess.onExit(({ exitCode }) => {
      if (!window.isDestroyed() && !window.webContents.isDestroyed()) {
        window.webContents.send('pty:exit', id, exitCode)
      }
      this.ptys.delete(id)
    })

    this.ptys.set(id, { pty: ptyProcess, id, sandboxed: isSandboxed })

    // Execute startup command if provided
    if (startupCommand && startupCommand.trim()) {
      // Small delay to ensure shell is ready
      setTimeout(() => {
        // Use 'exec' on Unix platforms to replace the shell process
        // This ensures the PTY exits when the command exits (e.g., AI terminal closes when AI exits)
        const cmd = process.platform === 'win32'
          ? startupCommand.trim()
          : `exec ${startupCommand.trim()}`
        ptyProcess.write(cmd + '\n')
      }, 100)
    }

    return id
  }

  private generateSandboxProfile(workspacePath: string, sandbox: SandboxConfig): string {
    const allowedPaths = [
      workspacePath,
      os.tmpdir(),
      '/usr',
      '/bin',
      '/sbin',
      '/Library/Frameworks',
      '/System',
      '/private/var/folders', // Temp files
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

    const networkRule = sandbox.allowNetwork
      ? '(allow network*)'
      : '(deny network*)'

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

  write(id: string, data: string): void {
    const instance = this.ptys.get(id)
    if (instance) {
      instance.pty.write(data)
    } else {
      console.warn(`[pty] write: PTY ${id} not found`)
    }
  }

  resize(id: string, cols: number, rows: number): void {
    const instance = this.ptys.get(id)
    if (instance) {
      instance.pty.resize(cols, rows)
    } else {
      console.warn(`[pty] resize: PTY ${id} not found`)
    }
  }

  kill(id: string): void {
    const instance = this.ptys.get(id)
    if (instance) {
      instance.pty.kill()
      this.ptys.delete(id)
    } else {
      console.warn(`[pty] kill: PTY ${id} not found`)
    }
  }

  killAll(): void {
    for (const [id] of this.ptys) {
      this.kill(id)
    }
  }

  isSandboxed(id: string): boolean {
    const instance = this.ptys.get(id)
    return instance?.sandboxed ?? false
  }

  isAlive(id: string): boolean {
    return this.ptys.has(id)
  }
}

export const ptyManager = new PtyManager()
