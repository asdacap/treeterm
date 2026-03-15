import { describe, it, expect, vi, beforeEach } from 'vitest'
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
})
