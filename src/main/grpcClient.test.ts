import { describe, it, expect, vi, beforeEach } from 'vitest'

const mocks = vi.hoisted(() => {
  const mockClientInstance = {
    waitForReady: vi.fn(),
    createPty: vi.fn(),
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

vi.mock('./socketPath', () => ({
  getDefaultSocketPath: vi.fn().mockReturnValue('/tmp/test.sock')
}))

import { GrpcDaemonClient } from './grpcClient'

const { mockClientInstance } = mocks

// Helper to create a mock per-session stream
function makeMockSessionStream() {
  return {
    on: vi.fn(),
    write: vi.fn(),
    end: vi.fn(),
    cancel: vi.fn(),
    removeListener: vi.fn()
  }
}

// Helper to make the client connected
function connectClient(client: GrpcDaemonClient): void {
  mockClientInstance.ptyStream.mockReturnValue(makeMockSessionStream())

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

    it('openPtyStream creates a PtyStream with handle and sessionId', () => {
      const mockStream = makeMockSessionStream()
      mockClientInstance.ptyStream.mockReturnValue(mockStream)
      mockStream.on.mockReturnValue(mockStream)

      const ptyStream = client.openPtyStream('pty-1')
      expect(ptyStream.handle).toBeDefined()
      expect(ptyStream.sessionId).toBe('pty-1')
      // Should have sent a start message
      expect(mockStream.write).toHaveBeenCalledWith({ start: { sessionId: 'pty-1' } })
    })

    it('PtyStream.write sends write message to stream', () => {
      const mockStream = makeMockSessionStream()
      mockClientInstance.ptyStream.mockReturnValue(mockStream)
      mockStream.on.mockReturnValue(mockStream)

      const ptyStream = client.openPtyStream('pty-1')
      ptyStream.write('hello')
      expect(mockStream.write).toHaveBeenCalledWith({
        write: { data: Buffer.from('hello', 'utf-8') }
      })
    })

    it('PtyStream.resize sends resize message to stream', () => {
      const mockStream = makeMockSessionStream()
      mockClientInstance.ptyStream.mockReturnValue(mockStream)
      mockStream.on.mockReturnValue(mockStream)

      const ptyStream = client.openPtyStream('pty-1')
      ptyStream.resize(120, 40)
      expect(mockStream.write).toHaveBeenCalledWith({
        resize: { cols: 120, rows: 40 }
      })
    })

    it('PtyStream.close ends the stream', () => {
      const mockStream = makeMockSessionStream()
      mockClientInstance.ptyStream.mockReturnValue(mockStream)
      mockStream.on.mockReturnValue(mockStream)

      const ptyStream = client.openPtyStream('pty-1')
      ptyStream.close()
      expect(mockStream.end).toHaveBeenCalled()
    })

    it('PtyStream.close is idempotent', () => {
      const mockStream = makeMockSessionStream()
      mockClientInstance.ptyStream.mockReturnValue(mockStream)
      mockStream.on.mockReturnValue(mockStream)

      const ptyStream = client.openPtyStream('pty-1')
      ptyStream.close()
      ptyStream.close()
      expect(mockStream.end).toHaveBeenCalledTimes(1)
    })

    it.each(['write', 'resize'] as const)('PtyStream.%s is no-op after close', (method) => {
      const mockStream = makeMockSessionStream()
      mockClientInstance.ptyStream.mockReturnValue(mockStream)
      mockStream.on.mockReturnValue(mockStream)

      const ptyStream = client.openPtyStream('pty-1')
      ptyStream.close()
      mockStream.write.mockClear()

      if (method === 'write') ptyStream.write('data')
      else ptyStream.resize(80, 24)

      expect(mockStream.write).not.toHaveBeenCalled()
    })

    it.each(['write', 'resize'] as const)('PtyStream.%s catches stream errors', (method) => {
      const mockStream = makeMockSessionStream()
      mockClientInstance.ptyStream.mockReturnValue(mockStream)
      mockStream.on.mockReturnValue(mockStream)

      const ptyStream = client.openPtyStream('pty-1')
      mockStream.write.mockImplementation(() => { throw new Error('broken pipe') })

      if (method === 'write') expect(() => ptyStream.write('data')).not.toThrow()
      else expect(() => ptyStream.resize(80, 24)).not.toThrow()
    })

    it('PtyStream.onResize receives resize events from stream', () => {
      const mockStream = makeMockSessionStream()
      mockClientInstance.ptyStream.mockReturnValue(mockStream)

      const dataHandlers: Function[] = []
      mockStream.on.mockImplementation((event: string, handler: Function) => {
        if (event === 'data') dataHandlers.push(handler)
        return mockStream
      })

      const ptyStream = client.openPtyStream('pty-1')
      const cb = vi.fn()
      ptyStream.onResize(cb)

      dataHandlers[0]?.({ resize: { cols: 120, rows: 40 } })
      expect(cb).toHaveBeenCalledWith(120, 40)
    })

    it('killPtySession resolves on success', async () => {
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

  describe('PtyStream callbacks', () => {
    beforeEach(async () => {
      connectClient(client)
      await client.connect()
    })

    it('onDisconnect registers listener and returns unsubscribe', () => {
      const cb = vi.fn()
      const unsub = client.onDisconnect(cb)
      expect(typeof unsub).toBe('function')
      unsub()
    })

    it('PtyStream.onData receives data from stream', () => {
      const mockStream = makeMockSessionStream()
      mockClientInstance.ptyStream.mockReturnValue(mockStream)

      const dataHandlers: Function[] = []
      mockStream.on.mockImplementation((event: string, handler: Function) => {
        if (event === 'data') dataHandlers.push(handler)
        return mockStream
      })

      const ptyStream = client.openPtyStream('pty-1')
      const cb = vi.fn()
      ptyStream.onData(cb)

      // Simulate stream data event
      dataHandlers[0]?.({ data: { data: Buffer.from('hello') } })
      expect(cb).toHaveBeenCalledWith('hello')
    })

    it('PtyStream.onExit receives exit from stream', () => {
      const mockStream = makeMockSessionStream()
      mockClientInstance.ptyStream.mockReturnValue(mockStream)

      const dataHandlers: Function[] = []
      mockStream.on.mockImplementation((event: string, handler: Function) => {
        if (event === 'data') dataHandlers.push(handler)
        return mockStream
      })

      const ptyStream = client.openPtyStream('pty-1')
      const cb = vi.fn()
      ptyStream.onExit(cb)

      // Simulate stream exit event
      dataHandlers[0]?.({ exit: { exitCode: 0, signal: undefined } })
      expect(cb).toHaveBeenCalledWith(0, undefined)
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
        lastActivity: 2000
      }],
      createdAt: 1000,
      lastActivity: 2000
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
          lastActivity: 2000
        }],
        createdAt: 1000,
        lastActivity: 2000
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
          lastActivity: 2000
        }],
        createdAt: 1000,
        lastActivity: 2000
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

    it('readFile rejects when stream returns success=false', async () => {
      mockClientInstance.readFile.mockImplementation(() => {
        const stream = {
          on: (event: string, handler: Function) => {
            if (event === 'data') {
              setTimeout(() => handler({ end: { success: false, error: 'not found' } }), 0)
            }
            return stream
          }
        }
        return stream
      })

      const result = await client.readFile('/ws', '/missing.txt')
      expect(result.success).toBe(false)
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

    it('resolves initial with session on first data event', async () => {
      const mockStream = { on: vi.fn(), cancel: vi.fn() }
      mockClientInstance.sessionWatch.mockReturnValue(mockStream)

      const handlers: Record<string, Function> = {}
      mockStream.on.mockImplementation((event: string, handler: Function) => {
        handlers[event] = handler
        return mockStream
      })

      const onUpdate = vi.fn()
      const result = client.watchSession('session-1', 'listener-1', onUpdate)

      const mockProto = { id: 'session-1', workspaces: [], createdAt: 1000, lastActivity: 2000 }
      handlers.data({ session: mockProto })

      const session = await result.initial
      expect(session.id).toBe('session-1')
    })

    it('calls onUpdate for subsequent data events after initial', async () => {
      const mockStream = { on: vi.fn(), cancel: vi.fn() }
      mockClientInstance.sessionWatch.mockReturnValue(mockStream)

      const handlers: Record<string, Function> = {}
      mockStream.on.mockImplementation((event: string, handler: Function) => {
        handlers[event] = handler
        return mockStream
      })

      const onUpdate = vi.fn()
      const result = client.watchSession('session-1', 'listener-1', onUpdate)

      const mockProto = { id: 'session-1', workspaces: [], createdAt: 1000, lastActivity: 2000 }
      handlers.data({ session: mockProto }) // first = initial
      await result.initial

      handlers.data({ session: mockProto }) // second = onUpdate
      expect(onUpdate).toHaveBeenCalledTimes(1)
    })

    it('calls onError when stream errors after initial', async () => {
      const mockStream = { on: vi.fn(), cancel: vi.fn() }
      mockClientInstance.sessionWatch.mockReturnValue(mockStream)

      const handlers: Record<string, Function> = {}
      mockStream.on.mockImplementation((event: string, handler: Function) => {
        handlers[event] = handler
        return mockStream
      })

      const onUpdate = vi.fn()
      const onError = vi.fn()
      const result = client.watchSession('session-1', 'listener-1', onUpdate, onError)

      const mockProto = { id: 'session-1', workspaces: [], createdAt: 1000, lastActivity: 2000 }
      handlers.data({ session: mockProto })
      await result.initial

      handlers.error(new Error('stream broken'))
      expect(onError).toHaveBeenCalledWith(expect.any(Error))
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
