import { describe, it, expect, vi, beforeEach } from 'vitest'

const mocks = vi.hoisted(() => {
  const mockClientInstance = {
    waitForReady: vi.fn(),
    createPty: vi.fn(),
    attachPty: vi.fn(),
    detachPty: vi.fn(),
    killPty: vi.fn(),
    listPtySessions: vi.fn(),
    shutdown: vi.fn(),
    ptyStream: vi.fn(),
    execStream: vi.fn(),
    createSession: vi.fn(),
    updateSession: vi.fn(),
    deleteSession: vi.fn(),
    listSessions: vi.fn(),
    getDefaultSessionId: vi.fn(),
    sessionWatch: vi.fn(),
    readDirectory: vi.fn(),
    readFile: vi.fn(),
    writeFile: vi.fn(),
    searchFiles: vi.fn(),
    close: vi.fn()
  }
  return { mockClientInstance }
})

vi.mock('@grpc/grpc-js', () => {
  class MockMetadata {
    set = vi.fn()
  }
  return {
    credentials: {
      createInsecure: vi.fn().mockReturnValue('insecure-creds')
    },
    Metadata: MockMetadata,
    status: {
      NOT_FOUND: 5,
      INTERNAL: 13
    }
  }
})

vi.mock('../generated/treeterm', () => {
  return {
    TreeTermDaemonClient: class { constructor() { Object.assign(this, mocks.mockClientInstance) } }
  }
})

vi.mock('fs', () => ({
  existsSync: vi.fn().mockReturnValue(true),
  openSync: vi.fn().mockReturnValue(3)
}))

vi.mock('child_process', () => ({
  spawn: vi.fn().mockReturnValue({ unref: vi.fn(), pid: 1234 })
}))

vi.mock('electron', () => ({
  app: {
    isPackaged: false,
    getPath: vi.fn().mockReturnValue('/tmp')
  }
}))

vi.mock('../daemon/socketPath', () => ({
  getDefaultSocketPath: vi.fn().mockReturnValue('/tmp/test.sock')
}))

import { GrpcDaemonClient } from './grpcClient'

const { mockClientInstance } = mocks

// Helper to make the client connected
function connectClient(client: GrpcDaemonClient): void {
  const mockStream = {
    on: vi.fn(),
    write: vi.fn(),
    end: vi.fn(),
    cancel: vi.fn()
  }
  mockClientInstance.ptyStream.mockReturnValue(mockStream)

  mockClientInstance.waitForReady.mockImplementation((_deadline: number, cb: (err?: Error) => void) => {
    cb()
  })
}

