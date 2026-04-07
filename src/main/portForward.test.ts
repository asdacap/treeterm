import { describe, it, expect, vi, beforeEach, type Mock } from 'vitest'
import type { SSHConnectionConfig, PortForwardConfig } from '../shared/types'
import { EventEmitter } from 'events'

// Create a mock child process with event emitter behavior
function createMockProcess() {
  const proc = new EventEmitter() as EventEmitter & {
    stdout: EventEmitter
    stderr: EventEmitter
    kill: Mock
    pid: number
  }
  proc.stdout = new EventEmitter()
  proc.stderr = new EventEmitter()
  proc.kill = vi.fn()
  proc.pid = 1234
  return proc
}

let mockProcess: ReturnType<typeof createMockProcess>

vi.mock('child_process', () => ({
  spawn: vi.fn(() => mockProcess)
}))

import { PortForwardProcess } from './portForward'

const sshConfig: SSHConnectionConfig = {
  id: 'remote-1',
  host: 'example.com',
  user: 'admin',
  port: 22,
  portForwards: [],
}

const pfConfig: PortForwardConfig = {
  id: 'pf-1',
  connectionId: 'remote-1',
  localPort: 8080,
  remoteHost: 'localhost',
  remotePort: 3000,
}

describe('PortForwardProcess', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    mockProcess = createMockProcess()
  })

  describe('constructor and toInfo', () => {
    it('starts with connecting status', () => {
      const pf = new PortForwardProcess(sshConfig, pfConfig)
      const info = pf.toInfo()
      expect(info).toEqual({
        id: 'pf-1',
        connectionId: 'remote-1',
        localPort: 8080,
        remoteHost: 'localhost',
        remotePort: 3000,
        status: 'connecting',
      })
    })

    it('exposes status getter', () => {
      const pf = new PortForwardProcess(sshConfig, pfConfig)
      expect(pf.status).toBe('connecting')
    })
  })

  describe('start', () => {
    it('spawns ssh with correct args', async () => {
      const { spawn } = await import('child_process')
      const pf = new PortForwardProcess(sshConfig, pfConfig)
      pf.start()

      expect(spawn).toHaveBeenCalledWith(
        'ssh',
        expect.arrayContaining([
          '-L', '8080:localhost:3000',
          '-p', '22',
          'admin@example.com',
          'cat',
        ]),
        { stdio: ['pipe', 'pipe', 'pipe'] }
      )
    })

    it('includes identity file when configured', async () => {
      const { spawn } = await import('child_process')
      const configWithKey: SSHConnectionConfig = { ...sshConfig, identityFile: '/home/admin/.ssh/id_rsa' }
      const pf = new PortForwardProcess(configWithKey, pfConfig)
      pf.start()

      // Get the most recent spawn call
      const lastCall = (spawn as Mock).mock.calls.at(-1) as unknown[]
      const args = lastCall[1] as string[]
      expect(args).toContain('-i')
      expect(args).toContain('/home/admin/.ssh/id_rsa')
    })

    it('becomes active after stabilization timeout', () => {
      const pf = new PortForwardProcess(sshConfig, pfConfig)
      pf.start()
      expect(pf.status).toBe('connecting')

      vi.advanceTimersByTime(2000)
      expect(pf.status).toBe('active')
    })

    it('collects stdout output', () => {
      const pf = new PortForwardProcess(sshConfig, pfConfig)
      pf.start()

      mockProcess.stdout.emit('data', Buffer.from('tunnel ready\n'))
      const output = pf.getOutput()
      expect(output).toContain('[portfwd] tunnel ready')
    })

    it('collects stderr output', () => {
      const pf = new PortForwardProcess(sshConfig, pfConfig)
      pf.start()

      mockProcess.stderr.emit('data', Buffer.from('warning: something\n'))
      const output = pf.getOutput()
      expect(output).toContain('[portfwd:err] warning: something')
    })

    it('sets error status on process close with non-zero code', () => {
      const pf = new PortForwardProcess(sshConfig, pfConfig)
      pf.start()

      mockProcess.emit('close', 1)
      expect(pf.status).toBe('error')
      expect(pf.toInfo()).toEqual(expect.objectContaining({
        status: 'error',
        // eslint-disable-next-line @typescript-eslint/no-unsafe-assignment
        error: expect.stringContaining('exited (code 1)'),
      }))
    })

    it('sets error status on process error', () => {
      const pf = new PortForwardProcess(sshConfig, pfConfig)
      pf.start()

      mockProcess.emit('error', new Error('ENOENT'))
      expect(pf.status).toBe('error')
      expect(pf.toInfo()).toEqual(expect.objectContaining({
        status: 'error',
        // eslint-disable-next-line @typescript-eslint/no-unsafe-assignment
        error: expect.stringContaining('ENOENT'),
      }))
    })

    it('does not become active if process closes before stabilization', () => {
      const pf = new PortForwardProcess(sshConfig, pfConfig)
      pf.start()

      mockProcess.emit('close', 255)
      vi.advanceTimersByTime(2000)
      expect(pf.status).toBe('error')
    })
  })

  describe('stop', () => {
    it('kills the process and sets stopped status', () => {
      const pf = new PortForwardProcess(sshConfig, pfConfig)
      pf.start()
      vi.advanceTimersByTime(2000) // make it active

      pf.stop()
      expect(pf.status).toBe('stopped')
      expect(mockProcess.kill).toHaveBeenCalled()
    })

    it('does not transition to error after stop on process close', () => {
      const pf = new PortForwardProcess(sshConfig, pfConfig)
      pf.start()
      pf.stop()

      // Process close fires after kill - should not override stopped
      mockProcess.emit('close', 0)
      expect(pf.status).toBe('stopped')
    })
  })

  describe('output buffer', () => {
    it('trims buffer when exceeding 1000 lines', () => {
      const pf = new PortForwardProcess(sshConfig, pfConfig)
      pf.start() // adds 2 lines

      // Push enough lines to exceed 1000 total, triggering the trim
      for (let i = 0; i < 999; i++) {
        mockProcess.stdout.emit('data', Buffer.from(`line-${String(i)}\n`))
      }
      // Now at 1001 lines, trim fires: slices to last 500
      const output = pf.getOutput()
      expect(output.length).toBe(500)
    })

    it('returns a copy of the buffer', () => {
      const pf = new PortForwardProcess(sshConfig, pfConfig)
      const a = pf.getOutput()
      const b = pf.getOutput()
      expect(a).not.toBe(b)
    })
  })

  describe('listeners', () => {
    it('notifies output listeners', () => {
      const pf = new PortForwardProcess(sshConfig, pfConfig)
      const cb = vi.fn()
      pf.onOutput(cb)
      pf.start()

      mockProcess.stdout.emit('data', Buffer.from('hello\n'))
      expect(cb).toHaveBeenCalledWith('[portfwd] hello')
    })

    it('unsubscribes output listeners', () => {
      const pf = new PortForwardProcess(sshConfig, pfConfig)
      const cb = vi.fn()
      const unsub = pf.onOutput(cb)
      unsub()
      pf.start()

      mockProcess.stdout.emit('data', Buffer.from('hello\n'))
      // Only start() output lines, not 'hello' since we unsubscribed
      const callsWithHello = (cb.mock.calls as string[][]).filter((c) => c[0]!.includes('hello'))
      expect(callsWithHello).toHaveLength(0)
    })

    it('notifies status listeners on status change', () => {
      const pf = new PortForwardProcess(sshConfig, pfConfig)
      const cb = vi.fn()
      pf.onStatusChange(cb)
      pf.start()

      vi.advanceTimersByTime(2000) // connecting -> active
      expect(cb).toHaveBeenCalledWith(expect.objectContaining({ status: 'active' }))
    })

    it('unsubscribes status listeners', () => {
      const pf = new PortForwardProcess(sshConfig, pfConfig)
      const cb = vi.fn()
      const unsub = pf.onStatusChange(cb)
      unsub()
      pf.start()

      vi.advanceTimersByTime(2000)
      expect(cb).not.toHaveBeenCalled()
    })
  })
})
