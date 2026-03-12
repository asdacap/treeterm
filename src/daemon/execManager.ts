/**
 * Exec Manager - Execute one-shot shell commands with streaming I/O
 * 
 * This module provides a low-level primitive for executing shell commands
 * outside of PTY sessions. Used by Main process for git operations and
 * other command-line workflows.
 */

import { spawn, ChildProcess } from 'child_process'
import { createModuleLogger } from './logger'

const log = createModuleLogger('execManager')

export interface ExecOptions {
  cwd: string
  command: string
  args: string[]
  env?: Record<string, string>
  timeoutMs?: number
}

export interface ExecHandlers {
  onStdout: (data: Buffer) => void
  onStderr: (data: Buffer) => void
  onExit: (code: number | null, signal: string | null, error?: Error) => void
}

export class ExecManager {
  private processes = new Map<string, ChildProcess>()
  private idCounter = 0

  /**
   * Start a new command execution
   */
  start(id: string, options: ExecOptions, handlers: ExecHandlers): void {
    log.debug({ id, options }, 'starting exec')

    // Merge environment variables
    const env = { ...process.env, ...options.env }
    
    try {
      // Spawn the process
      const child = spawn(options.command, options.args, {
        cwd: options.cwd,
        env,
        stdio: ['pipe', 'pipe', 'pipe'],
      })

      this.processes.set(id, child)

      // Handle stdout data
      child.stdout?.on('data', (data: Buffer) => {
        handlers.onStdout(data)
      })

      // Handle stderr data
      child.stderr?.on('data', (data: Buffer) => {
        handlers.onStderr(data)
      })

      // Handle process exit
      child.on('close', (code, signal) => {
        log.debug({ id, code, signal }, 'exec exited')
        this.processes.delete(id)
        handlers.onExit(code, signal)
      })

      // Handle spawn errors
      child.on('error', (error) => {
        log.error({ id, error }, 'exec spawn error')
        this.processes.delete(id)
        handlers.onExit(null, null, error)
      })

      // Set up timeout if specified
      if (options.timeoutMs && options.timeoutMs > 0) {
        setTimeout(() => {
          if (this.processes.has(id)) {
            log.warn({ id, timeoutMs: options.timeoutMs }, 'exec timeout, sending SIGTERM')
            child.kill('SIGTERM')
            
            // Force kill after grace period if still running
            setTimeout(() => {
              if (this.processes.has(id)) {
                log.warn({ id }, 'exec did not terminate, sending SIGKILL')
                child.kill('SIGKILL')
              }
            }, 5000)
          }
        }, options.timeoutMs)
      }
    } catch (error) {
      log.error({ id, error }, 'failed to spawn exec')
      handlers.onExit(null, null, error instanceof Error ? error : new Error(String(error)))
    }
  }

  /**
   * Write data to stdin of the process
   */
  writeStdin(id: string, data: Buffer): void {
    const child = this.processes.get(id)
    if (!child) {
      log.warn({ id }, 'attempted to write stdin to non-existent exec')
      return
    }
    
    if (child.stdin?.writable) {
      child.stdin.write(data)
    } else {
      log.warn({ id }, 'stdin not writable')
    }
  }

  /**
   * Close the stdin stream (send EOF)
   */
  closeStdin(id: string): void {
    const child = this.processes.get(id)
    if (!child) {
      return
    }
    
    child.stdin?.end()
  }

  /**
   * Send a signal to the process
   */
  kill(id: string, signal: number): void {
    const child = this.processes.get(id)
    if (!child) {
      log.warn({ id }, 'attempted to kill non-existent exec')
      return
    }

    // Map numeric signal to string
    const sig = signal === 9 ? 'SIGKILL' 
                : signal === 15 ? 'SIGTERM' 
                : signal === 2 ? 'SIGINT'
                : signal === 1 ? 'SIGHUP'
                : 'SIGTERM'
    
    log.debug({ id, signal: sig }, 'sending signal to exec')
    child.kill(sig)
  }

  /**
   * Clean up all running processes
   */
  shutdown(): void {
    log.info({ count: this.processes.size }, 'shutting down exec manager')
    
    for (const [id, child] of this.processes) {
      log.debug({ id }, 'killing exec process')
      child.kill('SIGTERM')
    }
    
    this.processes.clear()
  }
}

// Export singleton instance
export const execManager = new ExecManager()
