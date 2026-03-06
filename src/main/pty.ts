import * as pty from 'node-pty'
import { BrowserWindow } from 'electron'

interface PtyInstance {
  pty: pty.IPty
  id: string
}

class PtyManager {
  private ptys: Map<string, PtyInstance> = new Map()
  private counter = 0

  create(cwd: string, window: BrowserWindow): string {
    const id = `pty-${++this.counter}`
    const shell = process.platform === 'win32' ? 'powershell.exe' : process.env.SHELL || '/bin/zsh'

    const ptyProcess = pty.spawn(shell, [], {
      name: 'xterm-256color',
      cols: 80,
      rows: 24,
      cwd,
      env: process.env as { [key: string]: string }
    })

    ptyProcess.onData((data) => {
      window.webContents.send('pty:data', id, data)
    })

    ptyProcess.onExit(({ exitCode }) => {
      window.webContents.send('pty:exit', id, exitCode)
      this.ptys.delete(id)
    })

    this.ptys.set(id, { pty: ptyProcess, id })
    return id
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
}

export const ptyManager = new PtyManager()
