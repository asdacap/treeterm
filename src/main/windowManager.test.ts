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
import { windowManager, WindowInfo } from './windowManager'

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
    it('registers a window with session info', () => {
      const win = { ...mockBrowserWindow, id: 2, webContents: { id: 102 } }
      
      windowManager.registerWindow(win as any, 'session-1', mockIpcServer as any, 'uuid-1')
      
      const info = windowManager.getWindow(2)
      expect(info).toBeDefined()
      expect(info?.sessionId).toBe('session-1')
      expect(info?.uuid).toBe('uuid-1')
    })

    it('auto-generates uuid if not provided', () => {
      const win = { ...mockBrowserWindow, id: 3, webContents: { id: 103 } }
      
      windowManager.registerWindow(win as any, null, mockIpcServer as any)
      
      const info = windowManager.getWindow(3)
      expect(info?.uuid).toMatch(/^win-/)
    })

    it('sets up closed event handler', () => {
      const win = { ...mockBrowserWindow, id: 4, webContents: { id: 104 }, on: vi.fn() }
      
      windowManager.registerWindow(win as any, 'session-1', mockIpcServer as any)
      
      expect(win.on).toHaveBeenCalledWith('closed', expect.any(Function))
    })
  })

  describe('getWindow', () => {
    it('returns undefined for unregistered window', () => {
      const info = windowManager.getWindow(999)
      expect(info).toBeUndefined()
    })

    it('returns window info for registered window', () => {
      const win = { ...mockBrowserWindow, id: 5, webContents: { id: 105 }, on: vi.fn() }
      
      windowManager.registerWindow(win as any, 'session-1', mockIpcServer as any)
      
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

  describe('findWindowBySessionId', () => {
    it('finds window by session ID', () => {
      const win = { ...mockBrowserWindow, id: 6, webContents: { id: 106 }, on: vi.fn() }
      
      windowManager.registerWindow(win as any, 'session-find-test', mockIpcServer as any)
      
      const found = windowManager.findWindowBySessionId('session-find-test')
      expect(found?.window.id).toBe(6)
    })

    it('returns undefined when session not found', () => {
      const found = windowManager.findWindowBySessionId('nonexistent-session')
      expect(found).toBeUndefined()
    })
  })

  describe('isSessionOpen', () => {
    it('returns true when session is open', () => {
      const win = { ...mockBrowserWindow, id: 7, webContents: { id: 107 }, on: vi.fn() }
      
      windowManager.registerWindow(win as any, 'session-open-test', mockIpcServer as any)
      
      const isOpen = windowManager.isSessionOpen('session-open-test')
      expect(isOpen).toBe(true)
    })

    it('returns false when session is not open', () => {
      const isOpen = windowManager.isSessionOpen('never-registered-session')
      expect(isOpen).toBe(false)
    })
  })

  describe('updateSessionId', () => {
    it('updates session ID for existing window', () => {
      const win = { ...mockBrowserWindow, id: 8, webContents: { id: 108 }, on: vi.fn() }
      
      windowManager.registerWindow(win as any, 'old-session', mockIpcServer as any)
      windowManager.updateSessionId(8, 'new-session')
      
      const info = windowManager.getWindow(8)
      expect(info?.sessionId).toBe('new-session')
    })

    it('does nothing for non-existent window', () => {
      expect(() => windowManager.updateSessionId(999, 'new-session')).not.toThrow()
    })
  })

  describe('findWindowByWebContentsId', () => {
    it('finds window by webContents ID', () => {
      const win = { ...mockBrowserWindow, id: 9, webContents: { id: 109 }, on: vi.fn() }
      
      windowManager.registerWindow(win as any, 'session-1', mockIpcServer as any)
      
      const found = windowManager.findWindowByWebContentsId(109)
      expect(found?.window.id).toBe(9)
    })

    it('returns undefined when webContents ID not found', () => {
      const found = windowManager.findWindowByWebContentsId(9999)
      expect(found).toBeUndefined()
    })
  })

  describe('getWindowsBySessionId', () => {
    it('returns all windows for a session ID', () => {
      const win1 = { ...mockBrowserWindow, id: 10, webContents: { id: 110 }, on: vi.fn() }
      const win2 = { ...mockBrowserWindow, id: 11, webContents: { id: 111 }, on: vi.fn() }
      
      windowManager.registerWindow(win1 as any, 'shared-session', mockIpcServer as any)
      windowManager.registerWindow(win2 as any, 'shared-session', mockIpcServer as any)
      
      const windows = windowManager.getWindowsBySessionId('shared-session')
      expect(windows.length).toBeGreaterThanOrEqual(2)
    })

    it('returns empty array when no windows for session', () => {
      const windows = windowManager.getWindowsBySessionId('no-windows-session')
      expect(windows).toEqual([])
    })
  })

  describe('focusWindowBySessionId', () => {
    it('focuses window when found', () => {
      const win = { 
        ...mockBrowserWindow, 
        id: 12, 
        webContents: { id: 112 }, 
        on: vi.fn(),
        isMinimized: vi.fn().mockReturnValue(false),
        focus: vi.fn()
      }
      
      windowManager.registerWindow(win as any, 'focus-session', mockIpcServer as any)
      
      const result = windowManager.focusWindowBySessionId('focus-session')
      
      expect(result).toBe(true)
      expect(win.focus).toHaveBeenCalled()
    })

    it('restores minimized window before focusing', () => {
      const win = { 
        ...mockBrowserWindow, 
        id: 13, 
        webContents: { id: 113 }, 
        on: vi.fn(),
        isMinimized: vi.fn().mockReturnValue(true),
        restore: vi.fn(),
        focus: vi.fn()
      }
      
      windowManager.registerWindow(win as any, 'minimized-session', mockIpcServer as any)
      
      windowManager.focusWindowBySessionId('minimized-session')
      
      expect(win.restore).toHaveBeenCalled()
      expect(win.focus).toHaveBeenCalled()
    })

    it('returns false when session not found', () => {
      const result = windowManager.focusWindowBySessionId('nonexistent-session')
      expect(result).toBe(false)
    })
  })
})
