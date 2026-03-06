import * as pty from 'node-pty'
import { BrowserWindow } from 'electron'
import * as path from 'path'
import * as os from 'os'

export interface SandboxConfig {
  enabled: boolean
  allowNetwork: boolean
  allowedPaths: string[]
}

interface PtyInstance {
  pty: pty.IPty
  id: string
  sandboxed: boolean
}

class PtyManager {
  private ptys: Map<string, PtyInstance> = new Map()
  private counter = 0

  create(cwd: string, window: BrowserWindow, sandbox?: SandboxConfig): string {
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
    } else if (isSandboxed) {
      // On other platforms, just set restricted env vars as a soft sandbox
      shell = process.platform === 'win32' ? 'powershell.exe' : process.env.SHELL || '/bin/zsh'
      env.TREETERM_SANDBOXED = '1'
      env.PS1 = '[SANDBOX] ' + (env.PS1 || '\\$ ')
      // Restrict HOME to workspace to limit access
      env.HOME = cwd
      env.TMPDIR = path.join(cwd, '.treeterm-tmp')
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
      window.webContents.send('pty:data', id, data)
    })

    ptyProcess.onExit(({ exitCode }) => {
      window.webContents.send('pty:exit', id, exitCode)
      this.ptys.delete(id)
    })

    this.ptys.set(id, { pty: ptyProcess, id, sandboxed: isSandboxed })
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
    }
  }

  resize(id: string, cols: number, rows: number): void {
    const instance = this.ptys.get(id)
    if (instance) {
      instance.pty.resize(cols, rows)
    }
  }

  kill(id: string): void {
    const instance = this.ptys.get(id)
    if (instance) {
      instance.pty.kill()
      this.ptys.delete(id)
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
}

export const ptyManager = new PtyManager()
