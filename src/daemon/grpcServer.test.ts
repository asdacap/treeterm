import { describe, it, expect, vi, beforeEach } from 'vitest'

// Capture service implementation from addService
let capturedServiceImpl: Record<string, Function> = {}

const { mockBindAsync, mockForceShutdown, mockAddService } = vi.hoisted(() => {
  return {
    mockBindAsync: vi.fn(),
    mockForceShutdown: vi.fn(),
    mockAddService: vi.fn()
  }
})

vi.mock('@grpc/grpc-js', () => ({
  Server: class {
    constructor() {
      Object.assign(this, {
        addService: mockAddService,
        bindAsync: mockBindAsync,
        forceShutdown: mockForceShutdown
      })
    }
  },
  ServerCredentials: {
    createInsecure: vi.fn().mockReturnValue('insecure-creds')
  },
  Metadata: class {
    get = vi.fn().mockReturnValue([])
    set = vi.fn()
  },
  status: {
    INVALID_ARGUMENT: 3,
    NOT_FOUND: 5,
    INTERNAL: 13
  }
}))

vi.mock('fs', () => ({
  existsSync: vi.fn().mockReturnValue(false),
  unlinkSync: vi.fn(),
  mkdirSync: vi.fn(),
  chmodSync: vi.fn()
}))

vi.mock('./logger', () => ({
  createModuleLogger: () => ({
    info: vi.fn(),
    warn: vi.fn(),
    error: vi.fn(),
    debug: vi.fn()
  })
}))

vi.mock('./filesystem', () => ({
  readDirectory: vi.fn(),
  readFile: vi.fn(),
  writeFile: vi.fn(),
  searchFiles: vi.fn()
}))

vi.mock('./execManager', () => ({
  execManager: {
    start: vi.fn(),
    writeStdin: vi.fn(),
    kill: vi.fn(),
    closeStdin: vi.fn(),
    shutdown: vi.fn()
  }
}))

// Patch Node.js Module._load to handle dynamic require('./sessionStore') in grpcServer constructor.
// vitest's vi.mock only intercepts ESM imports, not CJS require() calls.
import Module from 'module'
const origLoad = (Module as any)._load
;(Module as any)._load = function(request: string, parent: any, ...args: any[]) {
  if (request === './sessionStore' && parent?.filename?.includes('grpcServer')) {
    return {
      SessionStore: class {
        createSession() {}
        updateSession() {}
        getSession() {}
        deleteSession() {}
        listSessions() { return [] }
        getOrCreateDefaultSession() {}
        detachClient() {}
      }
    }
  }
  return origLoad.call(this, request, parent, ...args)
}

vi.mock('../generated/treeterm', () => ({
  TreeTermDaemonService: {}
}))

vi.mock('./socketPath', () => ({
  getDefaultSocketPath: vi.fn().mockReturnValue('/tmp/test.sock')
}))

import { GrpcServer } from './grpcServer'
import * as filesystem from './filesystem'
import { execManager } from './execManager'
import * as fs from 'fs'

// Helper to create mock call objects
function makeUnaryCall(request: any, metadata?: any): any {
  return {
    request,
    metadata: metadata || {
      get: vi.fn().mockReturnValue([])
    }
  }
}

function makeCallback(): any {
  return vi.fn()
}

// Helper to create a mock pty manager
function makeMockPtyManager(): any {
  return {
    create: vi.fn().mockReturnValue('pty-1'),
    attach: vi.fn().mockReturnValue({ scrollback: ['line1'], session: {}, exitCode: undefined }),
    write: vi.fn(),
    resize: vi.fn(),
    kill: vi.fn(),
    listSessions: vi.fn().mockReturnValue([]),
    onData: vi.fn().mockReturnValue(() => {}),
    onExit: vi.fn().mockReturnValue(() => {}),
    onSessionData: vi.fn().mockReturnValue(() => {}),
    onSessionExit: vi.fn().mockReturnValue(() => {}),
    onSessionResize: vi.fn().mockReturnValue(() => {}),
    shutdown: vi.fn()
  }
}

