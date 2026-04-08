import { describe, it, expect, vi, beforeEach } from 'vitest'

// Mock application renderers that depend on browser APIs (xterm)
vi.mock('../../applications/terminal/renderer', () => ({
  createTerminalApplication: vi.fn<(...args: any[]) => any>().mockReturnValue({
    id: 'terminal', name: 'Terminal', icon: '>', createInitialState: () => ({}),
    render: () => null, onWorkspaceLoad: () => ({ dispose: () => {} }), canClose: true, showInNewTabMenu: true,
    displayStyle: 'flex', isDefault: true
  }),
  createTerminalVariant: vi.fn<(...args: any[]) => any>().mockReturnValue({
    id: 'terminal-custom', name: 'Custom', icon: '>', createInitialState: () => ({}),
    render: () => null, onWorkspaceLoad: () => ({ dispose: () => {} }), canClose: true, showInNewTabMenu: true,
    displayStyle: 'flex', isDefault: false
  })
}))

vi.mock('../../applications/filesystem/renderer', () => ({
  filesystemApplication: {
    id: 'filesystem', name: 'Files', icon: 'F', createInitialState: () => ({}),
    render: () => null, onWorkspaceLoad: () => ({ dispose: () => {} }), canClose: true, showInNewTabMenu: true,
    displayStyle: 'flex', isDefault: false
  }
}))

vi.mock('../../applications/aiHarness/renderer', () => ({
  createAiHarnessVariant: vi.fn<(...args: any[]) => any>().mockReturnValue({
    id: 'aiharness-test', name: 'AI', icon: 'A', createInitialState: () => ({}),
    render: () => null, onWorkspaceLoad: () => ({ dispose: () => {} }), canClose: true, showInNewTabMenu: true,
    displayStyle: 'flex', isDefault: false
  })
}))

vi.mock('../../applications/customRunner/renderer', () => ({
  createCustomRunnerVariant: vi.fn<(...args: any[]) => any>().mockReturnValue({
    id: 'customrunner-test', name: 'Runner', icon: '▶', createInitialState: () => ({}),
    render: () => null, onWorkspaceLoad: () => ({ dispose: () => {} }), canClose: true, showInNewTabMenu: true,
    displayStyle: 'flex', isDefault: false
  })
}))

vi.mock('../../applications/review/renderer', () => ({
  reviewApplication: {
    id: 'review', name: 'Review', icon: 'R', createInitialState: () => ({}),
    render: () => null, onWorkspaceLoad: () => ({ dispose: () => {} }), canClose: true, showInNewTabMenu: false,
    displayStyle: 'flex', isDefault: false
  }
}))

vi.mock('../../applications/editor/renderer', () => ({
  editorApplication: {
    id: 'editor', name: 'Editor', icon: 'E', createInitialState: () => ({}),
    render: () => null, onWorkspaceLoad: () => ({ dispose: () => {} }), canClose: true, showInNewTabMenu: false,
    displayStyle: 'flex', isDefault: false
  }
}))

vi.mock('../../applications/comments/renderer', () => ({
  commentsApplication: {
    id: 'comments', name: 'Comments', icon: 'C', createInitialState: () => ({}),
    render: () => null, onWorkspaceLoad: () => ({ dispose: () => {} }), canClose: true, showInNewTabMenu: false,
    displayStyle: 'flex', isDefault: false
  }
}))


import type { StoreApi } from 'zustand'
import { useAppStore, type AppDeps } from './app'
import { useNavigationStore } from './navigation'
import { useSessionNamesStore } from './sessionNames'
import type { SessionState, SessionDeps } from './createSessionStore'
import type { Workspace } from '../types'
import { ConnectionStatus } from '../../shared/types'

// Mock createSessionStore and its utilities
vi.mock('./createSessionStore', () => ({
  createSessionStore: vi.fn<(...args: any[]) => any>().mockImplementation(() => ({
    getState: vi.fn<() => any>().mockReturnValue({
      workspaces: {},
      workspaceStores: {},
      activeWorkspaceId: null,
      isRestoring: false,
      addWorkspace: vi.fn<(...args: any[]) => any>(),
      setActiveWorkspace: vi.fn<(...args: any[]) => void>(),
      getWorkspace: vi.fn<(...args: any[]) => any>().mockReturnValue(null),
      syncToDaemon: vi.fn<() => Promise<void>>().mockResolvedValue(undefined),
      handleRestore: vi.fn<(...args: any[]) => Promise<void>>().mockResolvedValue(undefined),
      handleExternalUpdate: vi.fn<(...args: any[]) => Promise<void>>().mockResolvedValue(undefined),
      removeOrphanWorkspace: vi.fn<(...args: any[]) => void>(),
    }),
    setState: vi.fn<(...args: any[]) => void>(),
    subscribe: vi.fn<(...args: any[]) => any>()
  })),
  getUnmergedSubWorkspaces: vi.fn<(...args: any[]) => any[]>().mockReturnValue([])
}))

vi.mock('./settings', () => ({
  useSettingsStore: {
    getState: vi.fn<() => any>().mockReturnValue({
      init: vi.fn<(...args: any[]) => void>()
    })
  }
}))

