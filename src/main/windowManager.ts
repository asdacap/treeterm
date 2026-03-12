/**
 * Window Manager for multi-window session support
 * Tracks multiple windows and their associated sessions
 */

import { BrowserWindow } from 'electron'
import { IpcServer } from './ipc/ipc-server'

interface WindowInfo {
  window: BrowserWindow
  sessionId: string | null
  ipcServer: IpcServer
}

class WindowManager {
  private windows: Map<number, WindowInfo> = new Map()
  private static instance: WindowManager | null = null

  static getInstance(): WindowManager {
    if (!WindowManager.instance) {
      WindowManager.instance = new WindowManager()
    }
    return WindowManager.instance
  }

  /**
   * Register a window with its associated session
   */
  registerWindow(window: BrowserWindow, sessionId: string | null, ipcServer: IpcServer): void {
    const id = window.id
    this.windows.set(id, { window, sessionId, ipcServer })

    // Clean up when window closes
    window.on('closed', () => {
      this.windows.delete(id)
    })
  }

  /**
   * Get window info by window ID
   */
  getWindow(id: number): WindowInfo | undefined {
    return this.windows.get(id)
  }

  /**
   * Get all managed windows
   */
  getAllWindows(): WindowInfo[] {
    return Array.from(this.windows.values())
  }

  /**
   * Get window count
   */
  getWindowCount(): number {
    return this.windows.size
  }

  /**
   * Find window by session ID
   */
  findWindowBySessionId(sessionId: string): WindowInfo | undefined {
    for (const info of this.windows.values()) {
      if (info.sessionId === sessionId) {
        return info
      }
    }
    return undefined
  }

  /**
   * Check if a session is already open in any window
   */
  isSessionOpen(sessionId: string): boolean {
    return this.findWindowBySessionId(sessionId) !== undefined
  }

  /**
   * Focus window by session ID
   */
  focusWindowBySessionId(sessionId: string): boolean {
    const info = this.findWindowBySessionId(sessionId)
    if (info) {
      if (info.window.isMinimized()) {
        info.window.restore()
      }
      info.window.focus()
      return true
    }
    return false
  }
}

export const windowManager = WindowManager.getInstance()
export type { WindowInfo }
