import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import * as pty from 'node-pty'
import { execSync } from 'child_process'

const mockPtyProcess = {
  write: vi.fn(),
  resize: vi.fn(),
  kill: vi.fn(),
  onData: vi.fn(),
  onExit: vi.fn(),
  pid: 1234
}

vi.mock('node-pty', () => ({
  spawn: vi.fn(() => mockPtyProcess)
}))

vi.mock('./logger', () => ({
  createModuleLogger: () => ({
    info: vi.fn(),
    debug: vi.fn(),
    warn: vi.fn(),
    error: vi.fn()
  })
}))

vi.mock('fs', () => ({
  existsSync: vi.fn(() => true),
  statSync: vi.fn(() => ({ isFile: () => false })),
  readFileSync: vi.fn()
}))

vi.mock('child_process', () => ({
  execSync: vi.fn(() => '/usr/bin/bwrap')
}))

import { DaemonPtyManager } from './ptyManager'

describe('DaemonPtyManager', () => {
  let manager: DaemonPtyManager

  beforeEach(() => {
    vi.clearAllMocks()
    manager = new DaemonPtyManager(1024 * 1024)
  })

  afterEach(() => {
    manager.shutdown()
  })

  describe('create', () => {
    it('creates PTY session with default shell', () => {
      const sessionId = manager.create({
        cwd: '/home/user',
        env: {},
        cols: 80,
        rows: 24
      })

      expect(sessionId).toMatch(/^pty-\d+$/)
    })

    it('creates PTY with startup command', () => {
      vi.useFakeTimers()
      
      const sessionId = manager.create({
        cwd: '/home/user',
        env: {},
        cols: 80,
        rows: 24,
        startupCommand: 'ls -la'
      })

      vi.advanceTimersByTime(150)
      
      expect(mockPtyProcess.write).toHaveBeenCalled()
      
      vi.useRealTimers()
    })

    it('assigns sequential IDs', () => {
      const id1 = manager.create({ cwd: '/tmp', env: {} })
      const id2 = manager.create({ cwd: '/tmp', env: {} })
      
      expect(id1).toBe('pty-1')
      expect(id2).toBe('pty-2')
    })
  })

  describe('attach', () => {
    it('attaches to existing session', () => {
      const sessionId = manager.create({ cwd: '/tmp', env: {} })

      const result = manager.attach(sessionId)

      expect(result.scrollback).toEqual([])
      expect(result.session.id).toBe(sessionId)
    })

    it('throws error for non-existent session', () => {
      expect(() => manager.attach('pty-nonexistent')).toThrow('Session pty-nonexistent not found')
    })
  })

  describe('write', () => {
    it('writes data to PTY', () => {
      const sessionId = manager.create({ cwd: '/tmp', env: {} })
      
      manager.write(sessionId, 'hello')
      
      expect(mockPtyProcess.write).toHaveBeenCalledWith('hello')
    })

    it('throws error for non-existent session', () => {
      expect(() => manager.write('pty-nonexistent', 'data')).toThrow('Session pty-nonexistent not found')
    })
  })

  describe('resize', () => {
    it('resizes PTY', () => {
      const sessionId = manager.create({ cwd: '/tmp', env: {} })
      
      manager.resize(sessionId, 100, 50)
      
      expect(mockPtyProcess.resize).toHaveBeenCalledWith(100, 50)
    })

    it('throws error for non-existent session', () => {
      expect(() => manager.resize('pty-nonexistent', 80, 24)).toThrow('Session pty-nonexistent not found')
    })
  })

  describe('kill', () => {
    it('kills PTY session', () => {
      const sessionId = manager.create({ cwd: '/tmp', env: {} })
      
      manager.kill(sessionId)
      
      expect(mockPtyProcess.kill).toHaveBeenCalled()
    })

    it('handles kill for non-existent session gracefully', () => {
      expect(() => manager.kill('pty-nonexistent')).not.toThrow()
    })
  })

  describe('listSessions', () => {
    it('returns empty array when no sessions', () => {
      const sessions = manager.listSessions()
      expect(sessions).toEqual([])
    })

    it('returns session info for active sessions', () => {
      const sessionId = manager.create({ cwd: '/tmp', env: {}, cols: 80, rows: 24 })
      
      const sessions = manager.listSessions()
      
      expect(sessions).toHaveLength(1)
      expect(sessions[0].id).toBe(sessionId)
      expect(sessions[0].cwd).toBe('/tmp')
      expect(sessions[0].cols).toBe(80)
      expect(sessions[0].rows).toBe(24)
    })
  })

  describe('onData callback', () => {
    it('registers and receives data callbacks', () => {
      const callback = vi.fn()
      const unsubscribe = manager.onData(callback)
      
      const sessionId = manager.create({ cwd: '/tmp', env: {} })
      
      // Simulate data from PTY
      const onDataHandler = mockPtyProcess.onData.mock.calls[0][0]
      onDataHandler('test data')
      
      expect(callback).toHaveBeenCalledWith(sessionId, 'test data')
      
      unsubscribe()
    })
  })

  describe('onExit callback', () => {
    it('registers and receives exit callbacks', () => {
      const callback = vi.fn()
      const unsubscribe = manager.onExit(callback)
      
      const sessionId = manager.create({ cwd: '/tmp', env: {} })
      
      // Simulate exit from PTY
      const onExitHandler = mockPtyProcess.onExit.mock.calls[0][0]
      onExitHandler({ exitCode: 0, signal: undefined })
      
      expect(callback).toHaveBeenCalledWith(sessionId, 0, undefined)
      
      unsubscribe()
    })
  })

  describe('shutdown', () => {
    it('kills all sessions on shutdown', () => {
      manager.create({ cwd: '/tmp/1', env: {} })
      manager.create({ cwd: '/tmp/2', env: {} })
      
      manager.shutdown()
      
      expect(mockPtyProcess.kill).toHaveBeenCalledTimes(2)
    })

  })

  describe('scrollback management', () => {
    it('accumulates scrollback data', () => {
      const callback = vi.fn()
      manager.onData(callback)

      const sessionId = manager.create({ cwd: '/tmp', env: {} })

      const onDataHandler = mockPtyProcess.onData.mock.calls[0][0]
      onDataHandler('line 1')
      onDataHandler('line 2')

      const { scrollback } = manager.attach(sessionId)
      expect(scrollback).toEqual(['line 1', 'line 2'])
    })

    it('returns copy of scrollback, not reference', () => {
      const sessionId = manager.create({ cwd: '/tmp', env: {} })

      const { scrollback: scrollback1 } = manager.attach(sessionId)
      const { scrollback: scrollback2 } = manager.attach(sessionId)

      expect(scrollback1).not.toBe(scrollback2)
    })

    it('truncates scrollback at byte limit', () => {
      // Create manager with small scrollback limit (100 bytes)
      const smallManager = new DaemonPtyManager(100)

      const sessionId = smallManager.create({ cwd: '/tmp', env: {} })

      const onDataHandler = mockPtyProcess.onData.mock.calls[0][0]
      // Push data exceeding 100 bytes
      onDataHandler('a'.repeat(60))
      onDataHandler('b'.repeat(60))

      const { scrollback } = smallManager.attach(sessionId)
      // First chunk should have been removed since total exceeds 100 bytes
      expect(scrollback).not.toContain('a'.repeat(60))
      expect(scrollback).toContain('b'.repeat(60))

      smallManager.shutdown()
    })
  })

  describe('environment variable merging', () => {
    it('merges config.env with process.env', () => {
      manager.create({ cwd: '/tmp', env: { CUSTOM_VAR: 'hello' } })

      const spawnCall = vi.mocked(pty.spawn).mock.calls[0]
      const envArg = spawnCall[2]?.env
      expect(envArg!.CUSTOM_VAR).toBe('hello')
      // process.env vars should also be present
      expect(envArg!.PATH).toBeDefined()
    })
  })

  describe('multiple callbacks', () => {
    it('multiple onData callbacks all fire', () => {
      const cb1 = vi.fn()
      const cb2 = vi.fn()
      manager.onData(cb1)
      manager.onData(cb2)

      const sessionId = manager.create({ cwd: '/tmp', env: {} })

      const onDataHandler = mockPtyProcess.onData.mock.calls[0][0]
      onDataHandler('hello')

      expect(cb1).toHaveBeenCalledWith(sessionId, 'hello')
      expect(cb2).toHaveBeenCalledWith(sessionId, 'hello')
    })

    it('onExit callback receives exit code', () => {
      const cb = vi.fn()
      manager.onExit(cb)

      const sessionId = manager.create({ cwd: '/tmp', env: {} })

      const onExitHandler = mockPtyProcess.onExit.mock.calls[0][0]
      onExitHandler({ exitCode: 42, signal: 15 })

      expect(cb).toHaveBeenCalledWith(sessionId, 42, 15)
    })
  })

  describe('activity tracking', () => {
    it('updates lastActivity on data', () => {
      const sessionId = manager.create({ cwd: '/tmp', env: {} })

      const sessionsBefore = manager.listSessions()
      const activityBefore = sessionsBefore[0].lastActivity

      // Small delay then trigger data
      const onDataHandler = mockPtyProcess.onData.mock.calls[0][0]
      onDataHandler('output')

      const sessionsAfter = manager.listSessions()
      expect(sessionsAfter[0].lastActivity).toBeGreaterThanOrEqual(activityBefore)
    })
  })

  describe('platform-specific shell defaults', () => {
    it('uses SHELL env var for unix platforms', () => {
      const originalShell = process.env.SHELL
      process.env.SHELL = '/bin/fish'

      manager.create({ cwd: '/tmp', env: {} })

      const spawnCall = vi.mocked(pty.spawn).mock.calls[0]
      expect(spawnCall[0]).toBe('/bin/fish')

      if (originalShell) {
        process.env.SHELL = originalShell
      }
    })
  })

  describe('sandbox paths', () => {
    it('macOS sandbox uses sandbox-exec', () => {
      const originalPlatform = process.platform
      Object.defineProperty(process, 'platform', { value: 'darwin', configurable: true })

      const sandboxManager = new DaemonPtyManager(1024 * 1024)

      sandboxManager.create({
        cwd: '/workspace',
        env: {},
        sandbox: { enabled: true, allowNetwork: false, allowedPaths: [] }
      })

      const spawnCall = vi.mocked(pty.spawn).mock.calls[0]
      expect(spawnCall[0]).toBe('/usr/bin/sandbox-exec')

      sandboxManager.shutdown()
      Object.defineProperty(process, 'platform', { value: originalPlatform, configurable: true })
    })

    it('Linux with bwrap uses bwrap', () => {
      const originalPlatform = process.platform
      Object.defineProperty(process, 'platform', { value: 'linux', configurable: true })

      const sandboxManager = new DaemonPtyManager(1024 * 1024)

      sandboxManager.create({
        cwd: '/workspace',
        env: {},
        sandbox: { enabled: true, allowNetwork: false, allowedPaths: [] }
      })

      const spawnCall = vi.mocked(pty.spawn).mock.calls[0]
      expect(spawnCall[0]).toBe('bwrap')
      expect(spawnCall[1]).toContain('--die-with-parent')

      sandboxManager.shutdown()
      Object.defineProperty(process, 'platform', { value: originalPlatform, configurable: true })
    })

    it('Linux without bwrap falls back to default shell', () => {
      const originalPlatform = process.platform
      Object.defineProperty(process, 'platform', { value: 'linux', configurable: true })

      vi.mocked(execSync).mockImplementation(() => { throw new Error('not found') })

      const sandboxManager = new DaemonPtyManager(1024 * 1024)

      sandboxManager.create({
        cwd: '/workspace',
        env: {},
        sandbox: { enabled: true, allowNetwork: false, allowedPaths: [] }
      })

      const spawnCall = vi.mocked(pty.spawn).mock.calls[0]
      expect(spawnCall[0]).not.toBe('bwrap')

      sandboxManager.shutdown()
      Object.defineProperty(process, 'platform', { value: originalPlatform, configurable: true })
    })
  })
})
