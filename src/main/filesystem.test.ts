import { describe, it, expect, vi, beforeEach } from 'vitest'

const mockReaddir = vi.fn()
const mockReadFile = vi.fn()
const mockStat = vi.fn()
const mockDaemonWriteFile = vi.fn()

vi.mock('fs/promises', () => ({
  readdir: (...args: any[]) => mockReaddir(...args),
  readFile: (...args: any[]) => mockReadFile(...args),
  stat: (...args: any[]) => mockStat(...args)
}))

const mockServer = {
  onFsReadDirectory: vi.fn(),
  onFsReadFile: vi.fn(),
  onFsWriteFile: vi.fn()
}

const mockDaemonClient = {
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
    it('returns error for path outside workspace', async () => {
      registerFilesystemHandlers(mockServer as any, mockDaemonClient as any)
      const handler = mockServer.onFsReadDirectory.mock.calls[0][0]
      
      const result = await handler('/workspace', '/other/path')
      
      expect(result.success).toBe(false)
      expect(result.error).toContain('Access denied')
    })

    it('reads directory entries', async () => {
      mockReaddir.mockResolvedValue([
        { name: 'file.txt', isDirectory: () => false, isFile: () => true },
        { name: 'folder', isDirectory: () => true, isFile: () => false }
      ])
      mockStat.mockResolvedValue({
        size: 100,
        mtimeMs: Date.now()
      })

      registerFilesystemHandlers(mockServer as any, mockDaemonClient as any)
      const handler = mockServer.onFsReadDirectory.mock.calls[0][0]
      
      const result = await handler('/workspace', '/workspace')
      
      expect(result.success).toBe(true)
      expect(result.contents.entries).toHaveLength(2)
    })

    it('filters hidden files', async () => {
      mockReaddir.mockResolvedValue([
        { name: '.git', isDirectory: () => true, isFile: () => false },
        { name: 'visible.txt', isDirectory: () => false, isFile: () => true }
      ])
      mockStat.mockResolvedValue({
        size: 100,
        mtimeMs: Date.now()
      })

      registerFilesystemHandlers(mockServer as any, mockDaemonClient as any)
      const handler = mockServer.onFsReadDirectory.mock.calls[0][0]
      
      const result = await handler('/workspace', '/workspace')
      
      expect(result.contents.entries).toHaveLength(1)
      expect(result.contents.entries[0].name).toBe('visible.txt')
    })

    it('handles stat errors gracefully', async () => {
      mockReaddir.mockResolvedValue([
        { name: 'file.txt', isDirectory: () => false, isFile: () => true }
      ])
      mockStat.mockRejectedValue(new Error('Permission denied'))

      registerFilesystemHandlers(mockServer as any, mockDaemonClient as any)
      const handler = mockServer.onFsReadDirectory.mock.calls[0][0]
      
      const result = await handler('/workspace', '/workspace')
      
      expect(result.success).toBe(true)
      expect(result.contents.entries[0].size).toBeUndefined()
    })

    it('sorts directories first', async () => {
      mockReaddir.mockResolvedValue([
        { name: 'zebra.txt', isDirectory: () => false, isFile: () => true },
        { name: 'alpha', isDirectory: () => true, isFile: () => false },
        { name: 'beta.txt', isDirectory: () => false, isFile: () => true }
      ])
      mockStat.mockResolvedValue({ size: 0, mtimeMs: 0 })

      registerFilesystemHandlers(mockServer as any, mockDaemonClient as any)
      const handler = mockServer.onFsReadDirectory.mock.calls[0][0]
      
      const result = await handler('/workspace', '/workspace')
      
      expect(result.contents.entries[0].name).toBe('alpha')
      expect(result.contents.entries[0].isDirectory).toBe(true)
    })
  })

  describe('read file handler', () => {
    it('returns error for path outside workspace', async () => {
      registerFilesystemHandlers(mockServer as any, mockDaemonClient as any)
      const handler = mockServer.onFsReadFile.mock.calls[0][0]
      
      const result = await handler('/workspace', '/other/file.txt')
      
      expect(result.success).toBe(false)
      expect(result.error).toContain('Access denied')
    })

    it('returns error for files larger than 1MB', async () => {
      mockStat.mockResolvedValue({ size: 2 * 1024 * 1024 })

      registerFilesystemHandlers(mockServer as any, mockDaemonClient as any)
      const handler = mockServer.onFsReadFile.mock.calls[0][0]
      
      const result = await handler('/workspace', '/workspace/large.bin')
      
      expect(result.success).toBe(false)
      expect(result.error).toContain('too large')
    })

    it('reads file content with language detection', async () => {
      mockStat.mockResolvedValue({ size: 100 })
      mockReadFile.mockResolvedValue('file content')

      registerFilesystemHandlers(mockServer as any, mockDaemonClient as any)
      const handler = mockServer.onFsReadFile.mock.calls[0][0]
      
      const result = await handler('/workspace', '/workspace/test.ts')
      
      expect(result.success).toBe(true)
      expect(result.file.content).toBe('file content')
      expect(result.file.language).toBe('typescript')
    })

    it('detects different languages by extension', async () => {
      mockStat.mockResolvedValue({ size: 100 })
      mockReadFile.mockResolvedValue('content')

      registerFilesystemHandlers(mockServer as any, mockDaemonClient as any)
      const handler = mockServer.onFsReadFile.mock.calls[0][0]
      
      const jsResult = await handler('/workspace', '/workspace/test.js')
      expect(jsResult.file.language).toBe('javascript')
      
      const pyResult = await handler('/workspace', '/workspace/test.py')
      expect(pyResult.file.language).toBe('python')
      
      const jsonResult = await handler('/workspace', '/workspace/test.json')
      expect(jsonResult.file.language).toBe('json')
      
      const unknownResult = await handler('/workspace', '/workspace/test.xyz')
      expect(unknownResult.file.language).toBe('plaintext')
    })

    it('handles read errors', async () => {
      mockStat.mockRejectedValue(new Error('File not found'))

      registerFilesystemHandlers(mockServer as any, mockDaemonClient as any)
      const handler = mockServer.onFsReadFile.mock.calls[0][0]
      
      const result = await handler('/workspace', '/workspace/missing.txt')
      
      expect(result.success).toBe(false)
      expect(result.error).toContain('File not found')
    })
  })

  describe('write file handler', () => {
    it('returns error for path outside workspace', async () => {
      registerFilesystemHandlers(mockServer as any, mockDaemonClient as any)
      const handler = mockServer.onFsWriteFile.mock.calls[0][0]
      
      const result = await handler('/workspace', '/other/file.txt', 'content')
      
      expect(result.success).toBe(false)
      expect(result.error).toContain('Access denied')
    })

    it('writes file via daemon client', async () => {
      mockDaemonWriteFile.mockResolvedValue(undefined)

      registerFilesystemHandlers(mockServer as any, mockDaemonClient as any)
      const handler = mockServer.onFsWriteFile.mock.calls[0][0]
      
      const result = await handler('/workspace', '/workspace/test.txt', 'new content')
      
      expect(result.success).toBe(true)
      expect(mockDaemonWriteFile).toHaveBeenCalledWith('/workspace', '/workspace/test.txt', 'new content')
    })

    it('handles daemon write errors', async () => {
      mockDaemonWriteFile.mockRejectedValue(new Error('Permission denied'))

      registerFilesystemHandlers(mockServer as any, mockDaemonClient as any)
      const handler = mockServer.onFsWriteFile.mock.calls[0][0]
      
      const result = await handler('/workspace', '/workspace/test.txt', 'content')
      
      expect(result.success).toBe(false)
      expect(result.error).toContain('Permission denied')
    })
  })
})