// Mock deps for initialize
const mockDeps = {
  platform: 'darwin' as const,
  getWindowUuid: vi.fn<() => Promise<string>>().mockResolvedValue('test-uuid'),
  getInitialWorkspace: vi.fn<() => Promise<string | null>>().mockResolvedValue(null),
  settingsApi: { onOpen: vi.fn<(...args: any[]) => () => void>().mockReturnValue(() => {}) },
  appApi: {
    onCloseConfirm: vi.fn<(...args: any[]) => () => void>().mockReturnValue(() => {}),
    onReady: vi.fn<(...args: any[]) => () => void>().mockReturnValue(() => {}),
    onSshAutoConnected: vi.fn<(...args: any[]) => () => void>().mockReturnValue(() => {}),
    onConnectionReconnected: vi.fn<(...args: any[]) => () => void>().mockReturnValue(() => {}),
    confirmClose: vi.fn<() => void>(),
    cancelClose: vi.fn<() => void>()
  },
  sessionApi: {
    onSync: vi.fn<(...args: any[]) => () => void>().mockReturnValue(() => {}),
    update: vi.fn<(...args: any[]) => Promise<any>>().mockResolvedValue({ success: true, session: { id: 'test-session' } })
  },
  daemon: { onDisconnected: vi.fn<(...args: any[]) => () => void>().mockReturnValue(() => {}) },
  terminal: {
    onActiveProcessesOpen: vi.fn<(...args: any[]) => () => void>().mockReturnValue(() => {}),
    list: vi.fn<(...args: any[]) => Promise<any[]>>().mockResolvedValue([]),
    kill: vi.fn<(...args: any[]) => void>(),
    bind: vi.fn<(...args: any[]) => any>().mockReturnThis()
  },
  git: { onOutput: vi.fn<(...args: any[]) => () => void>().mockReturnValue(() => {}) },
  github: { getPrInfo: vi.fn<(...args: any[]) => any>() },
  filesystem: {},
  reviews: {},
  stt: {},
  runActions: { detect: vi.fn<(...args: any[]) => Promise<any[]>>().mockResolvedValue([]), run: vi.fn<(...args: any[]) => Promise<any>>().mockResolvedValue(null) },
  sandbox: {},
  ssh: {
    connect: vi.fn<(...args: any[]) => Promise<any>>().mockResolvedValue({ id: 'test', target: { type: 'remote' }, status: ConnectionStatus.Connected }),
    disconnect: vi.fn<(...args: any[]) => Promise<void>>().mockResolvedValue(undefined),
    listConnections: vi.fn<(...args: any[]) => Promise<any[]>>().mockResolvedValue([]),
    saveConnection: vi.fn<(...args: any[]) => Promise<void>>().mockResolvedValue(undefined),
    getSavedConnections: vi.fn<(...args: any[]) => Promise<any[]>>().mockResolvedValue([]),
    removeSavedConnection: vi.fn<(...args: any[]) => Promise<void>>().mockResolvedValue(undefined),
    getOutput: vi.fn<(...args: any[]) => Promise<any[]>>().mockResolvedValue([]),
    onConnectionStatus: vi.fn<(...args: any[]) => () => void>().mockReturnValue(() => {}),
    onOutput: vi.fn<(...args: any[]) => () => void>().mockReturnValue(() => {})
  },
  selectFolder: vi.fn<(...args: any[]) => any>(),
  llm: {
    analyzeTerminal: vi.fn<(...args: any[]) => any>(),
    generateTitle: vi.fn<(...args: any[]) => any>(),
  },
} as unknown as AppDeps

