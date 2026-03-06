import { vi } from 'vitest'

export const app = {
  getPath: vi.fn().mockReturnValue('/mock/userData'),
  getName: vi.fn().mockReturnValue('TreeTerm'),
  getVersion: vi.fn().mockReturnValue('0.1.0')
}

export const BrowserWindow = vi.fn().mockImplementation(() => ({
  isDestroyed: vi.fn().mockReturnValue(false),
  webContents: {
    isDestroyed: vi.fn().mockReturnValue(false),
    send: vi.fn()
  }
}))
