import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import * as pty from 'node-pty'

vi.mock('node-pty', () => ({
  spawn: vi.fn().mockReturnValue({
    onData: vi.fn(),
    onExit: vi.fn(),
    write: vi.fn(),
    resize: vi.fn(),
    kill: vi.fn()
  })
}))

vi.mock('electron', () => ({
  BrowserWindow: vi.fn()
}))

vi.mock('fs', () => ({
  statSync: vi.fn(() => null),
  readFileSync: vi.fn(() => ''),
  existsSync: vi.fn(() => true)
}))

vi.mock('child_process', () => ({
  execSync: vi.fn(() => '/usr/bin/bwrap')
}))

describe('PtyManager', () => {
  let ptyManager: typeof import('./pty').ptyManager

  beforeEach(async () => {
    vi.resetModules()
    const ptyModule = await import('./pty')
    ptyManager = ptyModule.ptyManager
    vi.clearAllMocks()
  })

  describe('isSandboxed', () => {
    it('returns false for non-existent PTY', () => {
      expect(ptyManager.isSandboxed('non-existent')).toBe(false)
    })
  })

  describe('create', () => {
    it('creates a PTY with correct id format', () => {
      const mockWindow = {
        isDestroyed: vi.fn().mockReturnValue(false),
        webContents: {
          isDestroyed: vi.fn().mockReturnValue(false),
          send: vi.fn()
        }
      }

      const id = ptyManager.create('/test/cwd', mockWindow as any)

      expect(id).toMatch(/^pty-\d+$/)
      expect(pty.spawn).toHaveBeenCalled()
    })

    it('creates non-sandboxed PTY by default', () => {
      const mockWindow = {
        isDestroyed: vi.fn().mockReturnValue(false),
        webContents: {
          isDestroyed: vi.fn().mockReturnValue(false),
          send: vi.fn()
        }
      }

      const id = ptyManager.create('/test/cwd', mockWindow as any)

      expect(ptyManager.isSandboxed(id)).toBe(false)
    })
  })

  describe('write', () => {
    it('does nothing for non-existent PTY', () => {
      // Should not throw
      ptyManager.write('non-existent', 'data')
    })

    it('writes data to existing PTY', () => {
      const mockWindow = {
        isDestroyed: vi.fn().mockReturnValue(false),
        webContents: { isDestroyed: vi.fn().mockReturnValue(false), send: vi.fn() }
      }
      const id = ptyManager.create('/test/cwd', mockWindow as any)
      const spawnedPty = vi.mocked(pty.spawn).mock.results[0].value

      ptyManager.write(id, 'hello world')

      expect(spawnedPty.write).toHaveBeenCalledWith('hello world')
    })
  })

  describe('resize', () => {
    it('does nothing for non-existent PTY', () => {
      // Should not throw
      ptyManager.resize('non-existent', 100, 50)
    })

    it('resizes existing PTY', () => {
      const mockWindow = {
        isDestroyed: vi.fn().mockReturnValue(false),
        webContents: { isDestroyed: vi.fn().mockReturnValue(false), send: vi.fn() }
      }
      const id = ptyManager.create('/test/cwd', mockWindow as any)
      const spawnedPty = vi.mocked(pty.spawn).mock.results[0].value

      ptyManager.resize(id, 120, 40)

      expect(spawnedPty.resize).toHaveBeenCalledWith(120, 40)
    })
  })

  describe('kill', () => {
    it('does nothing for non-existent PTY', () => {
      // Should not throw
      ptyManager.kill('non-existent')
    })

    it('kills existing PTY and removes it', () => {
      const mockWindow = {
        isDestroyed: vi.fn().mockReturnValue(false),
        webContents: { isDestroyed: vi.fn().mockReturnValue(false), send: vi.fn() }
      }
      const id = ptyManager.create('/test/cwd', mockWindow as any)
      const spawnedPty = vi.mocked(pty.spawn).mock.results[0].value

      expect(ptyManager.isAlive(id)).toBe(true)
      ptyManager.kill(id)

      expect(spawnedPty.kill).toHaveBeenCalled()
      expect(ptyManager.isAlive(id)).toBe(false)
    })
  })

  describe('killAll', () => {
    it('kills all existing PTYs', () => {
      const mockWindow = {
        isDestroyed: vi.fn().mockReturnValue(false),
        webContents: { isDestroyed: vi.fn().mockReturnValue(false), send: vi.fn() }
      }
      const id1 = ptyManager.create('/test/cwd1', mockWindow as any)
      const id2 = ptyManager.create('/test/cwd2', mockWindow as any)

      expect(ptyManager.isAlive(id1)).toBe(true)
      expect(ptyManager.isAlive(id2)).toBe(true)

      ptyManager.killAll()

      expect(ptyManager.isAlive(id1)).toBe(false)
      expect(ptyManager.isAlive(id2)).toBe(false)
    })
  })

  describe('isAlive', () => {
    it('returns false for non-existent PTY', () => {
      expect(ptyManager.isAlive('non-existent')).toBe(false)
    })

    it('returns true after create', () => {
      const mockWindow = {
        isDestroyed: vi.fn().mockReturnValue(false),
        webContents: { isDestroyed: vi.fn().mockReturnValue(false), send: vi.fn() }
      }
      const id = ptyManager.create('/test/cwd', mockWindow as any)
      expect(ptyManager.isAlive(id)).toBe(true)
    })

    it('returns false after kill', () => {
      const mockWindow = {
        isDestroyed: vi.fn().mockReturnValue(false),
        webContents: { isDestroyed: vi.fn().mockReturnValue(false), send: vi.fn() }
      }
      const id = ptyManager.create('/test/cwd', mockWindow as any)
      ptyManager.kill(id)
      expect(ptyManager.isAlive(id)).toBe(false)
    })
  })

  describe('sandbox', () => {
    it('macOS sandbox uses sandbox-exec', async () => {
      const originalPlatform = process.platform
      Object.defineProperty(process, 'platform', { value: 'darwin', configurable: true })

      vi.resetModules()
      const ptyModule = await import('./pty')
      const mgr = ptyModule.ptyManager

      const mockWindow = {
        isDestroyed: vi.fn().mockReturnValue(false),
        webContents: { isDestroyed: vi.fn().mockReturnValue(false), send: vi.fn() }
      }

      const id = mgr.create('/test/cwd', mockWindow as any, {
        enabled: true,
        allowNetwork: false,
        allowedPaths: []
      })

      expect(pty.spawn).toHaveBeenCalledWith(
        '/usr/bin/sandbox-exec',
        expect.arrayContaining(['-p']),
        expect.any(Object)
      )
      expect(mgr.isSandboxed(id)).toBe(true)

      Object.defineProperty(process, 'platform', { value: originalPlatform, configurable: true })
    })

    it('macOS sandbox profile contains workspace path and deny network', async () => {
      const originalPlatform = process.platform
      Object.defineProperty(process, 'platform', { value: 'darwin', configurable: true })

      vi.resetModules()
      const ptyModule = await import('./pty')
      const mgr = ptyModule.ptyManager

      const mockWindow = {
        isDestroyed: vi.fn().mockReturnValue(false),
        webContents: { isDestroyed: vi.fn().mockReturnValue(false), send: vi.fn() }
      }

      mgr.create('/my/workspace', mockWindow as any, {
        enabled: true,
        allowNetwork: false,
        allowedPaths: []
      })

      const spawnCall = vi.mocked(pty.spawn).mock.calls[0]
      const profileArg = spawnCall[1][1] // -p <profile>
      expect(profileArg).toContain('/my/workspace')
      expect(profileArg).toContain('(deny network*)')

      Object.defineProperty(process, 'platform', { value: originalPlatform, configurable: true })
    })

    it('macOS sandbox with allowNetwork includes allow network', async () => {
      const originalPlatform = process.platform
      Object.defineProperty(process, 'platform', { value: 'darwin', configurable: true })

      vi.resetModules()
      const ptyModule = await import('./pty')
      const mgr = ptyModule.ptyManager

      const mockWindow = {
        isDestroyed: vi.fn().mockReturnValue(false),
        webContents: { isDestroyed: vi.fn().mockReturnValue(false), send: vi.fn() }
      }

      mgr.create('/my/workspace', mockWindow as any, {
        enabled: true,
        allowNetwork: true,
        allowedPaths: []
      })

      const spawnCall = vi.mocked(pty.spawn).mock.calls[0]
      const profileArg = spawnCall[1][1]
      expect(profileArg).toContain('(allow network*)')

      Object.defineProperty(process, 'platform', { value: originalPlatform, configurable: true })
    })

    it('Linux with bwrap uses bwrap for sandboxing', async () => {
      const originalPlatform = process.platform
      Object.defineProperty(process, 'platform', { value: 'linux', configurable: true })

      vi.resetModules()
      const ptyModule = await import('./pty')
      const mgr = ptyModule.ptyManager

      const mockWindow = {
        isDestroyed: vi.fn().mockReturnValue(false),
        webContents: { isDestroyed: vi.fn().mockReturnValue(false), send: vi.fn() }
      }

      const id = mgr.create('/test/cwd', mockWindow as any, {
        enabled: true,
        allowNetwork: false,
        allowedPaths: []
      })

      expect(pty.spawn).toHaveBeenCalledWith(
        'bwrap',
        expect.arrayContaining(['--die-with-parent']),
        expect.any(Object)
      )
      expect(mgr.isSandboxed(id)).toBe(true)

      Object.defineProperty(process, 'platform', { value: originalPlatform, configurable: true })
    })

    it('Linux without bwrap falls back to default shell', async () => {
      const originalPlatform = process.platform
      Object.defineProperty(process, 'platform', { value: 'linux', configurable: true })

      vi.resetModules()
      const { execSync } = await import('child_process')
      vi.mocked(execSync).mockImplementation(() => { throw new Error('not found') })

      const ptyModule = await import('./pty')
      const mgr = ptyModule.ptyManager

      const mockWindow = {
        isDestroyed: vi.fn().mockReturnValue(false),
        webContents: { isDestroyed: vi.fn().mockReturnValue(false), send: vi.fn() }
      }

      const id = mgr.create('/test/cwd', mockWindow as any, {
        enabled: true,
        allowNetwork: false,
        allowedPaths: []
      })

      // Should not use bwrap
      const spawnCall = vi.mocked(pty.spawn).mock.calls[0]
      expect(spawnCall[0]).not.toBe('bwrap')

      Object.defineProperty(process, 'platform', { value: originalPlatform, configurable: true })
    })

    it('unsupported platform sandbox falls back to default shell', async () => {
      const originalPlatform = process.platform
      Object.defineProperty(process, 'platform', { value: 'win32', configurable: true })

      vi.resetModules()
      const ptyModule = await import('./pty')
      const mgr = ptyModule.ptyManager

      const mockWindow = {
        isDestroyed: vi.fn().mockReturnValue(false),
        webContents: { isDestroyed: vi.fn().mockReturnValue(false), send: vi.fn() }
      }

      mgr.create('/test/cwd', mockWindow as any, {
        enabled: true,
        allowNetwork: false,
        allowedPaths: []
      })

      const spawnCall = vi.mocked(pty.spawn).mock.calls[0]
      expect(spawnCall[0]).toBe('powershell.exe')

      Object.defineProperty(process, 'platform', { value: originalPlatform, configurable: true })
    })
  })

  describe('callbacks', () => {
    it('onData sends data to window webContents', () => {
      const mockSend = vi.fn()
      const mockWindow = {
        isDestroyed: vi.fn().mockReturnValue(false),
        webContents: { isDestroyed: vi.fn().mockReturnValue(false), send: mockSend }
      }
      const id = ptyManager.create('/test/cwd', mockWindow as any)
      const spawnedPty = vi.mocked(pty.spawn).mock.results[0].value

      // Trigger onData callback
      const onDataHandler = spawnedPty.onData.mock.calls[0][0]
      onDataHandler('test output')

      expect(mockSend).toHaveBeenCalledWith('pty:data', id, 'test output')
    })

    it('onData skips destroyed window', () => {
      const mockSend = vi.fn()
      const mockWindow = {
        isDestroyed: vi.fn().mockReturnValue(true),
        webContents: { isDestroyed: vi.fn().mockReturnValue(false), send: mockSend }
      }
      ptyManager.create('/test/cwd', mockWindow as any)
      const spawnedPty = vi.mocked(pty.spawn).mock.results[0].value

      const onDataHandler = spawnedPty.onData.mock.calls[0][0]
      onDataHandler('test output')

      expect(mockSend).not.toHaveBeenCalled()
    })

    it('onExit cleans up PTY and sends exit event', () => {
      const mockSend = vi.fn()
      const mockWindow = {
        isDestroyed: vi.fn().mockReturnValue(false),
        webContents: { isDestroyed: vi.fn().mockReturnValue(false), send: mockSend }
      }
      const id = ptyManager.create('/test/cwd', mockWindow as any)
      const spawnedPty = vi.mocked(pty.spawn).mock.results[0].value

      expect(ptyManager.isAlive(id)).toBe(true)

      // Trigger onExit callback
      const onExitHandler = spawnedPty.onExit.mock.calls[0][0]
      onExitHandler({ exitCode: 0 })

      expect(mockSend).toHaveBeenCalledWith('pty:exit', id, 0)
      expect(ptyManager.isAlive(id)).toBe(false)
    })
  })

  describe('startup command', () => {
    afterEach(() => {
      vi.useRealTimers()
    })

    it('writes startup command after delay', () => {
      vi.useFakeTimers()

      const mockWindow = {
        isDestroyed: vi.fn().mockReturnValue(false),
        webContents: { isDestroyed: vi.fn().mockReturnValue(false), send: vi.fn() }
      }
      ptyManager.create('/test/cwd', mockWindow as any, undefined, 'npm start')
      const spawnedPty = vi.mocked(pty.spawn).mock.results[0].value

      // Not yet written
      expect(spawnedPty.write).not.toHaveBeenCalled()

      vi.advanceTimersByTime(150)

      expect(spawnedPty.write).toHaveBeenCalledWith(expect.stringContaining('npm start'))
    })
  })
})