describe('useAppStore', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    useAppStore.setState({
      windowUuid: null,
      daemonDisconnected: false,
      isSettingsOpen: false,
      showCloseConfirm: false,
      unmergedWorkspaces: [],
      sessionStores: new Map()
    })
    useNavigationStore.setState({ activeView: null })
  })

  describe('initial state', () => {
    it('has null windowUuid by default', () => {
      expect(useAppStore.getState().windowUuid).toBeNull()
    })

    it('has daemonDisconnected false by default', () => {
      expect(useAppStore.getState().daemonDisconnected).toBe(false)
    })

    it('has empty sessionStores by default', () => {
      expect(useAppStore.getState().sessionStores).toEqual(new Map())
    })

    it('has isSettingsOpen false by default', () => {
      expect(useAppStore.getState().isSettingsOpen).toBe(false)
    })

  })

  describe('disconnectSession', () => {
    it('removes session from sessionStores', () => {
      const mockStore = {
        getState: vi.fn<() => any>().mockReturnValue({ workspaces: new Map() }),
        setState: vi.fn<(...args: any[]) => void>(),
        subscribe: vi.fn<(...args: any[]) => any>()
      } as never
      useAppStore.setState({ sessionStores: new Map([
        ['s1', { store: mockStore }],
        ['s2', { store: mockStore }]
      ]) })
      useAppStore.getState().disconnectSession('s1')
      expect(useAppStore.getState().sessionStores.get('s1')).toBeUndefined()
      expect(useAppStore.getState().sessionStores.get('s2')).toBeDefined()
    })

    it('clears navigation when disconnecting viewed session', () => {
      const mockStore = {
        getState: vi.fn<() => any>().mockReturnValue({ workspaces: new Map([['ws-1', { id: 'ws-1' }]]) }),
        setState: vi.fn<(...args: any[]) => void>(),
        subscribe: vi.fn<(...args: any[]) => any>()
      } as never
      useAppStore.setState({ sessionStores: new Map([
        ['s1', { store: mockStore }],
        ['s2', { store: mockStore }]
      ]) })
      useNavigationStore.setState({ activeView: { type: 'workspace', workspaceId: 'ws-x', sessionId: 's1' } })
      useAppStore.getState().disconnectSession('s1')
      // Should switch to remaining session's first workspace
      const nav = useNavigationStore.getState().activeView
      expect(nav?.type === 'workspace' && nav.sessionId).toBe('s2')
    })
  })

  describe('session restore via onReady', () => {
    let mockAddWorkspace: ReturnType<typeof vi.fn>
    let mockSyncToDaemon: ReturnType<typeof vi.fn>
    let mockHandleRestore: ReturnType<typeof vi.fn>

    beforeEach(async () => {
      mockAddWorkspace = vi.fn<(...args: any[]) => Promise<string>>().mockResolvedValue('ws-new-id')
      mockSyncToDaemon = vi.fn<() => Promise<void>>().mockResolvedValue(undefined)
      mockHandleRestore = vi.fn<(...args: any[]) => Promise<void>>().mockResolvedValue(undefined)

      // Import and configure the mock
      const { createSessionStore } = await import('./createSessionStore')
      const mockedCreate = vi.mocked(createSessionStore)
      mockedCreate.mockImplementation(() => ({
        getState: vi.fn<() => any>().mockReturnValue({
          workspaces: {},
          workspaceStores: {},
          activeWorkspaceId: null,
          isRestoring: false,
          addWorkspace: mockAddWorkspace,
          setActiveWorkspace: vi.fn<(...args: any[]) => void>(),
          getWorkspace: vi.fn<(...args: any[]) => any>().mockReturnValue(null),
          syncToDaemon: mockSyncToDaemon,
          handleRestore: mockHandleRestore,
          handleExternalUpdate: vi.fn<(...args: any[]) => Promise<void>>().mockResolvedValue(undefined),
        }),
        setState: vi.fn<(...args: any[]) => void>(),
        subscribe: vi.fn<(...args: any[]) => any>()
      }) as unknown as StoreApi<SessionState>)

      // Initialize the store with deps so terminal/git/etc are available
      const cleanup = await useAppStore.getState().initialize(mockDeps)
      cleanup()
    })

    it('restores root + child workspaces with parent-child links', () => {
      const session: any = {
        id: 'session-1',
        workspaces: [
          {
            id: 'root-ws',
            path: '/projects/root',
            name: 'root',
            parentId: null,
            appStates: {},
            activeTabId: null
          },
          {
            id: 'child-ws',
            path: '/projects/root/child',
            name: 'child',
            parentId: 'root-ws',
            appStates: {},
            activeTabId: null
          }
        ]
      }

      // Trigger onReady with the session
      const readyCallback = vi.mocked(mockDeps.appApi.onReady).mock.calls[0]![0] as (session: any) => void
      readyCallback(session)

      // Workspaces restored via handleRestore (not addWorkspace)
      expect(mockAddWorkspace).not.toHaveBeenCalled()
      expect(mockHandleRestore).toHaveBeenCalledWith(session)
    })

    it('child workspace restored even when parent not in state', () => {
      const session: any = {
        id: 'session-2',
        workspaces: [
          {
            id: 'orphan-child',
            path: '/projects/orphan',
            name: 'orphan',
            parentId: 'nonexistent-parent',
            appStates: { 'tab-1': { applicationId: 'terminal', title: 'Term', state: { ptyId: 'pty-1' } } },
            activeTabId: 'tab-1'
          }
        ]
      }

      const readyCallback = vi.mocked(mockDeps.appApi.onReady).mock.calls[0]![0] as (session: any) => void
      readyCallback(session)

      // Child should still be restored via handleRestore
      expect(mockHandleRestore).toHaveBeenCalledWith(session)
    })

    it('PTY IDs preserved as-is without sessionMap validation', () => {
      const session: any = {
        id: 'session-3',
        workspaces: [
          {
            id: 'ws-1',
            path: '/projects/test',
            name: 'test',
            parentId: null,
            appStates: {
              'tab-1': { applicationId: 'terminal', title: 'Term', state: { ptyId: 'pty-maybe-dead' } },
              'tab-2': { applicationId: 'ai-harness', title: 'AI', state: { ptyId: 'pty-unknown' } }
            },
            activeTabId: 'tab-1'
          }
        ]
      }

      const readyCallback = vi.mocked(mockDeps.appApi.onReady).mock.calls[0]![0] as (session: any) => void
      readyCallback(session)

      // terminal.list() should NOT have been called
      expect(mockDeps.terminal.list).not.toHaveBeenCalled()
      expect(mockHandleRestore).toHaveBeenCalledWith(session)
    })

    it('child without parentId still restored', () => {
      const session: any = {
        id: 'session-4',
        workspaces: [
          {
            id: 'child-no-parent',
            path: '/projects/no-parent',
            name: 'no-parent',
            parentId: null,
            appStates: { 'tab-1': { applicationId: 'terminal', title: 'Term', state: { ptyId: 'pty-1' } } },
            activeTabId: 'tab-1'
          }
        ]
      }

      const readyCallback = vi.mocked(mockDeps.appApi.onReady).mock.calls[0]![0] as (session: any) => void
      readyCallback(session)

      expect(mockAddWorkspace).not.toHaveBeenCalled()
      expect(mockHandleRestore).toHaveBeenCalledWith(session)
    })

    it('syncToDaemon is not called after restore', () => {
      const session: any = {
        id: 'session-5',
        workspaces: [
          {
            id: 'ws-1',
            path: '/projects/test',
            name: 'test',
            parentId: null,
            appStates: {},
            activeTabId: null
          }
        ]
      }

      const readyCallback = vi.mocked(mockDeps.appApi.onReady).mock.calls[0]![0] as (session: any) => void
      readyCallback(session)

      expect(mockSyncToDaemon).not.toHaveBeenCalled()
    })
  })

  describe('initialize', () => {
    it('fetches window UUID on initialization', async () => {
      const cleanup = await useAppStore.getState().initialize(mockDeps)
      expect(mockDeps.getWindowUuid).toHaveBeenCalled()
      expect(useAppStore.getState().windowUuid).toBe('test-uuid')
      cleanup()
    })

    it('returns a cleanup function', async () => {
      const cleanup = await useAppStore.getState().initialize(mockDeps)
      expect(typeof cleanup).toBe('function')
      // Should not throw
      cleanup()
    })

    it('wires IPC event listeners', async () => {
      const cleanup = await useAppStore.getState().initialize(mockDeps)
      expect(mockDeps.settingsApi.onOpen).toHaveBeenCalled()
      expect(mockDeps.appApi.onCloseConfirm).toHaveBeenCalled()
      expect(mockDeps.appApi.onReady).toHaveBeenCalled()
      expect(mockDeps.sessionApi.onSync).toHaveBeenCalled()
      expect(mockDeps.daemon.onDisconnected).toHaveBeenCalled()
      cleanup()
    })

    it('onCloseConfirm confirms close when no active store', async () => {
      const cleanup = await useAppStore.getState().initialize(mockDeps)
      const closeCallback = vi.mocked(mockDeps.appApi.onCloseConfirm).mock.calls[0]![0] as () => void
      closeCallback()
      expect(mockDeps.appApi.confirmClose).toHaveBeenCalled()
      cleanup()
    })

    it('onCloseConfirm confirms close when no unmerged workspaces', async () => {
      const { getUnmergedSubWorkspaces } = await import('./createSessionStore')
      vi.mocked(getUnmergedSubWorkspaces).mockReturnValue([])

      const cleanup = await useAppStore.getState().initialize(mockDeps)
      const closeCallback = vi.mocked(mockDeps.appApi.onCloseConfirm).mock.calls[0]![0] as () => void
      closeCallback()
      expect(mockDeps.appApi.confirmClose).toHaveBeenCalled()
      cleanup()
    })

    it('onCloseConfirm shows confirm dialog when unmerged workspaces exist', async () => {
      const { createSessionStore, getUnmergedSubWorkspaces } = await import('./createSessionStore')
      const mockWs = { id: 'ws-1', name: 'unmerged', path: '/test', parentId: 'p', appStates: {}, activeTabId: null, metadata: {} }
      vi.mocked(getUnmergedSubWorkspaces).mockReturnValue([mockWs as unknown as Workspace])

      const mockStore = {
        getState: vi.fn<() => any>().mockReturnValue({
          workspaces: { 'ws-1': mockWs },
          workspaceStores: {},
          handleRestore: vi.fn<(...args: any[]) => Promise<void>>().mockResolvedValue(undefined),
          handleExternalUpdate: vi.fn<(...args: any[]) => Promise<void>>().mockResolvedValue(undefined),
        }),
        setState: vi.fn<(...args: any[]) => void>(),
        subscribe: vi.fn<(...args: any[]) => any>()
      }
      vi.mocked(createSessionStore).mockReturnValue(mockStore as unknown as StoreApi<SessionState>)

      const cleanup = await useAppStore.getState().initialize(mockDeps)

      // Trigger onReady to create a session store
      const readyCallback = vi.mocked(mockDeps.appApi.onReady).mock.calls[0]![0] as (session: any) => void
      readyCallback({ id: 'session-1', workspaces: [] })

      const closeCallback = vi.mocked(mockDeps.appApi.onCloseConfirm).mock.calls[0]![0] as () => void
      closeCallback()

      expect(useAppStore.getState().showCloseConfirm).toBe(true)
      expect(useAppStore.getState().unmergedWorkspaces).toEqual([mockWs])
      cleanup()
    })

    it('onReady restores session with workspaces', async () => {
      const { createSessionStore } = await import('./createSessionStore')
      const mockSetState = vi.fn<(...args: any[]) => void>()
      vi.mocked(createSessionStore).mockReturnValue({
        getState: vi.fn<() => any>().mockReturnValue({
          workspaces: {},
          workspaceStores: {},
          activeWorkspaceId: null,
          isRestoring: false,
          addWorkspace: vi.fn<(...args: any[]) => any>(),
          setActiveWorkspace: vi.fn<(...args: any[]) => void>(),
          getWorkspace: vi.fn<(...args: any[]) => any>().mockReturnValue(null),
          syncToDaemon: vi.fn<() => void>(),
          handleRestore: vi.fn<(...args: any[]) => Promise<void>>().mockResolvedValue(undefined),
          handleExternalUpdate: vi.fn<(...args: any[]) => Promise<void>>().mockResolvedValue(undefined),
        }),
        setState: mockSetState,
        subscribe: vi.fn<(...args: any[]) => any>()
      } as unknown as StoreApi<SessionState>)

      const cleanup = await useAppStore.getState().initialize(mockDeps)
      const readyCallback = vi.mocked(mockDeps.appApi.onReady).mock.calls[0]![0] as (session: any) => void

      const session = {
        id: 'session-ready',
        workspaces: [{
          id: 'ws-1', path: '/test', name: 'test',
          parentId: null, appStates: {}, activeTabId: null
        }]
      }
      readyCallback(session)

      expect(useAppStore.getState().sessionStores.get('session-ready')).toBeDefined()
      cleanup()
    })

    it('onReady creates store for session with no workspaces', async () => {
      const cleanup = await useAppStore.getState().initialize(mockDeps)
      const readyCallback = vi.mocked(mockDeps.appApi.onReady).mock.calls[0]![0] as (session: any) => void

      readyCallback({ id: 'empty-session', workspaces: [] })

      expect(useAppStore.getState().sessionStores.get('empty-session')).toBeDefined()
      cleanup()
    })

    it('onDisconnected sets daemonDisconnected', async () => {
      const cleanup = await useAppStore.getState().initialize(mockDeps)

      const disconnectCallback = vi.mocked(mockDeps.daemon.onDisconnected).mock.calls[0]![0] as () => void
      disconnectCallback()

      expect(useAppStore.getState().daemonDisconnected).toBe(true)
      cleanup()
    })

    it('initial workspace activates existing workspace', async () => {
      const mockSetActiveWorkspace = vi.fn<(...args: any[]) => void>()
      const { createSessionStore } = await import('./createSessionStore')
      vi.mocked(createSessionStore).mockReturnValue({
        getState: vi.fn<() => any>().mockReturnValue({
          workspaces: new Map([['ws-existing', { status: 'loaded', data: { id: 'ws-existing', path: '/projects/existing', name: 'existing' }, store: {} }]]),
          activeWorkspaceId: null,
          isRestoring: false,
          addWorkspace: vi.fn<(...args: any[]) => any>(),
          setActiveWorkspace: mockSetActiveWorkspace,
          syncToDaemon: vi.fn<() => void>(),
          handleRestore: vi.fn<(...args: any[]) => Promise<void>>().mockResolvedValue(undefined),
          handleExternalUpdate: vi.fn<(...args: any[]) => Promise<void>>().mockResolvedValue(undefined),
        }),
        setState: vi.fn<(...args: any[]) => void>(),
        subscribe: vi.fn<(...args: any[]) => any>()
      } as unknown as StoreApi<SessionState>)

      // Set up initial workspace to return a path
      const deps = {
        ...mockDeps,
        getInitialWorkspace: vi.fn<() => Promise<string>>().mockResolvedValue('/projects/existing'),
        appApi: {
          ...mockDeps.appApi,
          onReady: vi.fn<(...args: any[]) => () => void>().mockImplementation((cb: (session: any) => void) => {
            // Immediately fire onReady with a session so the store exists
            setTimeout(() => { cb({ id: 'session-init', workspaces: [] }); }, 0)
            return () => {}
          }),
          onCloseConfirm: vi.fn<(...args: any[]) => () => void>().mockReturnValue(() => {}),
        }
      }

      // We need the store to exist before getInitialWorkspace resolves
      // Create a mock session store directly
      const mockSessionStoreInstance = vi.mocked(createSessionStore)({ sessionId: 'pre-session', windowUuid: null }, {} as unknown as SessionDeps)
      useAppStore.setState({
        sessionStores: new Map([['pre-session', { store: mockSessionStoreInstance }]])
      })

      const cleanup = await useAppStore.getState().initialize(deps)
      expect(mockSetActiveWorkspace).toHaveBeenCalledWith('ws-existing')
      cleanup()
    })

    it('initial workspace adds new workspace when not existing', async () => {
      const mockAddWorkspace = vi.fn<(...args: any[]) => Promise<string>>().mockResolvedValue('ws-new')
      const { createSessionStore } = await import('./createSessionStore')
      vi.mocked(createSessionStore).mockReturnValue({
        getState: vi.fn<() => any>().mockReturnValue({
          workspaces: new Map(),
          activeWorkspaceId: null,
          isRestoring: false,
          addWorkspace: mockAddWorkspace,
          setActiveWorkspace: vi.fn<(...args: any[]) => void>(),
          syncToDaemon: vi.fn<() => void>(),
          handleRestore: vi.fn<(...args: any[]) => Promise<void>>().mockResolvedValue(undefined),
          handleExternalUpdate: vi.fn<(...args: any[]) => Promise<void>>().mockResolvedValue(undefined),
        }),
        setState: vi.fn<(...args: any[]) => void>(),
        subscribe: vi.fn<(...args: any[]) => any>()
      } as unknown as StoreApi<SessionState>)

      const mockSessionStoreInstance = vi.mocked(createSessionStore)({ sessionId: 'pre-session', windowUuid: null }, {} as unknown as SessionDeps)
      useAppStore.setState({
        sessionStores: new Map([['pre-session', { store: mockSessionStoreInstance }]])
      })

      const deps = { ...mockDeps, getInitialWorkspace: vi.fn<() => Promise<string>>().mockResolvedValue('/new/path') }
      const cleanup = await useAppStore.getState().initialize(deps)
      expect(mockAddWorkspace).toHaveBeenCalledWith('/new/path')
      cleanup()
    })
  })

  describe('handleExternalSessionUpdate via onSync', () => {
    let mockHandleExternalUpdate: ReturnType<typeof vi.fn>

    beforeEach(async () => {
      mockHandleExternalUpdate = vi.fn<(...args: any[]) => Promise<void>>().mockResolvedValue(undefined)
      const { createSessionStore } = await import('./createSessionStore')
      vi.mocked(createSessionStore).mockReturnValue({
        getState: vi.fn<() => any>().mockReturnValue({
          workspaces: {},
          workspaceStores: {},
          activeWorkspaceId: null,
          isRestoring: false,
          addWorkspace: vi.fn<(...args: any[]) => any>(),
          setActiveWorkspace: vi.fn<(...args: any[]) => void>(),
          getWorkspace: vi.fn<(...args: any[]) => any>().mockReturnValue(null),
          syncToDaemon: vi.fn<() => void>(),
          handleRestore: vi.fn<(...args: any[]) => Promise<void>>().mockResolvedValue(undefined),
          handleExternalUpdate: mockHandleExternalUpdate,
        }),
        setState: vi.fn<(...args: any[]) => void>(),
        subscribe: vi.fn<(...args: any[]) => any>()
      } as unknown as StoreApi<SessionState>)

      const cleanup = await useAppStore.getState().initialize(mockDeps)
      cleanup()
    })

    it('syncs new workspaces from daemon for known session', () => {
      const sessionId = 'sync-session'
      const session: any = {
        id: sessionId,
        workspaces: [{
          id: 'ws-new', path: '/new', name: 'new',
          parentId: null, appStates: {}, activeTabId: null
        }]
      }

      // Pre-populate session store so onSync finds it (local connection)
      const localConn = { id: 'local', target: { type: 'local' }, status: ConnectionStatus.Connected }
      const mockStore = {
        getState: vi.fn<() => any>().mockReturnValue({ connection: localConn, handleExternalUpdate: mockHandleExternalUpdate }),
        setState: vi.fn<(...args: any[]) => void>(),
        subscribe: vi.fn<(...args: any[]) => any>()
      }
      useAppStore.setState((state) => ({
        sessionStores: new Map(state.sessionStores).set(sessionId, { store: mockStore as unknown as StoreApi<SessionState> })
      }))

      // Trigger onSync with matching connectionId
      const syncCallback = vi.mocked(mockDeps.sessionApi.onSync).mock.calls[0]![0] as (connectionId: string, session: any) => void
      syncCallback('local', session)

      // handleExternalUpdate called to sync workspaces
      expect(mockHandleExternalUpdate).toHaveBeenCalledWith(session)
    })

    it('delegates external update handling to session store', () => {
      const sessionId = 'sync-session-2'
      const session: any = {
        id: sessionId,
        workspaces: [] // No workspaces — removal handled inside session store
      }

      // Pre-populate session store so onSync finds it (local connection)
      const localConn = { id: 'local', target: { type: 'local' }, status: ConnectionStatus.Connected }
      const mockStore = {
        getState: vi.fn<() => any>().mockReturnValue({ connection: localConn, handleExternalUpdate: mockHandleExternalUpdate }),
        setState: vi.fn<(...args: any[]) => void>(),
        subscribe: vi.fn<(...args: any[]) => any>()
      }
      useAppStore.setState((state) => ({
        sessionStores: new Map(state.sessionStores).set(sessionId, { store: mockStore as unknown as StoreApi<SessionState> })
      }))

      // Trigger onSync with matching connectionId
      const syncCallback = vi.mocked(mockDeps.sessionApi.onSync).mock.calls[0]![0] as (connectionId: string, session: any) => void
      syncCallback('local', session)

      // handleExternalUpdate is responsible for removing orphan workspaces internally
      expect(mockHandleExternalUpdate).toHaveBeenCalledWith(session)
    })

    it('ignores onSync for unknown session', () => {
      const session: any = {
        id: 'unknown-session',
        workspaces: [{ id: 'ws-1', path: '/test', name: 'test' }]
      }

      // Trigger onSync without pre-populating session store
      const syncCallback = vi.mocked(mockDeps.sessionApi.onSync).mock.calls[0]![0] as (connectionId: string, session: any) => void
      syncCallback('local', session)

      // handleExternalUpdate should NOT be called
      expect(mockHandleExternalUpdate).not.toHaveBeenCalled()
    })

    it('ignores onSync from wrong connection', () => {
      const sessionId = 'sync-session-3'
      const session: any = { id: sessionId, workspaces: [] }

      // Pre-populate session store with remote connection
      const mockStore = {
        getState: vi.fn<() => any>().mockReturnValue({ connection: { id: 'ssh-remote' }, handleExternalUpdate: mockHandleExternalUpdate }),
        setState: vi.fn<(...args: any[]) => void>(),
        subscribe: vi.fn<(...args: any[]) => any>()
      }
      useAppStore.setState((state) => ({
        sessionStores: new Map(state.sessionStores).set(sessionId, { store: mockStore as unknown as StoreApi<SessionState> })
      }))

      // Trigger onSync with wrong connectionId
      const syncCallback = vi.mocked(mockDeps.sessionApi.onSync).mock.calls[0]![0] as (connectionId: string, session: any) => void
      syncCallback('local', session)

      // handleExternalUpdate should NOT be called — wrong connection
      expect(mockHandleExternalUpdate).not.toHaveBeenCalled()
    })
  })

  describe('application registry', () => {
    const mockApp = {
      id: 'test-app',
      name: 'Test App',
      icon: 'T',
      createInitialState: () => ({}),
      onWorkspaceLoad: () => ({ dispose: () => {} }),
      render: () => null,
      canClose: true,
      showInNewTabMenu: true,
      displayStyle: 'flex' as const,
      isDefault: false
    }

    const mockApp2 = {
      ...mockApp,
      id: 'test-app-2',
      name: 'Test App 2',
      showInNewTabMenu: false,
      isDefault: true
    }

    beforeEach(() => {
      useAppStore.setState({ applications: new Map() })
    })

    it('registerApplication adds app to state', () => {
      useAppStore.getState().registerApplication(mockApp)
      expect(useAppStore.getState().applications.get('test-app')).toEqual(mockApp)
    })

    it('unregisterApplication removes app from state', () => {
      useAppStore.getState().registerApplication(mockApp)
      useAppStore.getState().unregisterApplication('test-app')
      expect(useAppStore.getState().applications.get('test-app')).toBeUndefined()
    })

    it('getApplication returns app by id', () => {
      useAppStore.getState().registerApplication(mockApp)
      expect(useAppStore.getState().getApplication('test-app')).toEqual(mockApp)
    })

    it('getApplication returns undefined for missing id', () => {
      expect(useAppStore.getState().getApplication('nonexistent')).toBeUndefined()
    })

    it('getAllApplications returns all apps', () => {
      useAppStore.getState().registerApplication(mockApp)
      useAppStore.getState().registerApplication(mockApp2)
      const all = useAppStore.getState().getAllApplications()
      expect(all).toHaveLength(2)
    })

    it('getMenuApplications filters by showInNewTabMenu', () => {
      useAppStore.getState().registerApplication(mockApp)
      useAppStore.getState().registerApplication(mockApp2)
      const menu = useAppStore.getState().getMenuApplications()
      expect(menu).toHaveLength(1)
      expect(menu[0]!.id).toBe('test-app')
    })

    it('getDefaultApplications filters by isDefault', () => {
      useAppStore.getState().registerApplication(mockApp)
      useAppStore.getState().registerApplication(mockApp2)
      const defaults = useAppStore.getState().getDefaultApplications()
      expect(defaults).toHaveLength(1)
      expect(defaults[0]!.id).toBe('test-app-2')
    })

    it('getDefaultApplication returns app by id', () => {
      useAppStore.getState().registerApplication(mockApp)
      useAppStore.getState().registerApplication(mockApp2)
      expect(useAppStore.getState().getDefaultApplication('test-app')?.id).toBe('test-app')
    })

    it('getDefaultApplication falls back to first app', () => {
      useAppStore.getState().registerApplication(mockApp)
      expect(useAppStore.getState().getDefaultApplication()?.id).toBe('test-app')
    })

    it('getDefaultApplication returns null when empty', () => {
      expect(useAppStore.getState().getDefaultApplication()).toBeNull()
    })

    it('getDefaultApplication falls back when id not found', () => {
      useAppStore.getState().registerApplication(mockApp)
      expect(useAppStore.getState().getDefaultApplication('nonexistent')?.id).toBe('test-app')
    })

    it('initializeApplications registers core apps', async () => {
      const cleanup = await useAppStore.getState().initialize(mockDeps)
      const apps = useAppStore.getState().applications
      expect(apps.get('terminal')).toBeDefined()
      expect(apps.get('filesystem')).toBeDefined()
      expect(apps.get('review')).toBeDefined()
      expect(apps.get('editor')).toBeDefined()
      expect(apps.get('comments')).toBeDefined()
      cleanup()
    })

    it('registerTerminalVariants updates base terminal and adds variants', async () => {
      const cleanup = await useAppStore.getState().initialize(mockDeps)
      useAppStore.getState().registerTerminalVariants(
        [{ id: 'custom', name: 'Custom', icon: '>', startupCommand: 'bash', isDefault: false }]
      )
      // Base terminal re-registered
      expect(useAppStore.getState().applications.get('terminal')).toBeDefined()
      cleanup()
    })

    it('registerAiHarnessVariants registers AI apps', async () => {
      const cleanup = await useAppStore.getState().initialize(mockDeps)
      useAppStore.getState().registerAiHarnessVariants([
        { id: 'claude', name: 'Claude', icon: 'C', command: 'claude', isDefault: false, enableSandbox: false, allowNetwork: true, backgroundColor: '#000', disableScrollbar: false, stripScrollbackClear: false }
      ])
      cleanup()
    })

    it('registerCustomRunnerVariants registers custom runner apps', async () => {
      const cleanup = await useAppStore.getState().initialize(mockDeps)
      useAppStore.getState().registerCustomRunnerVariants([
        { id: 'rider', name: 'Rider', icon: '▶', commandTemplate: 'rider {{workspace_path}}', isDefault: false }
      ])
      expect(useAppStore.getState().getApplication('customrunner-test')).toBeDefined()
      cleanup()
    })

    it('registerCustomRunnerVariants unregisters old customrunner- apps before registering new ones', async () => {
      const cleanup = await useAppStore.getState().initialize(mockDeps)
      // Register initial set
      useAppStore.getState().registerCustomRunnerVariants([
        { id: 'rider', name: 'Rider', icon: '▶', commandTemplate: 'rider {{workspace_path}}', isDefault: false }
      ])
      // Register a new set — old ones should be removed
      useAppStore.getState().registerCustomRunnerVariants([
        { id: 'rider2', name: 'Rider2', icon: '▶', commandTemplate: 'rider2 {{workspace_path}}', isDefault: false }
      ])
      cleanup()
    })

    it('registerCustomRunnerVariants with empty array only unregisters existing', async () => {
      const cleanup = await useAppStore.getState().initialize(mockDeps)
      useAppStore.getState().registerCustomRunnerVariants([
        { id: 'rider', name: 'Rider', icon: '▶', commandTemplate: 'rider {{workspace_path}}', isDefault: false }
      ])
      useAppStore.getState().registerCustomRunnerVariants([])
      expect(useAppStore.getState().getApplication('customrunner-rider')).toBeUndefined()
      cleanup()
    })
  })

  describe('session auto-naming', () => {
    beforeEach(async () => {
      useSessionNamesStore.setState({ names: new Map() })
      const cleanup = await useAppStore.getState().initialize(mockDeps)
      cleanup()
    })

    describe('local session via onReady', () => {
      it('names local session LOCAL', () => {
        const readyCallback = vi.mocked(mockDeps.appApi.onReady).mock.calls[0]![0] as (session: any) => void
        readyCallback({ id: 'local-session-1', workspaces: [] })
        expect(useSessionNamesStore.getState().getName('local-session-1')).toBe('LOCAL')
      })

      it('does not overwrite existing custom name', () => {
        useSessionNamesStore.getState().setName('local-session-2', 'My Custom Name')
        const readyCallback = vi.mocked(mockDeps.appApi.onReady).mock.calls[0]![0] as (session: any) => void
        readyCallback({ id: 'local-session-2', workspaces: [] })
        expect(useSessionNamesStore.getState().getName('local-session-2')).toBe('My Custom Name')
      })
    })

    describe('SSH session via addRemoteSession', () => {
      it('names session with user@host when no label', async () => {
        const connection = {
          id: 'conn-1',
          target: { type: 'remote' as const, config: { id: 'conn-1', host: 'myserver.com', user: 'alice', port: 22, portForwards: [] } },
          status: ConnectionStatus.Connected as const
        }
        const session = { id: 'ssh-session-1', workspaces: [], createdAt: 0, lastActivity: 0, version: 1 }
        await useAppStore.getState().addRemoteSession(session, connection)
        expect(useSessionNamesStore.getState().getName('ssh-session-1')).toBe('alice@myserver.com')
      })

      it('uses label over user@host when label is set', async () => {
        const connection = {
          id: 'conn-2',
          target: { type: 'remote' as const, config: { id: 'conn-2', host: 'myserver.com', user: 'alice', port: 22, label: 'Production', portForwards: [] } },
          status: ConnectionStatus.Connected as const
        }
        const session = { id: 'ssh-session-2', workspaces: [], createdAt: 0, lastActivity: 0, version: 1 }
        await useAppStore.getState().addRemoteSession(session, connection)
        expect(useSessionNamesStore.getState().getName('ssh-session-2')).toBe('Production')
      })

      it('does not overwrite existing custom name', async () => {
        useSessionNamesStore.getState().setName('ssh-session-3', 'My Server')
        const connection = {
          id: 'conn-3',
          target: { type: 'remote' as const, config: { id: 'conn-3', host: 'myserver.com', user: 'alice', port: 22, portForwards: [] } },
          status: ConnectionStatus.Connected as const
        }
        const session = { id: 'ssh-session-3', workspaces: [], createdAt: 0, lastActivity: 0, version: 1 }
        await useAppStore.getState().addRemoteSession(session, connection)
        expect(useSessionNamesStore.getState().getName('ssh-session-3')).toBe('My Server')
      })
    })
  })
})
