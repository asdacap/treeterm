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
  })

  describe('resize', () => {
    it('does nothing for non-existent PTY', () => {
      // Should not throw
      ptyManager.resize('non-existent', 100, 50)
    })
  })
})
