import { describe, it, expect, vi, beforeEach } from 'vitest'
import type { IpcServer } from './ipc/ipc-server'
import type { GrpcDaemonClient } from './grpcClient'

const mockDaemonReadDirectory = vi.fn<(workspacePath: string, dirPath: string) => any>()
const mockDaemonReadFile = vi.fn<(workspacePath: string, filePath: string) => any>()
const mockDaemonWriteFile = vi.fn<(workspacePath: string, filePath: string, content: string) => any>()

const mockServer = {
  onFsReadDirectory: vi.fn<(handler: (workspacePath: string, dirPath: string) => any) => void>(),
  onFsReadFile: vi.fn<(handler: (workspacePath: string, filePath: string) => any) => void>(),
  onFsWriteFile: vi.fn<(handler: (workspacePath: string, filePath: string, content: string) => any) => void>()
}

const mockDaemonClient = {
  readDirectory: (workspacePath: string, dirPath: string): any => mockDaemonReadDirectory(workspacePath, dirPath),
  readFile: (workspacePath: string, filePath: string): any => mockDaemonReadFile(workspacePath, filePath),
  writeFile: (workspacePath: string, filePath: string, content: string): any => mockDaemonWriteFile(workspacePath, filePath, content)
}

import { registerFilesystemHandlers } from './filesystem'

describe('filesystem', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  describe('registerFilesystemHandlers', () => {
    it('registers filesystem handlers', () => {
      registerFilesystemHandlers(mockServer as unknown as IpcServer, mockDaemonClient as unknown as GrpcDaemonClient)

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

      registerFilesystemHandlers(mockServer as unknown as IpcServer, mockDaemonClient as unknown as GrpcDaemonClient)
      const handler = mockServer.onFsReadDirectory.mock.calls[0][0] as (...args: any[]) => Promise<{ success: boolean; contents?: any; error?: string }>

      const result = await handler('/workspace', '/workspace')

      expect(result).toEqual(expected)
      expect(mockDaemonReadDirectory).toHaveBeenCalledWith('/workspace', '/workspace')
    })

    it('returns error on daemon failure', async () => {
      mockDaemonReadDirectory.mockRejectedValue(new Error('Connection failed'))

      registerFilesystemHandlers(mockServer as unknown as IpcServer, mockDaemonClient as unknown as GrpcDaemonClient)
      const handler = mockServer.onFsReadDirectory.mock.calls[0][0] as (...args: any[]) => Promise<{ success: boolean; error?: string }>

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

      registerFilesystemHandlers(mockServer as unknown as IpcServer, mockDaemonClient as unknown as GrpcDaemonClient)
      const handler = mockServer.onFsReadFile.mock.calls[0][0] as (...args: any[]) => Promise<{ success: boolean; file?: any; error?: string }>

      const result = await handler('/workspace', '/workspace/test.ts')

      expect(result).toEqual(expected)
      expect(mockDaemonReadFile).toHaveBeenCalledWith('/workspace', '/workspace/test.ts')
    })

    it('returns error on daemon failure', async () => {
      mockDaemonReadFile.mockRejectedValue(new Error('File not found'))

      registerFilesystemHandlers(mockServer as unknown as IpcServer, mockDaemonClient as unknown as GrpcDaemonClient)
      const handler = mockServer.onFsReadFile.mock.calls[0][0] as (...args: any[]) => Promise<{ success: boolean; error?: string }>

      const result = await handler('/workspace', '/workspace/missing.txt')

      expect(result.success).toBe(false)
      expect(result.error).toContain('File not found')
    })
  })

  describe('write file handler', () => {
    it('delegates to daemon client', async () => {
      mockDaemonWriteFile.mockResolvedValue({ success: true })

      registerFilesystemHandlers(mockServer as unknown as IpcServer, mockDaemonClient as unknown as GrpcDaemonClient)
      const handler = mockServer.onFsWriteFile.mock.calls[0][0] as (...args: any[]) => Promise<{ success: boolean; error?: string }>

      const result = await handler('/workspace', '/workspace/test.txt', 'new content')

      expect(result.success).toBe(true)
      expect(mockDaemonWriteFile).toHaveBeenCalledWith('/workspace', '/workspace/test.txt', 'new content')
    })

    it('returns error on daemon failure', async () => {
      mockDaemonWriteFile.mockRejectedValue(new Error('Permission denied'))

      registerFilesystemHandlers(mockServer as unknown as IpcServer, mockDaemonClient as unknown as GrpcDaemonClient)
      const handler = mockServer.onFsWriteFile.mock.calls[0][0] as (...args: any[]) => Promise<{ success: boolean; error?: string }>

      const result = await handler('/workspace', '/workspace/test.txt', 'content')

      expect(result.success).toBe(false)
      expect(result.error).toContain('Permission denied')
    })
  })
})
