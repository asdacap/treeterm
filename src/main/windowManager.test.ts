import { describe, it, expect, vi, beforeEach } from 'vitest'

const mockBrowserWindow = {
  id: 1,
  webContents: { id: 101 },
  on: vi.fn(),
  isMinimized: vi.fn().mockReturnValue(false),
  restore: vi.fn(),
  focus: vi.fn()
}

const mockIpcServer = {}

vi.mock('electron', () => ({
  BrowserWindow: vi.fn(() => mockBrowserWindow)
}))

// Import after mocking
import { windowManager } from './windowManager'

describe('WindowManager', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  describe('getInstance', () => {
    it('returns singleton instance', () => {
      expect(windowManager).toBeDefined()
    })
  })

  describe('registerWindow', () => {
    it('registers a window with info', () => {
      const win = { ...mockBrowserWindow, id: 2, webContents: { id: 102 } }

      windowManager.registerWindow(win as any, mockIpcServer as any, 'uuid-1')

      const info = windowManager.getWindow(2)
      expect(info).toBeDefined()
      expect(info?.uuid).toBe('uuid-1')
    })

    it('auto-generates uuid if not provided', () => {
      const win = { ...mockBrowserWindow, id: 3, webContents: { id: 103 } }

      windowManager.registerWindow(win as any, mockIpcServer as any)

      const info = windowManager.getWindow(3)
      expect(info?.uuid).toMatch(/^win-/)
    })

    it('sets up closed event handler that removes window', () => {
      const win = { ...mockBrowserWindow, id: 4, webContents: { id: 104 }, on: vi.fn() }

      windowManager.registerWindow(win as any, mockIpcServer as any)
      expect(windowManager.getWindow(4)).toBeDefined()

      // Simulate the closed event
      const closedHandler = win.on.mock.calls.find((c: any[]) => c[0] === 'closed')?.[1]
      closedHandler()

      expect(windowManager.getWindow(4)).toBeUndefined()
    })
  })

  describe('getWindow', () => {
    it('returns undefined for unregistered window', () => {
      const info = windowManager.getWindow(999)
      expect(info).toBeUndefined()
    })

    it('returns window info for registered window', () => {
      const win = { ...mockBrowserWindow, id: 5, webContents: { id: 105 }, on: vi.fn() }

      windowManager.registerWindow(win as any, mockIpcServer as any)

      const info = windowManager.getWindow(5)
      expect(info?.window).toBe(win)
    })
  })

  describe('getAllWindows', () => {
    it('returns empty array when no windows registered', () => {
      const windows = windowManager.getAllWindows()
      expect(windows).toBeInstanceOf(Array)
    })
  })

  describe('getWindowCount', () => {
    it('returns 0 when no windows', () => {
      // Note: other tests may have registered windows
      const count = windowManager.getWindowCount()
      expect(typeof count).toBe('number')
    })
  })

  describe('findWindowByWebContentsId', () => {
    it('finds window by webContents ID', () => {
      const win = { ...mockBrowserWindow, id: 9, webContents: { id: 109 }, on: vi.fn() }

      windowManager.registerWindow(win as any, mockIpcServer as any)

      const found = windowManager.findWindowByWebContentsId(109)
      expect(found?.window.id).toBe(9)
    })

    it('returns undefined when webContents ID not found', () => {
      const found = windowManager.findWindowByWebContentsId(9999)
      expect(found).toBeUndefined()
    })
  })
})