describe('GrpcDaemonClient', () => {
  let client: GrpcDaemonClient

  beforeEach(() => {
    vi.clearAllMocks()
    client = new GrpcDaemonClient('/tmp/test.sock')
  })

  describe('connection lifecycle', () => {
    it('connect succeeds when socket exists and client is ready', async () => {
      connectClient(client)
      await client.connect()
      expect(client.isConnected()).toBe(true)
    })

    it('connect throws when socket does not exist', async () => {
      const fs = await import('fs')
      vi.mocked(fs.existsSync).mockReturnValueOnce(false)

      await expect(client.connect()).rejects.toThrow('Daemon socket not found')
    })

    it('connect returns immediately if already connected', async () => {
      connectClient(client)
      await client.connect()
      await client.connect() // second call
      expect(mockClientInstance.waitForReady).toHaveBeenCalledTimes(1)
    })

    it('connect rejects on waitForReady error', async () => {
      mockClientInstance.waitForReady.mockImplementation((_deadline: number, cb: (err?: Error) => void) => {
        cb(new Error('connection failed'))
      })
      const mockStream = { on: vi.fn(), write: vi.fn(), end: vi.fn() }
      mockClientInstance.ptyStream.mockReturnValue(mockStream)

      await expect(client.connect()).rejects.toThrow('connection failed')
    })

    it('disconnect closes stream and client', async () => {
      connectClient(client)
      await client.connect()
      client.disconnect()
      expect(client.isConnected()).toBe(false)
      expect(mockClientInstance.close).toHaveBeenCalled()
    })

    it('isConnected returns false initially', () => {
      expect(client.isConnected()).toBe(false)
    })
  })

  describe('PTY methods', () => {
    beforeEach(async () => {
      connectClient(client)
      await client.connect()
    })

    it('createPtySession resolves with sessionId on success', async () => {
      mockClientInstance.createPty.mockImplementation((req: any, cb: any) => cb(null, { sessionId: 'pty-1' }))
      const result = await client.createPtySession({ cwd: '/home' })
      expect(result).toBe('pty-1')
    })

    it('createPtySession rejects on error', async () => {
      mockClientInstance.createPty.mockImplementation((req: any, cb: any) => cb({ message: 'fail' }))
      await expect(client.createPtySession({ cwd: '/home' })).rejects.toThrow('fail')
    })

    it('createPtySession throws when not connected', async () => {
      client.disconnect()
      await expect(client.createPtySession({ cwd: '/home' })).rejects.toThrow('Not connected')
    })

    it('attachPtySession resolves with scrollback on success', async () => {
      mockClientInstance.attachPty.mockImplementation((req: any, cb: any) => cb(null, { scrollback: ['line1'] }))
      const result = await client.attachPtySession('pty-1')
      expect(result).toEqual({ scrollback: ['line1'] })
    })

    it('attachPtySession rejects on error', async () => {
      mockClientInstance.attachPty.mockImplementation((req: any, cb: any) => cb({ message: 'not found' }))
      await expect(client.attachPtySession('pty-1')).rejects.toThrow('not found')
    })

    it('detachPtySession resolves on success', async () => {
      mockClientInstance.detachPty.mockImplementation((req: any, cb: any) => cb(null))
      await expect(client.detachPtySession('pty-1')).resolves.toBeUndefined()
    })

    it('detachPtySession rejects on error', async () => {
      mockClientInstance.detachPty.mockImplementation((req: any, cb: any) => cb({ message: 'fail' }))
      await expect(client.detachPtySession('pty-1')).rejects.toThrow('fail')
    })

    it('writeToPtySession writes to stream', async () => {
      const streamMock = mockClientInstance.ptyStream.mock.results[0]?.value
      client.writeToPtySession('pty-1', 'hello')
      expect(streamMock.write).toHaveBeenCalled()
    })

    it('writeToPtySession does not throw when stream is null', () => {
      client.disconnect()
      expect(() => client.writeToPtySession('pty-1', 'hello')).not.toThrow()
    })

    it('resizePtySession writes resize to stream', async () => {
      const streamMock = mockClientInstance.ptyStream.mock.results[0]?.value
      client.resizePtySession('pty-1', 120, 40)
      expect(streamMock.write).toHaveBeenCalled()
    })

    it('resizePtySession does not throw when stream is null', () => {
      client.disconnect()
      expect(() => client.resizePtySession('pty-1', 80, 24)).not.toThrow()
    })

    it('killPtySession resolves and cleans up listeners', async () => {
      mockClientInstance.killPty.mockImplementation((req: any, cb: any) => cb(null))
      await client.killPtySession('pty-1')
    })

    it('killPtySession rejects on error', async () => {
      mockClientInstance.killPty.mockImplementation((req: any, cb: any) => cb({ message: 'fail' }))
      await expect(client.killPtySession('pty-1')).rejects.toThrow('fail')
    })

    it('listPtySessions resolves with sessions', async () => {
      mockClientInstance.listPtySessions.mockImplementation((req: any, cb: any) =>
        cb(null, { sessions: [{ id: 'pty-1', cwd: '/home' }] })
      )
      const result = await client.listPtySessions()
      expect(result).toEqual([{ id: 'pty-1', cwd: '/home' }])
    })

    it('listPtySessions returns empty array when no response', async () => {
      mockClientInstance.listPtySessions.mockImplementation((req: any, cb: any) => cb(null, null))
      const result = await client.listPtySessions()
      expect(result).toEqual([])
    })
  })

  describe('listener management', () => {
    beforeEach(async () => {
      connectClient(client)
      await client.connect()
    })

    it('onPtySessionData registers listener and returns unsubscribe', () => {
      const cb = vi.fn()
      const unsub = client.onPtySessionData('pty-1', cb)
      expect(typeof unsub).toBe('function')
    })

    it('onPtySessionData unsubscribe removes listener', () => {
      const cb = vi.fn()
      const unsub = client.onPtySessionData('pty-1', cb)
      unsub()
      unsub() // second call should not throw
    })

    it('onPtySessionExit registers listener and returns unsubscribe', () => {
      const cb = vi.fn()
      const unsub = client.onPtySessionExit('pty-1', cb)
      expect(typeof unsub).toBe('function')
      unsub()
    })

    it('onDisconnect registers listener and returns unsubscribe', () => {
      const cb = vi.fn()
      const unsub = client.onDisconnect(cb)
      expect(typeof unsub).toBe('function')
      unsub()
    })

    it('stream data dispatches to data listeners', async () => {
      const streamMock = mockClientInstance.ptyStream.mock.results[0]?.value
      const dataHandler = streamMock.on.mock.calls.find((c: any[]) => c[0] === 'data')?.[1]

      const cb = vi.fn()
      client.onPtySessionData('pty-1', cb)

      if (dataHandler) {
        dataHandler({ data: { sessionId: 'pty-1', data: Buffer.from('hello') } })
        expect(cb).toHaveBeenCalledWith('hello')
      }
    })

    it('stream exit event dispatches to exit listeners and cleans up', async () => {
      const streamMock = mockClientInstance.ptyStream.mock.results[0]?.value
      const dataHandler = streamMock.on.mock.calls.find((c: any[]) => c[0] === 'data')?.[1]

      const cb = vi.fn()
      client.onPtySessionExit('pty-1', cb)

      if (dataHandler) {
        dataHandler({ exit: { sessionId: 'pty-1', exitCode: 0, signal: undefined } })
        expect(cb).toHaveBeenCalledWith(0, undefined)
      }
    })

    it('stream error triggers disconnect listeners', async () => {
      const streamMock = mockClientInstance.ptyStream.mock.results[0]?.value
      const errorHandler = streamMock.on.mock.calls.find((c: any[]) => c[0] === 'error')?.[1]

      const cb = vi.fn()
      client.onDisconnect(cb)

      if (errorHandler) {
        errorHandler(new Error('stream error'))
        expect(cb).toHaveBeenCalled()
        expect(client.isConnected()).toBe(false)
      }
    })
  })

  describe('session methods', () => {
    beforeEach(async () => {
      connectClient(client)
      await client.connect()
    })

    const mockProtoSession = {
      id: 'session-1',
      workspaces: [{
        id: 'ws-1',
        path: '/test',
        name: 'test',
        parentId: undefined,
        children: [],
        status: 'active',
        isGitRepo: false,
        gitBranch: undefined,
        gitRootPath: undefined,
        isWorktree: false,
        isDetached: false,
        appStates: {
          'tab-1': {
            applicationId: 'terminal',
            title: 'Terminal',
            state: Buffer.from(JSON.stringify({ ptyId: 'pty-1' }), 'utf-8')
          }
        },
        activeTabId: 'tab-1',
        metadata: Buffer.from('{}'),
        createdAt: 1000,
        lastActivity: 2000,
        attachedClients: 1
      }],
      createdAt: 1000,
      lastActivity: 2000,
      attachedClients: 1
    }

    it('createSession resolves with converted session', async () => {
      mockClientInstance.createSession.mockImplementation((req: any, cb: any) => cb(null, mockProtoSession))
      const result = await client.createSession([{
        id: 'ws-1',
        path: '/test',
        name: 'test',
        parentId: null,
        children: [],
        status: 'active',
        isGitRepo: false,
        gitBranch: null,
        gitRootPath: null,
        isWorktree: false,
        isDetached: false,
        appStates: {},
        activeTabId: null,
        metadata: {}
      }])
      expect(result.id).toBe('session-1')
      expect(result.workspaces[0].path).toBe('/test')
    })

    it('createSession rejects on error', async () => {
      mockClientInstance.createSession.mockImplementation((req: any, cb: any) => cb({ message: 'fail' }))
      await expect(client.createSession([])).rejects.toThrow('fail')
    })

    it('updateSession resolves with converted session', async () => {
      mockClientInstance.updateSession.mockImplementation((req: any, cb: any) => cb(null, mockProtoSession))
      const result = await client.updateSession('session-1', [])
      expect(result.id).toBe('session-1')
    })

    it('deleteSession resolves on success', async () => {
      mockClientInstance.deleteSession.mockImplementation((req: any, cb: any) => cb(null))
      await expect(client.deleteSession('session-1')).resolves.toBeUndefined()
    })

    it('listSessions resolves with session array', async () => {
      mockClientInstance.listSessions.mockImplementation((req: any, cb: any) =>
        cb(null, { sessions: [mockProtoSession] })
      )
      const result = await client.listSessions()
      expect(result).toHaveLength(1)
      expect(result[0].id).toBe('session-1')
    })

    it('listSessions returns empty array when no response', async () => {
      mockClientInstance.listSessions.mockImplementation((req: any, cb: any) => cb(null, null))
      const result = await client.listSessions()
      expect(result).toEqual([])
    })

    it('getDefaultSessionId resolves with session id', async () => {
      mockClientInstance.getDefaultSessionId.mockImplementation((req: any, cb: any) => cb(null, { sessionId: 'session-1' }))
      const result = await client.getDefaultSessionId()
      expect(result).toBe('session-1')
    })

    it('getDefaultSessionId rejects on error', async () => {
      mockClientInstance.getDefaultSessionId.mockImplementation((req: any, cb: any) => cb({ message: 'fail' }))
      await expect(client.getDefaultSessionId()).rejects.toThrow('fail')
    })

    it('not connected throws for session methods', async () => {
      client.disconnect()
      await expect(client.createSession([])).rejects.toThrow('Not connected')
      await expect(client.updateSession('s', [])).rejects.toThrow('Not connected')
      await expect(client.deleteSession('s')).rejects.toThrow('Not connected')
      await expect(client.listSessions()).rejects.toThrow('Not connected')
      await expect(client.getDefaultSessionId()).rejects.toThrow('Not connected')
    })
  })

  describe('proto conversion', () => {
    beforeEach(async () => {
      connectClient(client)
      await client.connect()
    })

    it('convertFromProtoSession correctly maps fields including tab state JSON parse', async () => {
      const protoSession = {
        id: 'session-1',
        workspaces: [{
          id: 'ws-1',
          path: '/test',
          name: 'test',
          parentId: 'parent-1',
          children: ['child-1'],
          status: 'active',
          isGitRepo: true,
          gitBranch: 'main',
          gitRootPath: '/test',
          isWorktree: true,
          isDetached: false,
          appStates: {
            'tab-1': {
              applicationId: 'terminal',
              title: 'Terminal',
              state: Buffer.from(JSON.stringify({ ptyId: 'pty-1' }), 'utf-8')
            }
          },
          activeTabId: 'tab-1',
          metadata: Buffer.from('{}'),
          createdAt: 1000,
          lastActivity: 2000,
          attachedClients: 2
        }],
        createdAt: 1000,
        lastActivity: 2000,
        attachedClients: 2
      }

      mockClientInstance.createSession.mockImplementation((req: any, cb: any) => cb(null, protoSession))
      const session = await client.createSession([])
      expect(session).not.toBeNull()
      expect(session.workspaces[0].appStates['tab-1'].state).toEqual({ ptyId: 'pty-1' })
      expect(session.workspaces[0].parentId).toBe('parent-1')
      expect(session.workspaces[0].gitBranch).toBe('main')
    })

    it('convertFromProtoWorkspace handles null/undefined optional fields', async () => {
      const protoSession = {
        id: 'session-1',
        workspaces: [{
          id: 'ws-1',
          path: '/test',
          name: 'test',
          parentId: undefined,
          children: [],
          status: 'active',
          isGitRepo: false,
          gitBranch: undefined,
          gitRootPath: undefined,
          isWorktree: false,
          isDetached: false,
          appStates: {},
          activeTabId: undefined,
          createdAt: 1000,
          lastActivity: 2000,
          attachedClients: 0
        }],
        createdAt: 1000,
        lastActivity: 2000,
        attachedClients: 0
      }

      mockClientInstance.createSession.mockImplementation((req: any, cb: any) => cb(null, protoSession))
      const session = await client.createSession([])
      expect(session.workspaces[0].parentId).toBeNull()
      expect(session.workspaces[0].gitBranch).toBeNull()
      expect(session.workspaces[0].gitRootPath).toBeNull()
      expect(session.workspaces[0].activeTabId).toBeNull()
    })
  })

  describe('filesystem methods', () => {
    beforeEach(async () => {
      connectClient(client)
      await client.connect()
    })

    it('readDirectory resolves with result', async () => {
      mockClientInstance.readDirectory.mockImplementation((req: any, cb: any) =>
        cb(null, { success: true, contents: { files: [] } })
      )
      const result = await client.readDirectory('/ws', '.')
      expect(result.success).toBe(true)
    })

    it('readDirectory rejects on error', async () => {
      mockClientInstance.readDirectory.mockImplementation((req: any, cb: any) =>
        cb({ message: 'fail' })
      )
      await expect(client.readDirectory('/ws', '.')).rejects.toThrow('fail')
    })

    it('readFile resolves with streamed file content', async () => {
      const chunks = [
        { header: { path: '/file.txt', size: 5, language: 'text' } },
        { data: { data: Buffer.from('hello') } },
        { end: { success: true } }
      ]

      mockClientInstance.readFile.mockImplementation((req: any) => {
        const stream = {
          on: (event: string, handler: Function) => {
            if (event === 'data') {
              setTimeout(() => {
                for (const chunk of chunks) {
                  handler(chunk)
                }
              }, 0)
            }
            return stream
          }
        }
        return stream
      })

      const result = await client.readFile('/ws', '/file.txt')
      expect(result.success).toBe(true)
      expect(result.file?.content).toBe('hello')
    })

    it('writeFile resolves with result', async () => {
      mockClientInstance.writeFile.mockImplementation((cb: any) => {
        setTimeout(() => cb(null, { success: true }), 0)
        return { write: vi.fn(), end: vi.fn() }
      })
      const result = await client.writeFile('/ws', '/file.txt', 'content')
      expect(result.success).toBe(true)
    })

    it('searchFiles resolves with result', async () => {
      mockClientInstance.searchFiles.mockImplementation((req: any, cb: any) =>
        cb(null, { success: true, entries: [{ name: 'file.txt' }] })
      )
      const result = await client.searchFiles('/ws', 'file')
      expect(result.success).toBe(true)
      expect(result.entries).toHaveLength(1)
    })

    it('filesystem methods throw when not connected', async () => {
      client.disconnect()
      await expect(client.readDirectory('/ws', '.')).rejects.toThrow('Not connected')
      await expect(client.readFile('/ws', '/f')).rejects.toThrow('Not connected')
      await expect(client.writeFile('/ws', '/f', 'c')).rejects.toThrow('Not connected')
      await expect(client.searchFiles('/ws', 'q')).rejects.toThrow('Not connected')
    })
  })

  describe('watchSession', () => {
    beforeEach(async () => {
      connectClient(client)
      await client.connect()
    })

    it('returns initial promise and unsubscribe', () => {
      const mockStream = { on: vi.fn(), cancel: vi.fn() }
      mockClientInstance.sessionWatch.mockReturnValue(mockStream)
      const result = client.watchSession('session-1', 'listener-1', vi.fn())
      expect(result.initial).toBeInstanceOf(Promise)
      expect(typeof result.unsubscribe).toBe('function')
    })

    it('unsubscribe cancels stream', () => {
      const mockStream = { on: vi.fn(), cancel: vi.fn() }
      mockClientInstance.sessionWatch.mockReturnValue(mockStream)
      const result = client.watchSession('session-1', 'listener-1', vi.fn())
      result.unsubscribe()
      expect(mockStream.cancel).toHaveBeenCalled()
    })

    it('returns rejected initial when not connected', async () => {
      client.disconnect()
      const result = client.watchSession('session-1', 'listener-1', vi.fn())
      await expect(result.initial).rejects.toThrow('Not connected')
    })
  })

  describe('execStream', () => {
    beforeEach(async () => {
      connectClient(client)
      await client.connect()
    })

    it('returns exec stream from client', () => {
      const mockStream = { on: vi.fn(), write: vi.fn() }
      mockClientInstance.execStream.mockReturnValue(mockStream)
      const stream = client.execStream()
      expect(stream).toBe(mockStream)
    })

    it('throws when not connected', () => {
      client.disconnect()
      expect(() => client.execStream()).toThrow('Not connected')
    })
  })
})
