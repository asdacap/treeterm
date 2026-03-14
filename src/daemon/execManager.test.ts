import { describe, it, expect, vi, beforeEach } from 'vitest'
import { EventEmitter } from 'events'

vi.mock('./logger', () => ({
  createModuleLogger: () => ({
    info: vi.fn(),
    warn: vi.fn(),
    error: vi.fn(),
    debug: vi.fn(),
  }),
}))

// Mock child_process spawn
const mockChild = {
  stdout: new EventEmitter() as any,
  stderr: new EventEmitter() as any,
  stdin: { writable: true, write: vi.fn(), end: vi.fn() },
  kill: vi.fn(),
  on: vi.fn(),
  pid: 1234,
}

vi.mock('child_process', () => ({
  spawn: vi.fn(() => mockChild),
}))

import { spawn } from 'child_process'
import { ExecManager } from './execManager'

function makeChild(overrides: Partial<typeof mockChild> = {}) {
  const child = {
    stdout: new EventEmitter() as any,
    stderr: new EventEmitter() as any,
    stdin: { writable: true, write: vi.fn(), end: vi.fn() },
    kill: vi.fn(),
    on: vi.fn(),
    pid: 1234,
    ...overrides,
  }
  vi.mocked(spawn).mockReturnValue(child as any)
  return child
}

