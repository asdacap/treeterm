import { describe, it, expect, vi, beforeEach } from 'vitest'

const mockDaemonReadDirectory = vi.fn()
const mockDaemonReadFile = vi.fn()
const mockDaemonWriteFile = vi.fn()

const mockServer = {
  onFsReadDirectory: vi.fn(),
  onFsReadFile: vi.fn(),
  onFsWriteFile: vi.fn()
}

const mockDaemonClient = {
  readDirectory: (...args: any[]) => mockDaemonReadDirectory(...args),
  readFile: (...args: any[]) => mockDaemonReadFile(...args),
  writeFile: (...args: any[]) => mockDaemonWriteFile(...args)
}

import { registerFilesystemHandlers } from './filesystem'

describe('filesystem', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  describe('registerFilesystemHandlers', () => {
    it('registers filesystem handlers', () => {
      registerFilesystemHandlers(mockServer as any, mockDaemonClient as any)

      expect(mockServer.onFsReadDirectory).toHaveBeenCalled()
      expect(mockServer.onFsReadFile).toHaveBeenCalled()
      expect(mockServer.onFsWriteFile).toHaveBeenCalled()
    })
  })

  describe('read directory handler', () => {
    it('delegates to daemon client', async () => {
      const expected = {
        success: true,
        contents: { path: '/workspace', entries: [{ name: 'file.txt' }] }
      }
      mockDaemonReadDirectory.mockResolvedValue(expected)

      registerFilesystemHandlers(mockServer as any, mockDaemonClient as any)
      const handler = mockServer.onFsReadDirectory.mock.calls[0][0]

      const result = await handler('/workspace', '/workspace')

      expect(result).toEqual(expected)
      expect(mockDaemonReadDirectory).toHaveBeenCalledWith('/workspace', '/workspace')
    })

    it('returns error on daemon failure', async () => {
      mockDaemonReadDirectory.mockRejectedValue(new Error('Connection failed'))

      registerFilesystemHandlers(mockServer as any, mockDaemonClient as any)
      const handler = mockServer.onFsReadDirectory.mock.calls[0][0]

      const result = await handler('/workspace', '/workspace')

      expect(result.success).toBe(false)
      expect(result.error).toContain('Connection failed')
    })
  })

  describe('read file handler', () => {
    it('delegates to daemon client', async () => {
      const expected = {
        success: true,
        file: { path: '/workspace/test.ts', content: 'const x = 1', size: 100, language: 'typescript' }
      }
      mockDaemonReadFile.mockResolvedValue(expected)

      registerFilesystemHandlers(mockServer as any, mockDaemonClient as any)
      const handler = mockServer.onFsReadFile.mock.calls[0][0]

      const result = await handler('/workspace', '/workspace/test.ts')

      expect(result).toEqual(expected)
      expect(mockDaemonReadFile).toHaveBeenCalledWith('/workspace', '/workspace/test.ts')
    })

    it('returns error on daemon failure', async () => {
      mockDaemonReadFile.mockRejectedValue(new Error('File not found'))

      registerFilesystemHandlers(mockServer as any, mockDaemonClient as any)
      const handler = mockServer.onFsReadFile.mock.calls[0][0]

      const result = await handler('/workspace', '/workspace/missing.txt')

      expect(result.success).toBe(false)
      expect(result.error).toContain('File not found')
    })
  })

  describe('write file handler', () => {
    it('delegates to daemon client', async () => {
      mockDaemonWriteFile.mockResolvedValue({ success: true })

      registerFilesystemHandlers(mockServer as any, mockDaemonClient as any)
      const handler = mockServer.onFsWriteFile.mock.calls[0][0]

      const result = await handler('/workspace', '/workspace/test.txt', 'new content')

      expect(result.success).toBe(true)
      expect(mockDaemonWriteFile).toHaveBeenCalledWith('/workspace', '/workspace/test.txt', 'new content')
    })

    it('returns error on daemon failure', async () => {
      mockDaemonWriteFile.mockRejectedValue(new Error('Permission denied'))

      registerFilesystemHandlers(mockServer as any, mockDaemonClient as any)
      const handler = mockServer.onFsWriteFile.mock.calls[0][0]

      const result = await handler('/workspace', '/workspace/test.txt', 'content')

      expect(result.success).toBe(false)
      expect(result.error).toContain('Permission denied')
    })
  })
})
