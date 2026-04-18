/* eslint-disable custom/no-string-literal-comparison -- test fixtures compare literal tokens */
import { describe, it, expect, vi, beforeEach } from 'vitest'

const mocks = vi.hoisted(() => {
  const mockClientInstance = {
    waitForReady: vi.fn<(...args: any[]) => any>(),
    createPty: vi.fn<(...args: any[]) => any>(),
    killPty: vi.fn<(...args: any[]) => any>(),
    listPtySessions: vi.fn<(...args: any[]) => any>(),
    shutdown: vi.fn<(...args: any[]) => any>(),
    lockSession: vi.fn<(...args: any[]) => any>(),
    unlockSession: vi.fn<(...args: any[]) => any>(),
    forceUnlockSession: vi.fn<(...args: any[]) => any>(),
    ptyStream: vi.fn<(...args: any[]) => any>(),
    execStream: vi.fn<(...args: any[]) => any>(),
    updateSession: vi.fn<(...args: any[]) => any>(),
    sessionWatch: vi.fn<(...args: any[]) => any>(),
    readDirectory: vi.fn<(...args: any[]) => any>(),
    readFile: vi.fn<(...args: any[]) => any>(),
    writeFile: vi.fn<(...args: any[]) => any>(),
    searchFiles: vi.fn<(...args: any[]) => any>(),
    close: vi.fn<() => void>()
  }
  return { mockClientInstance }
})