describe('ExecManager', () => {
  let manager: ExecManager

  beforeEach(() => {
    vi.clearAllMocks()
    manager = new ExecManager()
  })

  describe('start', () => {
    it('spawns process with correct args', () => {
      const child = makeChild()
      const onExit = vi.fn()

      manager.start('exec-1', {
        cwd: '/workspace',
        command: 'git',
        args: ['status'],
        env: { GIT_PAGER: 'cat' },
      }, {
        onStdout: vi.fn(),
        onStderr: vi.fn(),
        onExit,
      })

      expect(spawn).toHaveBeenCalledWith('git', ['status'], expect.objectContaining({
        cwd: '/workspace',
        stdio: ['pipe', 'pipe', 'pipe'],
      }))
    })

    it('merges env with process.env', () => {
      const child = makeChild()
      manager.start('exec-2', {
        cwd: '/workspace',
        command: 'echo',
        args: ['hello'],
        env: { CUSTOM_VAR: 'value' },
      }, { onStdout: vi.fn(), onStderr: vi.fn(), onExit: vi.fn() })

      const spawnCall = vi.mocked(spawn).mock.calls[0]
      const spawnEnv = (spawnCall[2] as any).env
      expect(spawnEnv.CUSTOM_VAR).toBe('value')
    })

    it('calls onStdout when stdout emits data', () => {
      const child = makeChild()
      const onStdout = vi.fn()

      manager.start('exec-3', { cwd: '/', command: 'ls', args: [] }, {
        onStdout,
        onStderr: vi.fn(),
        onExit: vi.fn(),
      })

      const buf = Buffer.from('output data')
      child.stdout.emit('data', buf)
      expect(onStdout).toHaveBeenCalledWith(buf)
    })

    it('calls onStderr when stderr emits data', () => {
      const child = makeChild()
      const onStderr = vi.fn()

      manager.start('exec-4', { cwd: '/', command: 'ls', args: [] }, {
        onStdout: vi.fn(),
        onStderr,
        onExit: vi.fn(),
      })

      const buf = Buffer.from('error output')
      child.stderr.emit('data', buf)
      expect(onStderr).toHaveBeenCalledWith(buf)
    })

    it('calls onExit on close event', () => {
      const child = makeChild()
      // Capture the 'close' handler registered via child.on
      let closeHandler: Function | null = null
      child.on.mockImplementation((event: string, handler: Function) => {
        if (event === 'close') closeHandler = handler
      })

      const onExit = vi.fn()
      manager.start('exec-5', { cwd: '/', command: 'ls', args: [] }, {
        onStdout: vi.fn(),
        onStderr: vi.fn(),
        onExit,
      })

      expect(closeHandler).not.toBeNull()
      closeHandler!(0, null)
      expect(onExit).toHaveBeenCalledWith(0, null)
    })

    it('calls onExit with error on spawn error event', () => {
      const child = makeChild()
      let errorHandler: Function | null = null
      child.on.mockImplementation((event: string, handler: Function) => {
        if (event === 'error') errorHandler = handler
      })

      const onExit = vi.fn()
      manager.start('exec-6', { cwd: '/', command: 'bad', args: [] }, {
        onStdout: vi.fn(),
        onStderr: vi.fn(),
        onExit,
      })

      const err = new Error('ENOENT')
      errorHandler!(err)
      expect(onExit).toHaveBeenCalledWith(null, null, err)
    })

    it('calls onExit with error when spawn throws', () => {
      vi.mocked(spawn).mockImplementation(() => { throw new Error('spawn failed') })
      const onExit = vi.fn()

      manager.start('exec-7', { cwd: '/', command: 'bad', args: [] }, {
        onStdout: vi.fn(),
        onStderr: vi.fn(),
        onExit,
      })

      expect(onExit).toHaveBeenCalledWith(null, null, expect.any(Error))
    })
  })

  describe('writeStdin', () => {
    it('does nothing for non-existent exec', () => {
      // Should not throw
      expect(() => manager.writeStdin('nope', Buffer.from('data'))).not.toThrow()
    })

    it('writes to stdin when writable', () => {
      const child = makeChild()
      manager.start('exec-8', { cwd: '/', command: 'cat', args: [] }, {
        onStdout: vi.fn(), onStderr: vi.fn(), onExit: vi.fn(),
      })

      manager.writeStdin('exec-8', Buffer.from('input'))
      expect(child.stdin.write).toHaveBeenCalledWith(Buffer.from('input'))
    })

    it('does not write when stdin is not writable', () => {
      const child = makeChild({
        stdin: { writable: false, write: vi.fn(), end: vi.fn() }
      })
      manager.start('exec-9', { cwd: '/', command: 'cat', args: [] }, {
        onStdout: vi.fn(), onStderr: vi.fn(), onExit: vi.fn(),
      })

      manager.writeStdin('exec-9', Buffer.from('data'))
      expect(child.stdin.write).not.toHaveBeenCalled()
    })
  })

  describe('closeStdin', () => {
    it('does nothing for non-existent exec', () => {
      expect(() => manager.closeStdin('nope')).not.toThrow()
    })

    it('calls stdin.end for existing exec', () => {
      const child = makeChild()
      manager.start('exec-10', { cwd: '/', command: 'cat', args: [] }, {
        onStdout: vi.fn(), onStderr: vi.fn(), onExit: vi.fn(),
      })

      manager.closeStdin('exec-10')
      expect(child.stdin.end).toHaveBeenCalled()
    })
  })

  describe('kill', () => {
    it('does nothing for non-existent exec', () => {
      expect(() => manager.kill('nope', 15)).not.toThrow()
    })

    it('maps signal 9 to SIGKILL', () => {
      const child = makeChild()
      manager.start('exec-11', { cwd: '/', command: 'sleep', args: ['100'] }, {
        onStdout: vi.fn(), onStderr: vi.fn(), onExit: vi.fn(),
      })

      manager.kill('exec-11', 9)
      expect(child.kill).toHaveBeenCalledWith('SIGKILL')
    })

    it('maps signal 15 to SIGTERM', () => {
      const child = makeChild()
      manager.start('exec-12', { cwd: '/', command: 'sleep', args: ['100'] }, {
        onStdout: vi.fn(), onStderr: vi.fn(), onExit: vi.fn(),
      })

      manager.kill('exec-12', 15)
      expect(child.kill).toHaveBeenCalledWith('SIGTERM')
    })

    it('maps signal 2 to SIGINT', () => {
      const child = makeChild()
      manager.start('exec-13', { cwd: '/', command: 'sleep', args: ['100'] }, {
        onStdout: vi.fn(), onStderr: vi.fn(), onExit: vi.fn(),
      })

      manager.kill('exec-13', 2)
      expect(child.kill).toHaveBeenCalledWith('SIGINT')
    })

    it('maps signal 1 to SIGHUP', () => {
      const child = makeChild()
      manager.start('exec-14', { cwd: '/', command: 'sleep', args: ['100'] }, {
        onStdout: vi.fn(), onStderr: vi.fn(), onExit: vi.fn(),
      })

      manager.kill('exec-14', 1)
      expect(child.kill).toHaveBeenCalledWith('SIGHUP')
    })

    it('maps unknown signal to SIGTERM', () => {
      const child = makeChild()
      manager.start('exec-15', { cwd: '/', command: 'sleep', args: ['100'] }, {
        onStdout: vi.fn(), onStderr: vi.fn(), onExit: vi.fn(),
      })

      manager.kill('exec-15', 99)
      expect(child.kill).toHaveBeenCalledWith('SIGTERM')
    })
  })

  describe('shutdown', () => {
    it('kills all running processes', () => {
      const child1 = makeChild()
      manager.start('exec-16', { cwd: '/', command: 'sleep', args: ['100'] }, {
        onStdout: vi.fn(), onStderr: vi.fn(), onExit: vi.fn(),
      })

      const child2 = makeChild()
      manager.start('exec-17', { cwd: '/', command: 'sleep', args: ['200'] }, {
        onStdout: vi.fn(), onStderr: vi.fn(), onExit: vi.fn(),
      })

      manager.shutdown()

      expect(child1.kill).toHaveBeenCalledWith('SIGTERM')
      expect(child2.kill).toHaveBeenCalledWith('SIGTERM')
    })

    it('clears the process map on shutdown', () => {
      const child = makeChild()
      manager.start('exec-18', { cwd: '/', command: 'sleep', args: ['100'] }, {
        onStdout: vi.fn(), onStderr: vi.fn(), onExit: vi.fn(),
      })

      manager.shutdown()

      // After shutdown, kill on non-existent exec should not throw
      expect(() => manager.kill('exec-18', 15)).not.toThrow()
    })
  })
})
