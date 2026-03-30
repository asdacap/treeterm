import { describe, it, expect, vi, beforeEach } from 'vitest'

// Mock application renderers that depend on browser APIs (xterm)
vi.mock('../../applications/terminal/renderer', () => ({
  createTerminalApplication: vi.fn().mockReturnValue({
    id: 'terminal', name: 'Terminal', icon: '>', createInitialState: () => ({}),
    render: () => null, onWorkspaceLoad: () => ({ dispose: () => {} }), canClose: true, showInNewTabMenu: true,
    displayStyle: 'flex', isDefault: true
  }),
  createTerminalVariant: vi.fn().mockReturnValue({
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
  createAiHarnessVariant: vi.fn().mockReturnValue({
    id: 'aiharness-test', name: 'AI', icon: 'A', createInitialState: () => ({}),
    render: () => null, onWorkspaceLoad: () => ({ dispose: () => {} }), canClose: true, showInNewTabMenu: true,
    displayStyle: 'flex', isDefault: false
  })
}))

vi.mock('../../applications/customRunner/renderer', () => ({
  createCustomRunnerVariant: vi.fn().mockReturnValue({
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


import { useAppStore } from './app'
import { useNavigationStore } from './navigation'
import { useSessionNamesStore } from './sessionNames'

// Mock createSessionStore and its utilities
vi.mock('./createSessionStore', () => ({
  createSessionStore: vi.fn().mockImplementation(() => ({
    getState: vi.fn().mockReturnValue({
      workspaces: {},
      workspaceStores: {},
      activeWorkspaceId: null,
      isRestoring: false,
      addWorkspace: vi.fn(),
      setActiveWorkspace: vi.fn(),
      getWorkspace: vi.fn().mockReturnValue(null),
      syncToDaemon: vi.fn().mockResolvedValue(undefined),
      handleRestore: vi.fn().mockResolvedValue(undefined),
      handleExternalUpdate: vi.fn().mockResolvedValue(undefined),
      removeOrphanWorkspace: vi.fn(),
    }),
    setState: vi.fn(),
    subscribe: vi.fn()
  })),
  getUnmergedSubWorkspaces: vi.fn().mockReturnValue([])
}))

vi.mock('./settings', () => ({
  useSettingsStore: {
    getState: vi.fn().mockReturnValue({
      init: vi.fn()
    })
  }
}))

// Mock deps for initialize
const mockDeps = {
  platform: 'darwin' as const,
  getWindowUuid: vi.fn().mockResolvedValue('test-uuid'),
  getInitialWorkspace: vi.fn().mockResolvedValue(null),
  settingsApi: { onOpen: vi.fn().mockReturnValue(() => {}) },
  appApi: {
    onCloseConfirm: vi.fn().mockReturnValue(() => {}),
    onReady: vi.fn().mockReturnValue(() => {}),
    confirmClose: vi.fn(),
    cancelClose: vi.fn()
  },
  sessionApi: {
    onSync: vi.fn().mockReturnValue(() => {}),
    update: vi.fn().mockResolvedValue({ success: true, session: { id: 'test-session' } })
  },
  daemon: { onDisconnected: vi.fn().mockReturnValue(() => {}) },
  terminal: {
    onActiveProcessesOpen: vi.fn().mockReturnValue(() => {}),
    list: vi.fn().mockResolvedValue([]),
    kill: vi.fn(),
    bind: vi.fn().mockReturnThis()
  },
  git: { onOutput: vi.fn().mockReturnValue(() => {}) },
  github: { getPrInfo: vi.fn() },
  filesystem: {},
  reviews: {},
  stt: {},
  runActions: { detect: vi.fn().mockResolvedValue([]), run: vi.fn().mockResolvedValue(null) },
  sandbox: {},
  ssh: {
    connect: vi.fn().mockResolvedValue({ id: 'test', target: { type: 'remote' }, status: 'connected' }),
    disconnect: vi.fn().mockResolvedValue(undefined),
    listConnections: vi.fn().mockResolvedValue([]),
    saveConnection: vi.fn().mockResolvedValue(undefined),
    getSavedConnections: vi.fn().mockResolvedValue([]),
    removeSavedConnection: vi.fn().mockResolvedValue(undefined),
    getOutput: vi.fn().mockResolvedValue([]),
    onConnectionStatus: vi.fn().mockReturnValue(() => {}),
    onOutput: vi.fn().mockReturnValue(() => {})
  },
  selectFolder: vi.fn(),
  llm: {
    analyzeTerminal: vi.fn(),
    generateTitle: vi.fn(),
  },
} as any

describe('useAppStore', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    useAppStore.setState({
      windowUuid: null,
      daemonDisconnected: false,
      isSettingsOpen: false,
      showCloseConfirm: false,
      unmergedWorkspaces: [],
      sessionStores: {}
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
      expect(useAppStore.getState().sessionStores).toEqual({})
    })

    it('has isSettingsOpen false by default', () => {
      expect(useAppStore.getState().isSettingsOpen).toBe(false)
    })

  })

  describe('disconnectSession', () => {
    it('removes session from sessionStores', () => {
      const mockStore = {
        getState: vi.fn().mockReturnValue({ workspaces: {} }),
        setState: vi.fn(),
        subscribe: vi.fn()
      } as never
      useAppStore.setState({ sessionStores: {
        's1': { status: 'connected', store: mockStore },
        's2': { status: 'connected', store: mockStore }
      } })
      useAppStore.getState().disconnectSession('s1')
      expect(useAppStore.getState().sessionStores['s1']).toBeUndefined()
      expect(useAppStore.getState().sessionStores['s2']).toBeDefined()
    })

    it('clears navigation when disconnecting viewed session', () => {
      const mockStore = {
        getState: vi.fn().mockReturnValue({ workspaces: { 'ws-1': { id: 'ws-1' } } }),
        setState: vi.fn(),
        subscribe: vi.fn()
      } as never
      useAppStore.setState({ sessionStores: {
        's1': { status: 'connected', store: mockStore },
        's2': { status: 'connected', store: mockStore }
      } })
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
      mockAddWorkspace = vi.fn().mockResolvedValue('ws-new-id')
      mockSyncToDaemon = vi.fn().mockResolvedValue(undefined)
      mockHandleRestore = vi.fn().mockResolvedValue(undefined)

      // Import and configure the mock
      const { createSessionStore } = await import('./createSessionStore')
      const mockedCreate = vi.mocked(createSessionStore)
      mockedCreate.mockImplementation(() => ({
        getState: vi.fn().mockReturnValue({
          workspaces: {},
          workspaceStores: {},
          activeWorkspaceId: null,
          isRestoring: false,
          addWorkspace: mockAddWorkspace,
          setActiveWorkspace: vi.fn(),
          getWorkspace: vi.fn().mockReturnValue(null),
          syncToDaemon: mockSyncToDaemon,
          handleRestore: mockHandleRestore,
          handleExternalUpdate: vi.fn().mockResolvedValue(undefined),
        }),
        setState: vi.fn(),
        subscribe: vi.fn()
      }) as any)

      // Initialize the store with deps so terminal/git/etc are available
      const cleanup = await useAppStore.getState().initialize(mockDeps)
      cleanup()
    })

    it('restores root + child workspaces with parent-child links', async () => {
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
      const readyCallback = mockDeps.appApi.onReady.mock.calls[0][0]
      readyCallback(session)

      // Workspaces restored via handleRestore (not addWorkspace)
      expect(mockAddWorkspace).not.toHaveBeenCalled()
      expect(mockHandleRestore).toHaveBeenCalledWith(session)
    })

    it('child workspace restored even when parent not in state', async () => {
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

      const readyCallback = mockDeps.appApi.onReady.mock.calls[0][0]
      readyCallback(session)

      // Child should still be restored via handleRestore
      expect(mockHandleRestore).toHaveBeenCalledWith(session)
    })

    it('PTY IDs preserved as-is without sessionMap validation', async () => {
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

      const readyCallback = mockDeps.appApi.onReady.mock.calls[0][0]
      readyCallback(session)

      // terminal.list() should NOT have been called
      expect(mockDeps.terminal.list).not.toHaveBeenCalled()
      expect(mockHandleRestore).toHaveBeenCalledWith(session)
    })

    it('child without parentId still restored', async () => {
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

      const readyCallback = mockDeps.appApi.onReady.mock.calls[0][0]
      readyCallback(session)

      expect(mockAddWorkspace).not.toHaveBeenCalled()
      expect(mockHandleRestore).toHaveBeenCalledWith(session)
    })

    it('syncToDaemon is not called after restore', async () => {
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

      const readyCallback = mockDeps.appApi.onReady.mock.calls[0][0]
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
      const closeCallback = mockDeps.appApi.onCloseConfirm.mock.calls[0][0]
      closeCallback()
      expect(mockDeps.appApi.confirmClose).toHaveBeenCalled()
      cleanup()
    })

    it('onCloseConfirm confirms close when no unmerged workspaces', async () => {
      const { getUnmergedSubWorkspaces } = await import('./createSessionStore')
      vi.mocked(getUnmergedSubWorkspaces).mockReturnValue([])

      const cleanup = await useAppStore.getState().initialize(mockDeps)
      const closeCallback = mockDeps.appApi.onCloseConfirm.mock.calls[0][0]
      closeCallback()
      expect(mockDeps.appApi.confirmClose).toHaveBeenCalled()
      cleanup()
    })

    it('onCloseConfirm shows confirm dialog when unmerged workspaces exist', async () => {
      const { createSessionStore, getUnmergedSubWorkspaces } = await import('./createSessionStore')
      const mockWs = { id: 'ws-1', name: 'unmerged', path: '/test', parentId: 'p', appStates: {}, activeTabId: null, metadata: {} }
      vi.mocked(getUnmergedSubWorkspaces).mockReturnValue([mockWs as any])

      const mockStore = {
        getState: vi.fn().mockReturnValue({
          workspaces: { 'ws-1': mockWs },
          workspaceStores: {},
          handleRestore: vi.fn().mockResolvedValue(undefined),
          handleExternalUpdate: vi.fn().mockResolvedValue(undefined),
        }),
        setState: vi.fn(),
        subscribe: vi.fn()
      }
      vi.mocked(createSessionStore).mockReturnValue(mockStore as any)

      const cleanup = await useAppStore.getState().initialize(mockDeps)

      // Trigger onReady to create a session store
      const readyCallback = mockDeps.appApi.onReady.mock.calls[0][0]
      readyCallback({ id: 'session-1', workspaces: [] })

      const closeCallback = mockDeps.appApi.onCloseConfirm.mock.calls[0][0]
      closeCallback()

      expect(useAppStore.getState().showCloseConfirm).toBe(true)
      expect(useAppStore.getState().unmergedWorkspaces).toEqual([mockWs])
      cleanup()
    })

    it('onReady restores session with workspaces', async () => {
      const { createSessionStore } = await import('./createSessionStore')
      const mockSetState = vi.fn()
      vi.mocked(createSessionStore).mockReturnValue({
        getState: vi.fn().mockReturnValue({
          workspaces: {},
          workspaceStores: {},
          activeWorkspaceId: null,
          isRestoring: false,
          addWorkspace: vi.fn(),
          setActiveWorkspace: vi.fn(),
          getWorkspace: vi.fn().mockReturnValue(null),
          syncToDaemon: vi.fn(),
          handleRestore: vi.fn().mockResolvedValue(undefined),
          handleExternalUpdate: vi.fn().mockResolvedValue(undefined),
        }),
        setState: mockSetState,
        subscribe: vi.fn()
      } as any)

      const cleanup = await useAppStore.getState().initialize(mockDeps)
      const readyCallback = mockDeps.appApi.onReady.mock.calls[0][0]

      const session = {
        id: 'session-ready',
        workspaces: [{
          id: 'ws-1', path: '/test', name: 'test',
          parentId: null, appStates: {}, activeTabId: null
        }]
      }
      readyCallback(session)

      expect(useAppStore.getState().sessionStores['session-ready']).toBeDefined()
      cleanup()
    })

    it('onReady creates store for session with no workspaces', async () => {
      const cleanup = await useAppStore.getState().initialize(mockDeps)
      const readyCallback = mockDeps.appApi.onReady.mock.calls[0][0]

      readyCallback({ id: 'empty-session', workspaces: [] })

      expect(useAppStore.getState().sessionStores['empty-session']).toBeDefined()
      cleanup()
    })

    it('onDisconnected sets daemonDisconnected', async () => {
      const cleanup = await useAppStore.getState().initialize(mockDeps)

      const disconnectCallback = mockDeps.daemon.onDisconnected.mock.calls[0][0]
      disconnectCallback()

      expect(useAppStore.getState().daemonDisconnected).toBe(true)
      cleanup()
    })

    it('initial workspace activates existing workspace', async () => {
      const mockSetActiveWorkspace = vi.fn()
      const { createSessionStore } = await import('./createSessionStore')
      vi.mocked(createSessionStore).mockReturnValue({
        getState: vi.fn().mockReturnValue({
          workspaces: { 'ws-existing': { status: 'loaded', data: { id: 'ws-existing', path: '/projects/existing', name: 'existing' }, store: {} } },
          activeWorkspaceId: null,
          isRestoring: false,
          addWorkspace: vi.fn(),
          setActiveWorkspace: mockSetActiveWorkspace,
          syncToDaemon: vi.fn(),
          handleRestore: vi.fn().mockResolvedValue(undefined),
          handleExternalUpdate: vi.fn().mockResolvedValue(undefined),
        }),
        setState: vi.fn(),
        subscribe: vi.fn()
      } as any)

      // Set up initial workspace to return a path
      const deps = {
        ...mockDeps,
        getInitialWorkspace: vi.fn().mockResolvedValue('/projects/existing'),
        appApi: {
          ...mockDeps.appApi,
          onReady: vi.fn().mockImplementation((cb: any) => {
            // Immediately fire onReady with a session so the store exists
            setTimeout(() => cb({ id: 'session-init', workspaces: [] }), 0)
            return () => {}
          }),
          onCloseConfirm: vi.fn().mockReturnValue(() => {}),
        }
      } as any

      // We need the store to exist before getInitialWorkspace resolves
      // Create a mock session store directly
      const mockSessionStoreInstance = vi.mocked(createSessionStore)({ sessionId: 'pre-session', windowUuid: null }, {} as any) as any
      useAppStore.setState({
        sessionStores: { 'pre-session': { status: 'connected', store: mockSessionStoreInstance } }
      })

      const cleanup = await useAppStore.getState().initialize(deps)
      expect(mockSetActiveWorkspace).toHaveBeenCalledWith('ws-existing')
      cleanup()
    })

    it('initial workspace adds new workspace when not existing', async () => {
      const mockAddWorkspace = vi.fn().mockResolvedValue('ws-new')
      const { createSessionStore } = await import('./createSessionStore')
      vi.mocked(createSessionStore).mockReturnValue({
        getState: vi.fn().mockReturnValue({
          workspaces: {},
          activeWorkspaceId: null,
          isRestoring: false,
          addWorkspace: mockAddWorkspace,
          setActiveWorkspace: vi.fn(),
          syncToDaemon: vi.fn(),
          handleRestore: vi.fn().mockResolvedValue(undefined),
          handleExternalUpdate: vi.fn().mockResolvedValue(undefined),
        }),
        setState: vi.fn(),
        subscribe: vi.fn()
      } as any)

      const mockSessionStoreInstance = vi.mocked(createSessionStore)({ sessionId: 'pre-session', windowUuid: null }, {} as any) as any
      useAppStore.setState({
        sessionStores: { 'pre-session': { status: 'connected', store: mockSessionStoreInstance } }
      })

      const deps = { ...mockDeps, getInitialWorkspace: vi.fn().mockResolvedValue('/new/path') } as any
      const cleanup = await useAppStore.getState().initialize(deps)
      expect(mockAddWorkspace).toHaveBeenCalledWith('/new/path')
      cleanup()
    })
  })

  describe('handleExternalSessionUpdate via onSync', () => {
    let mockHandleExternalUpdate: ReturnType<typeof vi.fn>

    beforeEach(async () => {
      mockHandleExternalUpdate = vi.fn().mockResolvedValue(undefined)
      const { createSessionStore } = await import('./createSessionStore')
      vi.mocked(createSessionStore).mockReturnValue({
        getState: vi.fn().mockReturnValue({
          workspaces: {},
          workspaceStores: {},
          activeWorkspaceId: null,
          isRestoring: false,
          addWorkspace: vi.fn(),
          setActiveWorkspace: vi.fn(),
          getWorkspace: vi.fn().mockReturnValue(null),
          syncToDaemon: vi.fn(),
          handleRestore: vi.fn().mockResolvedValue(undefined),
          handleExternalUpdate: mockHandleExternalUpdate,
        }),
        setState: vi.fn(),
        subscribe: vi.fn()
      } as any)

      const cleanup = await useAppStore.getState().initialize(mockDeps)
      cleanup()
    })

    it('syncs new workspaces from daemon', async () => {
      const session: any = {
        id: 'sync-session',
        workspaces: [{
          id: 'ws-new', path: '/new', name: 'new',
          parentId: null, appStates: {}, activeTabId: null
        }]
      }

      // Trigger onSync
      const syncCallback = mockDeps.sessionApi.onSync.mock.calls[0][0]
      await syncCallback(session)

      // handleExternalUpdate called to sync workspaces
      expect(mockHandleExternalUpdate).toHaveBeenCalledWith(session)
    })

    it('delegates external update handling to session store', async () => {
      const session: any = {
        id: 'sync-session-2',
        workspaces: [] // No workspaces — removal handled inside session store
      }

      // Trigger onSync
      const syncCallback = mockDeps.sessionApi.onSync.mock.calls[0][0]
      await syncCallback(session)

      // handleExternalUpdate is responsible for removing orphan workspaces internally
      expect(mockHandleExternalUpdate).toHaveBeenCalledWith(session)
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
      useAppStore.setState({ applications: {} })
    })

    it('registerApplication adds app to state', () => {
      useAppStore.getState().registerApplication(mockApp)
      expect(useAppStore.getState().applications['test-app']).toEqual(mockApp)
    })

    it('unregisterApplication removes app from state', () => {
      useAppStore.getState().registerApplication(mockApp)
      useAppStore.getState().unregisterApplication('test-app')
      expect(useAppStore.getState().applications['test-app']).toBeUndefined()
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
      expect(menu[0].id).toBe('test-app')
    })

    it('getDefaultApplications filters by isDefault', () => {
      useAppStore.getState().registerApplication(mockApp)
      useAppStore.getState().registerApplication(mockApp2)
      const defaults = useAppStore.getState().getDefaultApplications()
      expect(defaults).toHaveLength(1)
      expect(defaults[0].id).toBe('test-app-2')
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
      expect(apps['terminal']).toBeDefined()
      expect(apps['filesystem']).toBeDefined()
      expect(apps['review']).toBeDefined()
      expect(apps['editor']).toBeDefined()
      expect(apps['comments']).toBeDefined()
      cleanup()
    })

    it('registerTerminalVariants updates base terminal and adds variants', async () => {
      const cleanup = await useAppStore.getState().initialize(mockDeps)
      useAppStore.getState().registerTerminalVariants(
        [{ id: 'custom', name: 'Custom', icon: '>', startupCommand: 'bash', isDefault: false }]
      )
      // Base terminal re-registered
      expect(useAppStore.getState().applications['terminal']).toBeDefined()
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
      useSessionNamesStore.setState({ names: {} })
      const cleanup = await useAppStore.getState().initialize(mockDeps)
      cleanup()
    })

    describe('local session via onReady', () => {
      it('names local session LOCAL', () => {
        const readyCallback = mockDeps.appApi.onReady.mock.calls[0][0]
        readyCallback({ id: 'local-session-1', workspaces: [] })
        expect(useSessionNamesStore.getState().getName('local-session-1')).toBe('LOCAL')
      })

      it('does not overwrite existing custom name', () => {
        useSessionNamesStore.getState().setName('local-session-2', 'My Custom Name')
        const readyCallback = mockDeps.appApi.onReady.mock.calls[0][0]
        readyCallback({ id: 'local-session-2', workspaces: [] })
        expect(useSessionNamesStore.getState().getName('local-session-2')).toBe('My Custom Name')
      })
    })

    describe('SSH session via addRemoteSession', () => {
      it('names session with user@host when no label', async () => {
        const connection = {
          id: 'conn-1',
          target: { type: 'remote' as const, config: { id: 'conn-1', host: 'myserver.com', user: 'alice', port: 22, portForwards: [] } },
          status: 'connected' as const
        }
        const session = { id: 'ssh-session-1', workspaces: [], createdAt: 0, lastActivity: 0, version: 1 }
        await useAppStore.getState().addRemoteSession(session, connection)
        expect(useSessionNamesStore.getState().getName('ssh-session-1')).toBe('alice@myserver.com')
      })

      it('uses label over user@host when label is set', async () => {
        const connection = {
          id: 'conn-2',
          target: { type: 'remote' as const, config: { id: 'conn-2', host: 'myserver.com', user: 'alice', port: 22, label: 'Production', portForwards: [] } },
          status: 'connected' as const
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
          status: 'connected' as const
        }
        const session = { id: 'ssh-session-3', workspaces: [], createdAt: 0, lastActivity: 0, version: 1 }
        await useAppStore.getState().addRemoteSession(session, connection)
        expect(useSessionNamesStore.getState().getName('ssh-session-3')).toBe('My Server')
      })
    })
  })
})