vi.mock('@grpc/grpc-js', () => {
  class MockMetadata {
    set = vi.fn<(...args: any[]) => void>()
  }
  return {
    credentials: {
      createInsecure: vi.fn<() => string>().mockReturnValue('insecure-creds')
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
    TreeTermDaemonClient: function TreeTermDaemonClient() { Object.assign(this, mocks.mockClientInstance) }
  }
})

vi.mock('fs', () => ({
  existsSync: vi.fn<(...args: any[]) => boolean>().mockReturnValue(true),
  openSync: vi.fn<(...args: any[]) => number>().mockReturnValue(3)
}))

vi.mock('child_process', () => ({
  spawn: vi.fn<(...args: any[]) => any>().mockReturnValue({ unref: vi.fn<() => void>(), pid: 1234 })
}))

vi.mock('electron', () => ({
  app: {
    isPackaged: false,
    getPath: vi.fn<(...args: any[]) => string>().mockReturnValue('/tmp')
  }
}))

vi.mock('./socketPath', () => ({
  getDefaultSocketPath: vi.fn<() => string>().mockReturnValue('/tmp/test.sock')
}))

import { GrpcDaemonClient } from './grpcClient'

const { mockClientInstance } = mocks

// Helper to create a mock per-session stream
function makeMockSessionStream() {
  return {
    on: vi.fn<(...args: any[]) => any>(),
    write: vi.fn<(...args: any[]) => any>(),
    end: vi.fn<() => void>(),
    cancel: vi.fn<() => void>(),
    removeListener: vi.fn<(...args: any[]) => void>()
  }
}

// Helper to make the client connected
function connectClient(): void {
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
      connectClient()
      await client.connect()
      expect(client.isConnected()).toBe(true)
    })

    it('connect throws when socket does not exist', async () => {
      const fs = await import('fs')
      vi.mocked(fs.existsSync).mockReturnValueOnce(false)

      await expect(client.connect()).rejects.toThrow('Daemon socket not found')
    })

    it('connect returns immediately if already connected', async () => {
      connectClient()
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
      connectClient()
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
      connectClient()
      await client.connect()
    })

    it('createPtySession resolves with sessionId on success', async () => {
      mockClientInstance.createPty.mockImplementation((_req: any, cb: (err: any, res: any) => void) => { cb(null, { sessionId: 'pty-1' }); })
      const result = await client.createPtySession({ cwd: '/home' })
      expect(result).toBe('pty-1')
    })

    it('createPtySession rejects on error', async () => {
      mockClientInstance.createPty.mockImplementation((_req: any, cb: (err: any, res: any) => void) => { cb({ message: 'fail' }, null); })
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

      const ptyStream = client.openPtyStream('handle-1', 'pty-1', vi.fn<(...args: any[]) => void>())
      expect(ptyStream.handle).toBeDefined()
      expect(ptyStream.sessionId).toBe('pty-1')
      // Should have sent a start message
      expect(mockStream.write).toHaveBeenCalledWith({ start: { sessionId: 'pty-1' } })
    })

    it('PtyStream.write sends write message and resolves when the stream callback fires', async () => {
      const mockStream = makeMockSessionStream()
      mockClientInstance.ptyStream.mockReturnValue(mockStream)
      mockStream.on.mockReturnValue(mockStream)

      const ptyStream = client.openPtyStream('handle-1', 'pty-1', vi.fn<(...args: any[]) => void>())

      let capturedCb: ((err: Error | null) => void) | undefined
      mockStream.write.mockImplementation((_msg: unknown, cb: (err: Error | null) => void) => {
        capturedCb = cb
      })

      const writePromise = ptyStream.write('hello')
      expect(mockStream.write).toHaveBeenCalledWith(
        { write: { data: Buffer.from('hello', 'utf-8') } },
        expect.any(Function)
      )

      expect(capturedCb).toBeDefined()
      capturedCb?.(null)
      await expect(writePromise).resolves.toBeUndefined()
    })

    it('PtyStream.write rejects when the stream callback reports an error', async () => {
      const mockStream = makeMockSessionStream()
      mockClientInstance.ptyStream.mockReturnValue(mockStream)
      mockStream.on.mockReturnValue(mockStream)

      const ptyStream = client.openPtyStream('handle-1', 'pty-1', vi.fn<(...args: any[]) => void>())

      mockStream.write.mockImplementation((_msg: unknown, cb: (err: Error | null) => void) => {
        cb(new Error('grpc write failed'))
      })

      await expect(ptyStream.write('hello')).rejects.toThrow('grpc write failed')
    })

    it('PtyStream.write rejects pending writes when the stream errors', async () => {
      const mockStream = makeMockSessionStream()
      mockClientInstance.ptyStream.mockReturnValue(mockStream)
      // Capture the 'error' handler so we can simulate a stream-level error.
      let errorHandler: ((err: Error) => void) | undefined
      mockStream.on.mockImplementation((event: string, handler: (...args: any[]) => any) => {
        if (event === 'error') errorHandler = handler as (err: Error) => void
        return mockStream
      })

      const ptyStream = client.openPtyStream('handle-1', 'pty-1', vi.fn<(...args: any[]) => void>())

      // Write that never gets a per-message callback — the stream will error mid-flight.
      mockStream.write.mockImplementation(() => { /* no callback, no throw */ })

      const writePromise = ptyStream.write('hello')
      expect(errorHandler).toBeDefined()
      errorHandler?.(new Error('stream died'))
      await expect(writePromise).rejects.toThrow('stream died')
    })

    it('PtyStream.resize sends resize message to stream', () => {
      const mockStream = makeMockSessionStream()
      mockClientInstance.ptyStream.mockReturnValue(mockStream)
      mockStream.on.mockReturnValue(mockStream)

      const ptyStream = client.openPtyStream('handle-1', 'pty-1', vi.fn<(...args: any[]) => void>())
      ptyStream.resize(120, 40)
      expect(mockStream.write).toHaveBeenCalledWith({
        resize: { cols: 120, rows: 40 }
      })
    })

    it('PtyStream.close ends the stream', () => {
      const mockStream = makeMockSessionStream()
      mockClientInstance.ptyStream.mockReturnValue(mockStream)
      mockStream.on.mockReturnValue(mockStream)

      const ptyStream = client.openPtyStream('handle-1', 'pty-1', vi.fn<(...args: any[]) => void>())
      ptyStream.close()
      expect(mockStream.end).toHaveBeenCalled()
    })

    it('PtyStream.close is idempotent', () => {
      const mockStream = makeMockSessionStream()
      mockClientInstance.ptyStream.mockReturnValue(mockStream)
      mockStream.on.mockReturnValue(mockStream)

      const ptyStream = client.openPtyStream('handle-1', 'pty-1', vi.fn<(...args: any[]) => void>())
      ptyStream.close()
      ptyStream.close()
      expect(mockStream.end).toHaveBeenCalledTimes(1)
    })

    it('PtyStream.write rejects after close without calling stream.write', async () => {
      const mockStream = makeMockSessionStream()
      mockClientInstance.ptyStream.mockReturnValue(mockStream)
      mockStream.on.mockReturnValue(mockStream)

      const ptyStream = client.openPtyStream('handle-1', 'pty-1', vi.fn<(...args: any[]) => void>())
      ptyStream.close()
      mockStream.write.mockClear()

      await expect(ptyStream.write('data')).rejects.toThrow('pty stream closed')
      expect(mockStream.write).not.toHaveBeenCalled()
    })

    it('PtyStream.resize is a no-op after close', () => {
      const mockStream = makeMockSessionStream()
      mockClientInstance.ptyStream.mockReturnValue(mockStream)
      mockStream.on.mockReturnValue(mockStream)

      const ptyStream = client.openPtyStream('handle-1', 'pty-1', vi.fn<(...args: any[]) => void>())
      ptyStream.close()
      mockStream.write.mockClear()

      ptyStream.resize(80, 24)
      expect(mockStream.write).not.toHaveBeenCalled()
    })

    it('PtyStream.write rejects when stream.write throws synchronously', async () => {
      const mockStream = makeMockSessionStream()
      mockClientInstance.ptyStream.mockReturnValue(mockStream)
      mockStream.on.mockReturnValue(mockStream)

      const ptyStream = client.openPtyStream('handle-1', 'pty-1', vi.fn<(...args: any[]) => void>())
      mockStream.write.mockImplementation(() => { throw new Error('broken pipe') })

      await expect(ptyStream.write('data')).rejects.toThrow('broken pipe')
    })

    it('PtyStream.resize swallows synchronous stream.write throws', () => {
      const mockStream = makeMockSessionStream()
      mockClientInstance.ptyStream.mockReturnValue(mockStream)
      mockStream.on.mockReturnValue(mockStream)

      const ptyStream = client.openPtyStream('handle-1', 'pty-1', vi.fn<(...args: any[]) => void>())
      mockStream.write.mockImplementation(() => { throw new Error('broken pipe') })

      expect(() => { ptyStream.resize(80, 24); }).not.toThrow()
    })

    it('PtyStream receives resize events from stream via constructor callback', () => {
      const mockStream = makeMockSessionStream()
      mockClientInstance.ptyStream.mockReturnValue(mockStream)

      const dataHandlers: Array<(data: any) => void> = []
      mockStream.on.mockImplementation((event: string, handler: (data: any) => void) => {
        if (event === 'data') dataHandlers.push(handler)
        return mockStream
      })

      const cb = vi.fn<(...args: any[]) => void>()
      client.openPtyStream('handle-1', 'pty-1', cb)

      dataHandlers[0]?.({ resize: { cols: 120, rows: 40 } })
      expect(cb).toHaveBeenCalledWith({ type: 'resize', cols: 120, rows: 40 })
    })

    it('killPtySession resolves on success', async () => {
      mockClientInstance.killPty.mockImplementation((_req: any, cb: (err: any) => void) => { cb(null); })
      await client.killPtySession('pty-1')
    })

    it('killPtySession rejects on error', async () => {
      mockClientInstance.killPty.mockImplementation((_req: any, cb: (err: any) => void) => { cb({ message: 'fail' }); })
      await expect(client.killPtySession('pty-1')).rejects.toThrow('fail')
    })

    it('listPtySessions resolves with sessions', async () => {
      mockClientInstance.listPtySessions.mockImplementation((_req: any, cb: (err: any, res: any) => void) =>
        { cb(null, { sessions: [{ id: 'pty-1', cwd: '/home' }] }); }
      )
      const result = await client.listPtySessions()
      expect(result).toEqual([{ id: 'pty-1', cwd: '/home' }])
    })

    it('listPtySessions returns empty array when no response', async () => {
      mockClientInstance.listPtySessions.mockImplementation((_req: any, cb: (err: any, res: any) => void) => { cb(null, null); })
      const result = await client.listPtySessions()
      expect(result).toEqual([])
    })
  })

  describe('PtyStream callbacks', () => {
    beforeEach(async () => {
      connectClient()
      await client.connect()
    })

    it('onDisconnect registers listener and returns unsubscribe', () => {
      const cb = vi.fn<() => void>()
      const unsub = client.onDisconnect(cb)
      expect(typeof unsub).toBe('function')
      unsub()
    })

    it('PtyStream receives data from stream via constructor callback', () => {
      const mockStream = makeMockSessionStream()
      mockClientInstance.ptyStream.mockReturnValue(mockStream)

      const dataHandlers: Array<(data: any) => void> = []
      mockStream.on.mockImplementation((event: string, handler: (data: any) => void) => {
        if (event === 'data') dataHandlers.push(handler)
        return mockStream
      })

      const cb = vi.fn<(...args: any[]) => void>()
      client.openPtyStream('handle-1', 'pty-1', cb)

      const expectedData = Buffer.from('hello')
      dataHandlers[0]?.({ data: { data: expectedData } })
      expect(cb).toHaveBeenCalledWith({ type: 'data', data: expectedData })
    })

    it('PtyStream receives exit from stream via constructor callback', () => {
      const mockStream = makeMockSessionStream()
      mockClientInstance.ptyStream.mockReturnValue(mockStream)

      const dataHandlers: Array<(data: any) => void> = []
      mockStream.on.mockImplementation((event: string, handler: (data: any) => void) => {
        if (event === 'data') dataHandlers.push(handler)
        return mockStream
      })

      const cb = vi.fn<(...args: any[]) => void>()
      client.openPtyStream('handle-1', 'pty-1', cb)

      // Simulate stream exit event
      dataHandlers[0]?.({ exit: { exitCode: 0, signal: undefined } })
      expect(cb).toHaveBeenCalledWith({ type: 'exit', exitCode: 0, signal: undefined })
    })
  })

  describe('session methods', () => {
    beforeEach(async () => {
      connectClient()
      await client.connect()
    })

    const mockProtoSession = {
      id: 'session-1',
      workspaces: [{
        id: 'ws-1',
        path: '/test',
        name: 'test',
        parentId: undefined,
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

    it('updateSession resolves with converted session', async () => {
      mockClientInstance.updateSession.mockImplementation((_req: any, cb: (err: any, res: any) => void) => { cb(null, mockProtoSession); })
      const result = await client.updateSession([])
      expect(result.id).toBe('session-1')
    })

    it('not connected throws for session methods', async () => {
      client.disconnect()
      await expect(client.updateSession([])).rejects.toThrow('Not connected')
    })
  })

  describe('proto conversion', () => {
    beforeEach(async () => {
      connectClient()
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

      mockClientInstance.updateSession.mockImplementation((_req: any, cb: (err: any, res: any) => void) => { cb(null, protoSession); })
      const session = await client.updateSession([])
      expect(session).not.toBeNull()
      expect(session.workspaces[0]!.appStates['tab-1']!.state).toEqual({ ptyId: 'pty-1' })
      expect(session.workspaces[0]!.parentId).toBe('parent-1')
      expect(session.workspaces[0]!.gitBranch).toBe('main')
    })

    it('convertFromProtoWorkspace handles null/undefined optional fields', async () => {
      const protoSession = {
        id: 'session-1',
        workspaces: [{
          id: 'ws-1',
          path: '/test',
          name: 'test',
          parentId: undefined,
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

      mockClientInstance.updateSession.mockImplementation((_req: any, cb: (err: any, res: any) => void) => { cb(null, protoSession); })
      const session = await client.updateSession([])
      expect(session.workspaces[0]!.parentId).toBeNull()
      expect(session.workspaces[0]!.gitBranch).toBeNull()
      expect(session.workspaces[0]!.gitRootPath).toBeNull()
      expect(session.workspaces[0]!.activeTabId).toBeNull()
    })
  })

  describe('filesystem methods', () => {
    beforeEach(async () => {
      connectClient()
      await client.connect()
    })

    it('readDirectory resolves with result', async () => {
      mockClientInstance.readDirectory.mockImplementation((_req: any, cb: (err: any, res: any) => void) =>
        { cb(null, { success: true, contents: { files: [] } }); }
      )
      const result = await client.readDirectory('/ws', '.')
      expect(result.success).toBe(true)
    })

    it('readDirectory rejects on error', async () => {
      mockClientInstance.readDirectory.mockImplementation((_req: any, cb: (err: any) => void) =>
        { cb({ message: 'fail' }); }
      )
      await expect(client.readDirectory('/ws', '.')).rejects.toThrow('fail')
    })

    it('readFile resolves with streamed file content', async () => {
      const chunks = [
        { header: { path: '/file.txt', size: 5, language: 'text' } },
        { data: { data: Buffer.from('hello') } },
        { end: { success: true } }
      ]

      mockClientInstance.readFile.mockImplementation(() => {
        const stream = {
          on: (event: string, handler: (data: any) => void) => {
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
      expect(result).toMatchObject({ success: true, file: { content: 'hello' } })
    })

    it('writeFile resolves with result', async () => {
      mockClientInstance.writeFile.mockImplementation((cb: (err: any, res: any) => void) => {
        setTimeout(() => { cb(null, { success: true }); }, 0)
        return { write: vi.fn<(...args: any[]) => void>(), end: vi.fn<() => void>() }
      })
      const result = await client.writeFile('/ws', '/file.txt', 'content')
      expect(result.success).toBe(true)
    })

    it('searchFiles resolves with result', async () => {
      mockClientInstance.searchFiles.mockImplementation((_req: any, cb: (err: any, res: any) => void) =>
        { cb(null, { success: true, entries: [{ name: 'file.txt' }] }); }
      )
      const result = await client.searchFiles('/ws', 'file')
      // eslint-disable-next-line @typescript-eslint/no-unsafe-assignment
      expect(result).toMatchObject({ success: true, entries: expect.arrayContaining([expect.any(Object) as unknown]) })
    })

    it('readFile rejects when stream returns success=false', async () => {
      mockClientInstance.readFile.mockImplementation(() => {
        const stream = {
          on: (event: string, handler: (data: any) => void) => {
            if (event === 'data') {
              setTimeout(() => { handler({ end: { success: false, error: 'not found' } }); }, 0)
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
      connectClient()
      await client.connect()
    })

    it('returns initial promise and unsubscribe', () => {
      const mockStream = { on: vi.fn<(...args: any[]) => any>(), cancel: vi.fn<() => void>() }
      mockClientInstance.sessionWatch.mockReturnValue(mockStream)
      const result = client.watchSession('listener-1', vi.fn<(...args: any[]) => void>())
      expect(result.initial).toBeInstanceOf(Promise)
      expect(typeof result.unsubscribe).toBe('function')
    })

    it('unsubscribe cancels stream', () => {
      const mockStream = { on: vi.fn<(...args: any[]) => any>(), cancel: vi.fn<() => void>() }
      mockClientInstance.sessionWatch.mockReturnValue(mockStream)
      const result = client.watchSession('listener-1', vi.fn<(...args: any[]) => void>())
      result.unsubscribe()
      expect(mockStream.cancel).toHaveBeenCalled()
    })

    it('resolves initial with session on first data event', async () => {
      const mockStream = { on: vi.fn<(...args: any[]) => any>(), cancel: vi.fn<() => void>() }
      mockClientInstance.sessionWatch.mockReturnValue(mockStream)

      const handlers: Record<string, (...args: any[]) => void> = {}
      mockStream.on.mockImplementation((event: string, handler: (...args: any[]) => void) => {
        handlers[event] = handler
        return mockStream
      })

      const onUpdate = vi.fn<(...args: any[]) => void>()
      const result = client.watchSession('listener-1', onUpdate)

      const mockProto = { id: 'session-1', workspaces: [], createdAt: 1000, lastActivity: 2000 }
      handlers['data']!({ session: mockProto })

      const session = await result.initial
      expect(session.id).toBe('session-1')
    })

    it('calls onUpdate for subsequent data events after initial', async () => {
      const mockStream = { on: vi.fn<(...args: any[]) => any>(), cancel: vi.fn<() => void>() }
      mockClientInstance.sessionWatch.mockReturnValue(mockStream)

      const handlers: Record<string, (...args: any[]) => void> = {}
      mockStream.on.mockImplementation((event: string, handler: (...args: any[]) => void) => {
        handlers[event] = handler
        return mockStream
      })

      const onUpdate = vi.fn<(...args: any[]) => void>()
      const result = client.watchSession('listener-1', onUpdate)

      const mockProto = { id: 'session-1', workspaces: [], createdAt: 1000, lastActivity: 2000 }
      handlers['data']!({ session: mockProto }) // first = initial
      await result.initial

      handlers['data']!({ session: mockProto }) // second = onUpdate
      expect(onUpdate).toHaveBeenCalledTimes(1)
    })

    it('calls onError when stream errors after initial', async () => {
      const mockStream = { on: vi.fn<(...args: any[]) => any>(), cancel: vi.fn<() => void>() }
      mockClientInstance.sessionWatch.mockReturnValue(mockStream)

      const handlers: Record<string, (...args: any[]) => void> = {}
      mockStream.on.mockImplementation((event: string, handler: (...args: any[]) => void) => {
        handlers[event] = handler
        return mockStream
      })

      const onUpdate = vi.fn<(...args: any[]) => void>()
      const onError = vi.fn<(err: Error) => void>()
      const result = client.watchSession('listener-1', onUpdate, onError)

      const mockProto = { id: 'session-1', workspaces: [], createdAt: 1000, lastActivity: 2000 }
      handlers['data']!({ session: mockProto })
      await result.initial

      handlers['error']!(new Error('stream broken'))
      expect(onError).toHaveBeenCalledWith(expect.any(Error))
    })

    it('returns rejected initial when not connected', async () => {
      client.disconnect()
      const result = client.watchSession('listener-1', vi.fn<(...args: any[]) => void>())
      await expect(result.initial).rejects.toThrow('Not connected')
    })
  })

  describe('lockSession', () => {
    beforeEach(async () => {
      connectClient()
      await client.connect()
    })

    it('resolves with acquired and session on success', async () => {
      const mockProtoSession = {
        id: 'session-1',
        workspaces: [],
        createdAt: 100,
        lastActivity: 200,
        version: 1,
        lock: { acquiredAt: 1000, expiresAt: 2000 }
      }
      mockClientInstance.lockSession.mockImplementation((_req: any, cb: (err: any, res: any) => void) => {
        cb(null, { acquired: true, session: mockProtoSession })
      })
      const result = await client.lockSession()
      expect(result.acquired).toBe(true)
      expect(result.session.id).toBe('session-1')
      expect(result.session.lock).toEqual({ acquiredAt: 1000, expiresAt: 2000 })
    })

    it('rejects when response.session is missing', async () => {
      mockClientInstance.lockSession.mockImplementation((_req: any, cb: (err: any, res: any) => void) => {
        cb(null, { acquired: false, session: null })
      })
      await expect(client.lockSession()).rejects.toThrow('LockSession response missing session')
    })

    it('rejects on error', async () => {
      mockClientInstance.lockSession.mockImplementation((_req: any, cb: (err: any) => void) => {
        cb({ message: 'lock failed' })
      })
      await expect(client.lockSession()).rejects.toThrow('lock failed')
    })

    it('throws when not connected', async () => {
      client.disconnect()
      await expect(client.lockSession()).rejects.toThrow('Not connected')
    })
  })

  describe('unlockSession', () => {
    beforeEach(async () => {
      connectClient()
      await client.connect()
    })

    it('resolves with session on success', async () => {
      const mockProtoSession = {
        id: 'session-1',
        workspaces: [],
        createdAt: 100,
        lastActivity: 200,
        version: 1,
        lock: null
      }
      mockClientInstance.unlockSession.mockImplementation((_req: any, cb: (err: any, res: any) => void) => {
        cb(null, mockProtoSession)
      })
      const result = await client.unlockSession()
      expect(result.id).toBe('session-1')
      expect(result.lock).toBeNull()
    })

    it('rejects on error', async () => {
      mockClientInstance.unlockSession.mockImplementation((_req: any, cb: (err: any) => void) => {
        cb({ message: 'unlock failed' })
      })
      await expect(client.unlockSession()).rejects.toThrow('unlock failed')
    })
  })

  describe('forceUnlockSession', () => {
    beforeEach(async () => {
      connectClient()
      await client.connect()
    })

    it('resolves with session on success', async () => {
      const mockProtoSession = {
        id: 'session-1',
        workspaces: [],
        createdAt: 100,
        lastActivity: 200,
        version: 1,
        lock: null
      }
      mockClientInstance.forceUnlockSession.mockImplementation((_req: any, cb: (err: any, res: any) => void) => {
        cb(null, mockProtoSession)
      })
      const result = await client.forceUnlockSession()
      expect(result.id).toBe('session-1')
    })

    it('rejects on error', async () => {
      mockClientInstance.forceUnlockSession.mockImplementation((_req: any, cb: (err: any) => void) => {
        cb({ message: 'force unlock failed' })
      })
      await expect(client.forceUnlockSession()).rejects.toThrow('force unlock failed')
    })
  })

  describe('shutdownDaemon', () => {
    beforeEach(async () => {
      connectClient()
      await client.connect()
    })

    it('calls disconnect after shutdown', async () => {
      mockClientInstance.shutdown.mockImplementation((_req: any, cb: (err: any) => void) => {
        cb(null)
      })
      await client.shutdownDaemon()
      expect(client.isConnected()).toBe(false)
      expect(mockClientInstance.close).toHaveBeenCalled()
    })

    it('rejects on error', async () => {
      mockClientInstance.shutdown.mockImplementation((_req: any, cb: (err: any) => void) => {
        cb({ message: 'shutdown failed' })
      })
      await expect(client.shutdownDaemon()).rejects.toThrow('shutdown failed')
    })

    it('throws when not connected', async () => {
      client.disconnect()
      await expect(client.shutdownDaemon()).rejects.toThrow('Not connected')
    })
  })

  describe('readFile with image', () => {
    beforeEach(async () => {
      connectClient()
      await client.connect()
    })

    it('returns base64 content and language=image for image files', async () => {
      const imageData = Buffer.from([0x89, 0x50, 0x4e, 0x47])
      mockClientInstance.readFile.mockImplementation(() => {
        const handlers: Record<string, (...args: any[]) => void> = {}
        const stream = {
          on: (event: string, handler: (...args: any[]) => void) => { handlers[event] = handler; return stream },
        }
        setTimeout(() => {
          handlers['data']!({ header: { path: '/ws/image.png', size: 4, language: 'plaintext' } })
          handlers['data']!({ data: { data: imageData } })
          handlers['data']!({ end: { success: true } })
        }, 0)
        return stream
      })
      const result = await client.readFile('/ws', '/ws/image.png')
      expect(result.success).toBe(true)
      if (result.success) {
        expect(result.file.language).toBe('image')
        expect(result.file.content).toBe(imageData.toString('base64'))
      }
    })
  })

  describe('execStream', () => {
    beforeEach(async () => {
      connectClient()
      await client.connect()
    })

    it('returns exec stream from client', () => {
      const mockStream = { on: vi.fn<(...args: any[]) => any>(), write: vi.fn<(...args: any[]) => void>() }
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
