/**
 * Window Manager for multi-window support
 * Tracks windows and their metadata (ipcServer, connectionId, uuid)
 */

import { BrowserWindow } from 'electron'
import { IpcServer } from './ipc/ipc-server'

interface WindowInfo {
  window: BrowserWindow
  ipcServer: IpcServer
  uuid: string
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
   * Register a window
   */
  registerWindow(window: BrowserWindow, ipcServer: IpcServer, uuid?: string): void {
    const id = window.id
    this.windows.set(id, { window, ipcServer, uuid: uuid || `win-${String(id)}` })

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
   * Find window by webContents ID
   */
  findWindowByWebContentsId(webContentsId: number): WindowInfo | undefined {
    for (const info of this.windows.values()) {
      if (info.window.webContents.id === webContentsId) {
        return info
      }
    }
    return undefined
  }
}

export const windowManager = WindowManager.getInstance()
export type { WindowInfo }