describe('GrpcServer', () => {
  let server: GrpcServer
  let mockPtyManager: ReturnType<typeof makeMockPtyManager>
  let mockSessionStore: any

  beforeEach(() => {
    vi.clearAllMocks()
    capturedServiceImpl = {}
    mockPtyManager = makeMockPtyManager()

    // Capture the service impl when addService is called
    mockAddService.mockImplementation((_service: any, impl: any) => {
      capturedServiceImpl = impl
    })

    // Create the mock session store before constructing the server
    mockSessionStore = {
      createSession: vi.fn(),
      updateSession: vi.fn(),
      getSession: vi.fn(),
      deleteSession: vi.fn(),
      listSessions: vi.fn().mockReturnValue([]),
      getOrCreateDefaultSession: vi.fn(),
      detachClient: vi.fn()
    }

    // Pass sessionStore explicitly to avoid the dynamic require
    server = new GrpcServer('/tmp/test.sock', mockPtyManager, mockSessionStore)
  })

  describe('lifecycle', () => {
    it('start binds to unix socket', async () => {
      mockBindAsync.mockImplementation((_uri: string, _creds: any, cb: Function) => {
        cb(null, 0)
      })
      await server.start()
      expect(mockBindAsync).toHaveBeenCalledWith(
        'unix:///tmp/test.sock',
        expect.anything(),
        expect.any(Function)
      )
    })

    it('start removes stale socket if exists', async () => {
      vi.mocked(fs.existsSync).mockReturnValue(true)
      mockBindAsync.mockImplementation((_uri: string, _creds: any, cb: Function) => {
        cb(null, 0)
      })
      await server.start()
      expect(fs.unlinkSync).toHaveBeenCalledWith('/tmp/test.sock')
    })

    it('start sets socket permissions', async () => {
      mockBindAsync.mockImplementation((_uri: string, _creds: any, cb: Function) => {
        cb(null, 0)
      })
      await server.start()
      expect(fs.chmodSync).toHaveBeenCalledWith('/tmp/test.sock', 0o600)
    })

    it('start rejects on bind error', async () => {
      mockBindAsync.mockImplementation((_uri: string, _creds: any, cb: Function) => {
        cb(new Error('bind failed'))
      })
      await expect(server.start()).rejects.toThrow('bind failed')
    })

    it('stop shuts down server and removes socket', () => {
      vi.mocked(fs.existsSync).mockReturnValue(true)
      server.stop()
      expect(mockForceShutdown).toHaveBeenCalled()
      expect(fs.unlinkSync).toHaveBeenCalledWith('/tmp/test.sock')
    })

    it('stop calls execManager.shutdown', () => {
      server.stop()
      expect(execManager.shutdown).toHaveBeenCalled()
    })
  })

  describe('PTY handlers', () => {
    it('createPty succeeds and returns sessionId', () => {
      const call = makeUnaryCall({
        cwd: '/home',
        env: {},
        cols: 80,
        rows: 24,
        sandbox: undefined,
        startupCommand: undefined
      })
      const callback = makeCallback()

      capturedServiceImpl.createPty(call, callback)

      expect(mockPtyManager.create).toHaveBeenCalledWith({
        cwd: '/home',
        env: {},
        cols: 80,
        rows: 24,
        sandbox: undefined,
        startupCommand: undefined
      })
      expect(callback).toHaveBeenCalledWith(null, { sessionId: 'pty-1' })
    })

    it('createPty returns error on failure', () => {
      mockPtyManager.create.mockImplementation(() => { throw new Error('pty create failed') })
      const call = makeUnaryCall({ cwd: '/home', env: {} })
      const callback = makeCallback()

      capturedServiceImpl.createPty(call, callback)

      expect(callback).toHaveBeenCalledWith(
        expect.objectContaining({ code: 13, message: 'pty create failed' })
      )
    })

    it('killPty succeeds', () => {
      const call = makeUnaryCall({ sessionId: 'pty-1' })
      const callback = makeCallback()

      capturedServiceImpl.killPty(call, callback)

      expect(mockPtyManager.kill).toHaveBeenCalledWith('pty-1')
      expect(callback).toHaveBeenCalledWith(null, {})
    })

    it('listPtySessions returns session list', () => {
      mockPtyManager.listSessions.mockReturnValue([{
        id: 'pty-1',
        cwd: '/home',
        cols: 80,
        rows: 24,
        createdAt: 1000,
        lastActivity: 2000
      }])
      const call = makeUnaryCall({})
      const callback = makeCallback()

      capturedServiceImpl.listPtySessions(call, callback)

      expect(callback).toHaveBeenCalledWith(null, {
        sessions: [{
          id: 'pty-1',
          cwd: '/home',
          cols: 80,
          rows: 24,
          createdAt: 1000,
          lastActivity: 2000
        }]
      })
    })

  })

  describe('session handlers', () => {
    const mockSession = {
      id: 'session-1',
      workspaces: [{
        id: 'ws-1',
        path: '/test',
        name: 'test',
        parentId: null,
        children: [],
        status: 'active' as const,
        isGitRepo: false,
        gitBranch: null,
        gitRootPath: null,
        isWorktree: false,
        isDetached: false,
        appStates: {
          'tab-1': {
            applicationId: 'terminal',
            title: 'Terminal',
            state: { ptyId: 'pty-1' }
          }
        },
        activeTabId: 'tab-1',
        metadata: {},
        createdAt: 1000,
        lastActivity: 2000
      }],
      createdAt: 1000,
      lastActivity: 2000
    }

    it('createSession creates and returns proto session', () => {
      mockSessionStore.createSession.mockReturnValue(mockSession)
      const call = makeUnaryCall(
        {
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
                state: Buffer.from('{"ptyId":"pty-1"}')
              }
            },
            activeTabId: 'tab-1'
          }]
        },
        { get: vi.fn().mockReturnValue(['client-1']) }
      )
      const callback = makeCallback()

      capturedServiceImpl.createSession(call, callback)

      expect(mockSessionStore.createSession).toHaveBeenCalled()
      expect(callback).toHaveBeenCalledWith(null, expect.objectContaining({ id: 'session-1' }))
    })

    it('createSession returns error on failure', () => {
      mockSessionStore.createSession.mockImplementation(() => { throw new Error('fail') })
      const call = makeUnaryCall(
        { workspaces: [] },
        { get: vi.fn().mockReturnValue([]) }
      )
      const callback = makeCallback()

      capturedServiceImpl.createSession(call, callback)

      expect(callback).toHaveBeenCalledWith(
        expect.objectContaining({ code: 13, message: 'fail' })
      )
    })

    it('updateSession updates and returns session', () => {
      mockSessionStore.updateSession.mockReturnValue(mockSession)
      const call = makeUnaryCall(
        { sessionId: 'session-1', workspaces: [], senderId: 'sender-1' },
        { get: vi.fn().mockReturnValue(['client-1']) }
      )
      const callback = makeCallback()

      capturedServiceImpl.updateSession(call, callback)

      expect(callback).toHaveBeenCalledWith(null, expect.objectContaining({ id: 'session-1' }))
    })

    it('updateSession returns NOT_FOUND when session does not exist', () => {
      mockSessionStore.updateSession.mockReturnValue(null)
      const call = makeUnaryCall(
        { sessionId: 'nonexistent', workspaces: [], senderId: 'sender-1' },
        { get: vi.fn().mockReturnValue([]) }
      )
      const callback = makeCallback()

      capturedServiceImpl.updateSession(call, callback)

      expect(callback).toHaveBeenCalledWith(
        expect.objectContaining({ code: 5 })
      )
    })

    it('updateSession rejects request without senderId', () => {
      const call = makeUnaryCall(
        { sessionId: 'session-1', workspaces: [], senderId: undefined },
        { get: vi.fn().mockReturnValue(['client-1']) }
      )
      const callback = makeCallback()

      capturedServiceImpl.updateSession(call, callback)

      expect(callback).toHaveBeenCalledWith(
        expect.objectContaining({
          code: 3,
          message: 'senderId is required for session updates'
        })
      )
    })

    it('deleteSession deletes and returns empty', () => {
      const call = makeUnaryCall({ sessionId: 'session-1' })
      const callback = makeCallback()

      capturedServiceImpl.deleteSession(call, callback)

      expect(mockSessionStore.deleteSession).toHaveBeenCalledWith('session-1')
      expect(callback).toHaveBeenCalledWith(null, {})
    })

    it('listSessions returns all sessions', () => {
      mockSessionStore.listSessions.mockReturnValue([mockSession])
      const call = makeUnaryCall({})
      const callback = makeCallback()

      capturedServiceImpl.listSessions(call, callback)

      expect(callback).toHaveBeenCalledWith(null, {
        sessions: [expect.objectContaining({ id: 'session-1' })]
      })
    })

    it('getDefaultSessionId returns session id', () => {
      mockSessionStore.getOrCreateDefaultSession.mockReturnValue(mockSession)
      const call = makeUnaryCall({})
      call.metadata = { get: vi.fn().mockReturnValue(['client-1']) }
      const callback = makeCallback()

      capturedServiceImpl.getDefaultSessionId(call, callback)

      expect(mockSessionStore.getOrCreateDefaultSession).toHaveBeenCalledWith('client-1')
      expect(callback).toHaveBeenCalledWith(null, { sessionId: 'session-1' })
    })
  })

  describe('session watch', () => {
    const mockSession = {
      id: 'session-1',
      workspaces: [],
      createdAt: 1000,
      lastActivity: 2000,
      attachedClients: 1
    }

    it('registers watcher on sessionWatch and emits initial state', () => {
      mockSessionStore.getSession.mockReturnValue(mockSession)
      const mockStream = {
        request: { sessionId: 'session-1', listenerId: 'listener-1' },
        on: vi.fn(),
        write: vi.fn(),
        destroy: vi.fn()
      }

      capturedServiceImpl.sessionWatch(mockStream)

      // Should have emitted initial session state
      expect(mockStream.write).toHaveBeenCalledWith(
        expect.objectContaining({
          sessionId: 'session-1',
          senderId: ''
        })
      )
      // Should have registered on 'cancelled' and 'error' events
      expect(mockStream.on).toHaveBeenCalledWith('cancelled', expect.any(Function))
      expect(mockStream.on).toHaveBeenCalledWith('error', expect.any(Function))
    })

    it('sessionWatch destroys stream when session not found', () => {
      mockSessionStore.getSession.mockReturnValue(null)
      const mockStream = {
        request: { sessionId: 'nonexistent', listenerId: 'listener-1' },
        on: vi.fn(),
        write: vi.fn(),
        destroy: vi.fn()
      }

      capturedServiceImpl.sessionWatch(mockStream)

      expect(mockStream.destroy).toHaveBeenCalled()
    })

    it('updateSession with senderId broadcasts to watchers', () => {
      // First register a watcher
      mockSessionStore.getSession.mockReturnValue(mockSession)
      const mockWatchStream = {
        request: { sessionId: 'session-1', listenerId: 'listener-A' },
        on: vi.fn(),
        write: vi.fn(),
        destroy: vi.fn()
      }
      capturedServiceImpl.sessionWatch(mockWatchStream)
      mockWatchStream.write.mockClear() // Clear the initial write

      // Now update session with a senderId
      const mockSessionResult = {
        id: 'session-1',
        workspaces: [],
        createdAt: 1000,
        lastActivity: 2000
      }
      mockSessionStore.updateSession.mockReturnValue(mockSessionResult)

      const call = makeUnaryCall(
        { sessionId: 'session-1', workspaces: [], senderId: 'sender-B' },
        { get: vi.fn().mockReturnValue(['client-1']) }
      )
      const callback = makeCallback()

      capturedServiceImpl.updateSession(call, callback)

      // Watcher should receive the broadcast (different listenerId from senderId)
      expect(mockWatchStream.write).toHaveBeenCalledWith(
        expect.objectContaining({
          sessionId: 'session-1',
          senderId: 'sender-B'
        })
      )
    })

    it('updateSession does not broadcast to sender', () => {
      // Register a watcher with same listenerId as senderId
      mockSessionStore.getSession.mockReturnValue(mockSession)
      const mockWatchStream = {
        request: { sessionId: 'session-1', listenerId: 'same-id' },
        on: vi.fn(),
        write: vi.fn(),
        destroy: vi.fn()
      }
      capturedServiceImpl.sessionWatch(mockWatchStream)
      mockWatchStream.write.mockClear() // Clear the initial write

      const mockSessionResult = {
        id: 'session-1',
        workspaces: [],
        createdAt: 1000,
        lastActivity: 2000
      }
      mockSessionStore.updateSession.mockReturnValue(mockSessionResult)

      const call = makeUnaryCall(
        { sessionId: 'session-1', workspaces: [], senderId: 'same-id' },
        { get: vi.fn().mockReturnValue([]) }
      )
      const callback = makeCallback()

      capturedServiceImpl.updateSession(call, callback)

      // Should NOT broadcast to watcher with same listenerId
      expect(mockWatchStream.write).not.toHaveBeenCalled()
    })

    it('cancelled event cleans up watcher', () => {
      mockSessionStore.getSession.mockReturnValue(mockSession)
      const mockWatchStream = {
        request: { sessionId: 'session-1', listenerId: 'listener-1' },
        on: vi.fn(),
        write: vi.fn(),
        destroy: vi.fn()
      }
      capturedServiceImpl.sessionWatch(mockWatchStream)
      mockWatchStream.write.mockClear() // Clear the initial write

      // Trigger cancelled
      const cancelledHandler = mockWatchStream.on.mock.calls.find((c: any[]) => c[0] === 'cancelled')?.[1]
      cancelledHandler?.()

      // Now an update should not broadcast to this watcher
      const mockSessionResult = {
        id: 'session-1',
        workspaces: [],
        createdAt: 1000,
        lastActivity: 2000
      }
      mockSessionStore.updateSession.mockReturnValue(mockSessionResult)
      const call = makeUnaryCall(
        { sessionId: 'session-1', workspaces: [], senderId: 'other' },
        { get: vi.fn().mockReturnValue([]) }
      )
      capturedServiceImpl.updateSession(call, makeCallback())
      expect(mockWatchStream.write).not.toHaveBeenCalled()
    })
  })

  describe('ptyStream', () => {
    it('sends scrollback on start message', () => {
      const handlers: Record<string, Function> = {}
      const mockStream = {
        metadata: { get: vi.fn().mockReturnValue(['client-1']) },
        on: vi.fn().mockImplementation((event: string, handler: Function) => {
          handlers[event] = handler
        }),
        write: vi.fn(),
        end: vi.fn()
      }

      capturedServiceImpl.ptyStream(mockStream)

      // Send start message
      handlers.data({ start: { sessionId: 'pty-1' } })

      expect(mockPtyManager.attach).toHaveBeenCalledWith('pty-1')
      // Should have written scrollback data
      expect(mockStream.write).toHaveBeenCalledWith({
        data: { data: Buffer.from('line1', 'utf-8') }
      })
      // Should have subscribed to live data/exit
      expect(mockPtyManager.onSessionData).toHaveBeenCalledWith('pty-1', expect.any(Function))
      expect(mockPtyManager.onSessionExit).toHaveBeenCalledWith('pty-1', expect.any(Function))
    })

    it('sends exit and ends stream when session already exited', () => {
      mockPtyManager.attach.mockReturnValue({ scrollback: [], session: {}, exitCode: 0 })
      const handlers: Record<string, Function> = {}
      const mockStream = {
        metadata: { get: vi.fn().mockReturnValue(['client-1']) },
        on: vi.fn().mockImplementation((event: string, handler: Function) => {
          handlers[event] = handler
        }),
        write: vi.fn(),
        end: vi.fn()
      }

      capturedServiceImpl.ptyStream(mockStream)
      handlers.data({ start: { sessionId: 'pty-1' } })

      expect(mockStream.write).toHaveBeenCalledWith({
        exit: { exitCode: 0 }
      })
      expect(mockStream.end).toHaveBeenCalled()
    })

    it('handles write input after start', () => {
      const handlers: Record<string, Function> = {}
      const mockStream = {
        metadata: { get: vi.fn().mockReturnValue(['client-1']) },
        on: vi.fn().mockImplementation((event: string, handler: Function) => {
          handlers[event] = handler
        }),
        write: vi.fn(),
        end: vi.fn()
      }

      capturedServiceImpl.ptyStream(mockStream)
      handlers.data({ start: { sessionId: 'pty-1' } })

      handlers.data({ write: { data: Buffer.from('hello') } })
      expect(mockPtyManager.write).toHaveBeenCalledWith('pty-1', 'hello')
    })

    it('handles resize input after start', () => {
      const handlers: Record<string, Function> = {}
      const mockStream = {
        metadata: { get: vi.fn().mockReturnValue(['client-1']) },
        on: vi.fn().mockImplementation((event: string, handler: Function) => {
          handlers[event] = handler
        }),
        write: vi.fn(),
        end: vi.fn()
      }

      capturedServiceImpl.ptyStream(mockStream)
      handlers.data({ start: { sessionId: 'pty-1' } })

      handlers.data({ resize: { cols: 120, rows: 40 } })
      expect(mockPtyManager.resize).toHaveBeenCalledWith('pty-1', 120, 40)
    })

    it('handles end event', () => {
      const handlers: Record<string, Function> = {}
      const mockStream = {
        metadata: { get: vi.fn().mockReturnValue(['client-1']) },
        on: vi.fn().mockImplementation((event: string, handler: Function) => {
          handlers[event] = handler
        }),
        write: vi.fn(),
        end: vi.fn()
      }

      capturedServiceImpl.ptyStream(mockStream)

      expect(() => handlers.end()).not.toThrow()
    })
  })

  describe('execStream', () => {
    it('handles start input', () => {
      const handlers: Record<string, Function> = {}
      const mockStream = {
        metadata: { get: vi.fn().mockReturnValue(['client-1']) },
        on: vi.fn().mockImplementation((event: string, handler: Function) => {
          handlers[event] = handler
        }),
        write: vi.fn(),
        end: vi.fn()
      }

      capturedServiceImpl.execStream(mockStream)

      handlers.data({
        start: {
          cwd: '/home',
          command: 'ls',
          args: ['-la'],
          env: {},
          timeoutMs: 5000
        }
      })

      expect(execManager.start).toHaveBeenCalledWith(
        expect.any(String),
        expect.objectContaining({ command: 'ls', args: ['-la'] }),
        expect.objectContaining({
          onStdout: expect.any(Function),
          onStderr: expect.any(Function),
          onExit: expect.any(Function)
        })
      )
    })

    it('handles end event and closes stdin', () => {
      const handlers: Record<string, Function> = {}
      const mockStream = {
        metadata: { get: vi.fn().mockReturnValue(['client-1']) },
        on: vi.fn().mockImplementation((event: string, handler: Function) => {
          handlers[event] = handler
        }),
        write: vi.fn(),
        end: vi.fn()
      }

      capturedServiceImpl.execStream(mockStream)

      // Start a command first to get an execId
      handlers.data({
        start: { cwd: '/home', command: 'cat', args: [], env: {}, timeoutMs: 5000 }
      })

      handlers.end()
      expect(execManager.closeStdin).toHaveBeenCalled()
    })

    it('handles error event and kills process', () => {
      const handlers: Record<string, Function> = {}
      const mockStream = {
        metadata: { get: vi.fn().mockReturnValue(['client-1']) },
        on: vi.fn().mockImplementation((event: string, handler: Function) => {
          handlers[event] = handler
        }),
        write: vi.fn(),
        end: vi.fn()
      }

      capturedServiceImpl.execStream(mockStream)

      handlers.data({
        start: { cwd: '/home', command: 'cat', args: [], env: {}, timeoutMs: 5000 }
      })

      handlers.error(new Error('stream broken'))
      expect(execManager.kill).toHaveBeenCalled()
    })
  })

  describe('filesystem handlers', () => {
    it('readDirectory delegates to filesystem module', async () => {
      vi.mocked(filesystem.readDirectory).mockResolvedValue({
        success: true,
        contents: { directories: [], files: [] }
      } as any)
      const call = makeUnaryCall({ workspacePath: '/ws', dirPath: '.' })
      const callback = makeCallback()

      await capturedServiceImpl.readDirectory(call, callback)

      expect(filesystem.readDirectory).toHaveBeenCalledWith('/ws', '.')
      expect(callback).toHaveBeenCalledWith(null, expect.objectContaining({ success: true }))
    })

    it('readDirectory returns error on failure', async () => {
      vi.mocked(filesystem.readDirectory).mockRejectedValue(new Error('access denied'))
      const call = makeUnaryCall({ workspacePath: '/ws', dirPath: '.' })
      const callback = makeCallback()

      await capturedServiceImpl.readDirectory(call, callback)

      expect(callback).toHaveBeenCalledWith(
        expect.objectContaining({ code: 13, message: 'access denied' })
      )
    })

    it('readFile streams file in chunks', async () => {
      vi.mocked(filesystem.readFile).mockResolvedValue({
        success: true,
        file: { path: '/file.txt', content: 'hello world', size: 11, language: 'text' }
      } as any)

      const writes: any[] = []
      const mockStream = {
        request: { workspacePath: '/ws', filePath: '/file.txt' },
        write: vi.fn().mockImplementation((data: any) => writes.push(data)),
        end: vi.fn()
      }

      await capturedServiceImpl.readFile(mockStream)

      expect(writes.length).toBeGreaterThanOrEqual(3) // header, data, end
      expect(writes[0]).toHaveProperty('header')
      expect(writes[writes.length - 1]).toHaveProperty('end')
      expect(writes[writes.length - 1].end.success).toBe(true)
    })

    it('readFile handles error from filesystem', async () => {
      vi.mocked(filesystem.readFile).mockResolvedValue({
        success: false,
        error: 'file not found'
      } as any)

      const writes: any[] = []
      const mockStream = {
        request: { workspacePath: '/ws', filePath: '/missing.txt' },
        write: vi.fn().mockImplementation((data: any) => writes.push(data)),
        end: vi.fn()
      }

      await capturedServiceImpl.readFile(mockStream)

      expect(writes[0].end.success).toBe(false)
      expect(writes[0].end.error).toBe('file not found')
    })

    it('writeFile collects chunks and writes', async () => {
      vi.mocked(filesystem.writeFile).mockResolvedValue({ success: true } as any)

      const handlers: Record<string, Function> = {}
      const mockStream = {
        on: vi.fn().mockImplementation((event: string, handler: Function) => {
          handlers[event] = handler
        })
      }
      const callback = makeCallback()

      capturedServiceImpl.writeFile(mockStream, callback)

      // Simulate header + data + end
      handlers.data({ header: { workspacePath: '/ws', filePath: '/file.txt' } })
      handlers.data({ data: { data: Buffer.from('content') } })
      await handlers.end()

      expect(filesystem.writeFile).toHaveBeenCalledWith('/ws', '/file.txt', 'content')
      expect(callback).toHaveBeenCalledWith(null, { success: true })
    })

    it('searchFiles delegates to filesystem module', async () => {
      vi.mocked(filesystem.searchFiles).mockResolvedValue({
        success: true,
        entries: [{ name: 'file.txt', path: '/file.txt', isDirectory: false, size: 10 }]
      } as any)
      const call = makeUnaryCall({ workspacePath: '/ws', query: 'file' })
      const callback = makeCallback()

      capturedServiceImpl.searchFiles(call, callback)

      // Need to wait for promise resolution
      await vi.waitFor(() => {
        expect(callback).toHaveBeenCalledWith(null, expect.objectContaining({ success: true }))
      })
    })
  })

  describe('proto conversion', () => {
    it('convertWorkspaceInputs correctly parses app state from JSON buffer', () => {
      const input = [{
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
        activeTabId: 'tab-1'
      }]

      mockSessionStore.createSession.mockImplementation((clientId: string, workspaces: any[]) => {
        // Verify the conversion happened
        expect(workspaces[0].appStates['tab-1'].state).toEqual({ ptyId: 'pty-1' })
        expect(workspaces[0].parentId).toBeNull()
        return {
          id: 'session-1',
          workspaces: [{
            ...workspaces[0],
            createdAt: 1000,
            lastActivity: 2000
          }],
          createdAt: 1000,
          lastActivity: 2000
        }
      })

      const call = makeUnaryCall(
        { workspaces: input },
        { get: vi.fn().mockReturnValue(['client-1']) }
      )
      const callback = makeCallback()

      capturedServiceImpl.createSession(call, callback)

      expect(callback).toHaveBeenCalledWith(null, expect.anything())
    })

    it('convertToProtoSession serializes app state to JSON buffer', () => {
      const session = {
        id: 'session-1',
        workspaces: [{
          id: 'ws-1',
          path: '/test',
          name: 'test',
          parentId: null,
          children: [],
          status: 'active' as const,
          isGitRepo: false,
          gitBranch: null,
          gitRootPath: null,
          isWorktree: false,
          isDetached: false,
          appStates: {
            'tab-1': {
              applicationId: 'terminal',
              title: 'Terminal',
              state: { ptyId: 'pty-1' }
            }
          },
          activeTabId: 'tab-1',
          createdAt: 1000,
          lastActivity: 2000
        }],
        createdAt: 1000,
        lastActivity: 2000
      }

      // Use sessionWatch to test proto conversion (initial event emits converted session)
      mockSessionStore.getSession.mockReturnValue(session)
      const mockStream = {
        request: { sessionId: 'session-1', listenerId: 'listener-1' },
        on: vi.fn(),
        write: vi.fn(),
        destroy: vi.fn()
      }

      capturedServiceImpl.sessionWatch(mockStream)

      const result = mockStream.write.mock.calls[0][0].session
      expect(result.workspaces[0].appStates['tab-1'].state).toBeInstanceOf(Buffer)
      const parsed = JSON.parse(result.workspaces[0].appStates['tab-1'].state.toString('utf-8'))
      expect(parsed).toEqual({ ptyId: 'pty-1' })
    })
  })

  describe('error paths', () => {
    it('readFile catches thrown errors from filesystem', async () => {
      vi.mocked(filesystem.readFile).mockRejectedValue(new Error('disk failure'))

      const writes: any[] = []
      const mockStream = {
        request: { workspacePath: '/ws', filePath: '/file.txt' },
        write: vi.fn().mockImplementation((data: any) => writes.push(data)),
        end: vi.fn()
      }

      await capturedServiceImpl.readFile(mockStream)

      expect(writes[0].end.success).toBe(false)
      expect(writes[0].end.error).toBe('disk failure')
    })

    it('writeFile catches thrown errors from filesystem', async () => {
      vi.mocked(filesystem.writeFile).mockRejectedValue(new Error('permission denied'))

      const handlers: Record<string, Function> = {}
      const mockStream = {
        on: vi.fn().mockImplementation((event: string, handler: Function) => {
          handlers[event] = handler
        })
      }
      const callback = makeCallback()

      capturedServiceImpl.writeFile(mockStream, callback)

      handlers.data({ header: { workspacePath: '/ws', filePath: '/file.txt' } })
      handlers.data({ data: { data: Buffer.from('content') } })
      await handlers.end()

      expect(callback).toHaveBeenCalledWith(
        expect.objectContaining({ message: 'permission denied' })
      )
    })

    it('writeFile handles stream error event', () => {
      const handlers: Record<string, Function> = {}
      const mockStream = {
        on: vi.fn().mockImplementation((event: string, handler: Function) => {
          handlers[event] = handler
        })
      }
      const callback = makeCallback()

      capturedServiceImpl.writeFile(mockStream, callback)
      handlers.error(new Error('stream reset'))

      expect(callback).toHaveBeenCalledWith(
        expect.objectContaining({ message: 'stream reset' })
      )
    })
  })

})
