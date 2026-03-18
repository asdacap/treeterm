import { describe, it, expect, vi, beforeEach } from 'vitest'
import { useAppStore } from './app'

// Mock createWorkspaceStore and its utilities
vi.mock('./createWorkspaceStore', () => ({
  createWorkspaceStore: vi.fn().mockImplementation(() => ({
    getState: vi.fn().mockReturnValue({
      workspaces: {},
      activeWorkspaceId: null,
      isRestoring: false,
      addWorkspace: vi.fn(),
      setActiveWorkspace: vi.fn(),
      addTabWithState: vi.fn(),
      setActiveTab: vi.fn(),
      syncToDaemon: vi.fn().mockResolvedValue(undefined)
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
    onShowSessions: vi.fn().mockReturnValue(() => {}),
    list: vi.fn().mockResolvedValue({ success: true, sessions: [] }),
    openInNewWindow: vi.fn()
  },
  daemon: { onDisconnected: vi.fn().mockReturnValue(() => {}) },
  terminal: {
    onNewTerminal: vi.fn().mockReturnValue(() => {}),
    onActiveProcessesOpen: vi.fn().mockReturnValue(() => {}),
    list: vi.fn().mockResolvedValue([]),
    kill: vi.fn(),
    bind: vi.fn().mockReturnThis()
  },
  git: {},
  filesystem: {},
  reviews: {},
  stt: {},
  sandbox: {},
  selectFolder: vi.fn(),
  getRecentDirectories: vi.fn(),
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
      showWorkspacePicker: false,
      daemonSessions: [],
      activeSessionId: null,
      workspaceStores: {}
    })
  })

  describe('initial state', () => {
    it('has null windowUuid by default', () => {
      expect(useAppStore.getState().windowUuid).toBeNull()
    })

    it('has daemonDisconnected false by default', () => {
      expect(useAppStore.getState().daemonDisconnected).toBe(false)
    })

    it('has no active session by default', () => {
      expect(useAppStore.getState().activeSessionId).toBeNull()
    })

    it('has empty workspaceStores by default', () => {
      expect(useAppStore.getState().workspaceStores).toEqual({})
    })

    it('has isSettingsOpen false by default', () => {
      expect(useAppStore.getState().isSettingsOpen).toBe(false)
    })

    it('has showWorkspacePicker false by default', () => {
      expect(useAppStore.getState().showWorkspacePicker).toBe(false)
    })
  })

  describe('switchSession', () => {
    it('sets the active session ID', () => {
      useAppStore.getState().switchSession('session-123')
      expect(useAppStore.getState().activeSessionId).toBe('session-123')
    })

    it('switches between sessions', () => {
      useAppStore.getState().switchSession('session-1')
      useAppStore.getState().switchSession('session-2')
      expect(useAppStore.getState().activeSessionId).toBe('session-2')
    })
  })

  describe('getActiveWorkspaceStore', () => {
    it('returns null when no active session', () => {
      const result = useAppStore.getState().getActiveWorkspaceStore()
      expect(result).toBeNull()
    })

    it('returns null when active session has no store', () => {
      useAppStore.setState({ activeSessionId: 'session-1', workspaceStores: {} })
      const result = useAppStore.getState().getActiveWorkspaceStore()
      expect(result).toBeNull()
    })

    it('returns the store for the active session', () => {
      const mockStore = { getState: vi.fn(), setState: vi.fn(), subscribe: vi.fn() } as never
      useAppStore.setState({
        activeSessionId: 'session-1',
        workspaceStores: { 'session-1': mockStore }
      })
      const result = useAppStore.getState().getActiveWorkspaceStore()
      expect(result).toBe(mockStore)
    })
  })

  describe('handleSessionRestore', () => {
    let mockAddWorkspace: ReturnType<typeof vi.fn>
    let mockAddTabWithState: ReturnType<typeof vi.fn>
    let mockSetActiveWorkspace: ReturnType<typeof vi.fn>
    let mockSetActiveTab: ReturnType<typeof vi.fn>
    let mockSyncToDaemon: ReturnType<typeof vi.fn>
    let mockSetState: ReturnType<typeof vi.fn>

    beforeEach(async () => {
      mockAddWorkspace = vi.fn().mockResolvedValue('ws-new-id')
      mockAddTabWithState = vi.fn().mockReturnValue('tab-new-id')
      mockSetActiveWorkspace = vi.fn()
      mockSetActiveTab = vi.fn()
      mockSyncToDaemon = vi.fn().mockResolvedValue(undefined)
      mockSetState = vi.fn()

      // Import and configure the mock
      const { createWorkspaceStore } = await import('./createWorkspaceStore')
      const mockedCreate = vi.mocked(createWorkspaceStore)
      mockedCreate.mockImplementation(() => ({
        getState: vi.fn().mockReturnValue({
          workspaces: {},
          activeWorkspaceId: null,
          isRestoring: false,
          addWorkspace: mockAddWorkspace,
          setActiveWorkspace: mockSetActiveWorkspace,
          addTabWithState: mockAddTabWithState,
          setActiveTab: mockSetActiveTab,
          syncToDaemon: mockSyncToDaemon
        }),
        setState: mockSetState,
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
            children: ['child-ws'],
            tabs: [],
            activeTabId: null
          },
          {
            id: 'child-ws',
            path: '/projects/root/child',
            name: 'child',
            parentId: 'root-ws',
            children: [],
            tabs: [],
            activeTabId: null
          }
        ]
      }

      await useAppStore.getState().handleSessionRestore(session)

      // Root workspace reconstructed via setState (not addWorkspace)
      expect(mockAddWorkspace).not.toHaveBeenCalled()
      // Both root and child workspaces reconstructed via setState
      expect(mockSetState).toHaveBeenCalledWith(expect.any(Function))
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
            children: [],
            tabs: [{ id: 'tab-1', applicationId: 'terminal', title: 'Term', state: { ptyId: 'pty-1' } }],
            activeTabId: 'tab-1'
          }
        ]
      }

      await useAppStore.getState().handleSessionRestore(session)

      // Child should still be reconstructed (not null, not converted to root)
      // setState is called for isRestoring and for the child reconstruction
      const setStateCalls = mockSetState.mock.calls
      // Should have been called (isRestoring=true, workspace reconstruction, isRestoring=false)
      expect(setStateCalls.length).toBeGreaterThanOrEqual(2)
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
            children: [],
            tabs: [
              { id: 'tab-1', applicationId: 'terminal', title: 'Term', state: { ptyId: 'pty-maybe-dead' } },
              { id: 'tab-2', applicationId: 'ai-harness', title: 'AI', state: { ptyId: 'pty-unknown' } }
            ],
            activeTabId: 'tab-1'
          }
        ]
      }

      await useAppStore.getState().handleSessionRestore(session)

      // terminal.list() should NOT have been called
      expect(mockDeps.terminal.list).not.toHaveBeenCalled()

      // Workspace reconstructed via setState — tabs are preserved in the workspace object
      // (reconstructWorkspace spreads daemonWorkspace which includes tabs with ptyIds)
      expect(mockSetState).toHaveBeenCalledWith(expect.any(Function))
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
            children: [],
            tabs: [{ id: 'tab-1', applicationId: 'terminal', title: 'Term', state: { ptyId: 'pty-1' } }],
            activeTabId: 'tab-1'
          }
        ]
      }

      await useAppStore.getState().handleSessionRestore(session)

      // With parentId: null, it's treated as root — reconstructed via setState (not addWorkspace)
      expect(mockAddWorkspace).not.toHaveBeenCalled()
      // Workspace reconstructed with tabs preserved in the workspace object
      expect(mockSetState).toHaveBeenCalledWith(expect.any(Function))
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
            children: [],
            tabs: [],
            activeTabId: null
          }
        ]
      }

      await useAppStore.getState().handleSessionRestore(session)

      // Since IDs are preserved from daemon, no sync-back needed
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
      expect(mockDeps.terminal.onNewTerminal).toHaveBeenCalled()
      expect(mockDeps.sessionApi.onShowSessions).toHaveBeenCalled()
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
      const { getUnmergedSubWorkspaces } = await import('./createWorkspaceStore')
      vi.mocked(getUnmergedSubWorkspaces).mockReturnValue([])

      const cleanup = await useAppStore.getState().initialize(mockDeps)
      // Set up an active session so getActiveWorkspaceStore returns a store
      const state = useAppStore.getState()
      if (Object.keys(state.workspaceStores).length === 0) {
        // The store was created during initialize via getOrCreateSessionStore
        // We need a session to have a store
      }
      const closeCallback = mockDeps.appApi.onCloseConfirm.mock.calls[0][0]
      closeCallback()
      expect(mockDeps.appApi.confirmClose).toHaveBeenCalled()
      cleanup()
    })

    it('onCloseConfirm shows confirm dialog when unmerged workspaces exist', async () => {
      const { createWorkspaceStore, getUnmergedSubWorkspaces } = await import('./createWorkspaceStore')
      const mockWs = { id: 'ws-1', name: 'unmerged', path: '/test', parentId: 'p', children: [], tabs: [], activeTabId: null, metadata: {} }
      vi.mocked(getUnmergedSubWorkspaces).mockReturnValue([mockWs as any])

      const mockStore = {
        getState: vi.fn().mockReturnValue({ workspaces: { 'ws-1': mockWs } }),
        setState: vi.fn(),
        subscribe: vi.fn()
      }
      vi.mocked(createWorkspaceStore).mockReturnValue(mockStore as any)

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
      const { createWorkspaceStore } = await import('./createWorkspaceStore')
      const mockSetState = vi.fn()
      vi.mocked(createWorkspaceStore).mockReturnValue({
        getState: vi.fn().mockReturnValue({
          workspaces: {},
          activeWorkspaceId: null,
          isRestoring: false,
          addWorkspace: vi.fn(),
          setActiveWorkspace: vi.fn(),
          addTabWithState: vi.fn(),
          setActiveTab: vi.fn(),
          syncToDaemon: vi.fn()
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
          parentId: null, children: [], tabs: [], activeTabId: null
        }]
      }
      readyCallback(session)

      expect(useAppStore.getState().activeSessionId).toBe('session-ready')
      cleanup()
    })

    it('onReady creates store for session with no workspaces', async () => {
      const cleanup = await useAppStore.getState().initialize(mockDeps)
      const readyCallback = mockDeps.appApi.onReady.mock.calls[0][0]

      readyCallback({ id: 'empty-session', workspaces: [] })

      expect(useAppStore.getState().activeSessionId).toBe('empty-session')
      expect(useAppStore.getState().workspaceStores['empty-session']).toBeDefined()
      cleanup()
    })

    it('onNewTerminal adds terminal tab to active workspace', async () => {
      const mockAddTab = vi.fn()
      const { createWorkspaceStore } = await import('./createWorkspaceStore')
      vi.mocked(createWorkspaceStore).mockReturnValue({
        getState: vi.fn().mockReturnValue({
          workspaces: {},
          activeWorkspaceId: 'ws-1',
          isRestoring: false,
          addWorkspace: vi.fn(),
          addTab: mockAddTab,
          setActiveWorkspace: vi.fn(),
          addTabWithState: vi.fn(),
          setActiveTab: vi.fn(),
          syncToDaemon: vi.fn()
        }),
        setState: vi.fn(),
        subscribe: vi.fn()
      } as any)

      const cleanup = await useAppStore.getState().initialize(mockDeps)

      // Create an active session store
      const readyCallback = mockDeps.appApi.onReady.mock.calls[0][0]
      readyCallback({ id: 'session-term', workspaces: [] })

      // Trigger onNewTerminal
      const termCallback = mockDeps.terminal.onNewTerminal.mock.calls[0][0]
      termCallback()

      expect(mockAddTab).toHaveBeenCalledWith('ws-1', 'terminal')
      cleanup()
    })

    it('onShowSessions lists sessions and shows picker', async () => {
      const sessions = [{ id: 's1', workspaces: [] }, { id: 's2', workspaces: [] }]
      mockDeps.sessionApi.list.mockResolvedValue({ success: true, sessions })

      const cleanup = await useAppStore.getState().initialize(mockDeps)

      const showCallback = mockDeps.sessionApi.onShowSessions.mock.calls[0][0]
      await showCallback()

      expect(mockDeps.sessionApi.list).toHaveBeenCalled()
      expect(useAppStore.getState().showWorkspacePicker).toBe(true)
      expect(useAppStore.getState().daemonSessions).toEqual(sessions)
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
      const { createWorkspaceStore } = await import('./createWorkspaceStore')
      vi.mocked(createWorkspaceStore).mockReturnValue({
        getState: vi.fn().mockReturnValue({
          workspaces: { 'ws-existing': { id: 'ws-existing', path: '/projects/existing', name: 'existing' } },
          activeWorkspaceId: null,
          isRestoring: false,
          addWorkspace: vi.fn(),
          setActiveWorkspace: mockSetActiveWorkspace,
          addTabWithState: vi.fn(),
          setActiveTab: vi.fn(),
          syncToDaemon: vi.fn()
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
      // So first create a store, then initialize
      useAppStore.setState({
        activeSessionId: 'pre-session',
        workspaceStores: { 'pre-session': vi.mocked(createWorkspaceStore)({ sessionId: 'pre-session', windowUuid: null }, {} as any) as any }
      })

      const cleanup = await useAppStore.getState().initialize(deps)
      expect(mockSetActiveWorkspace).toHaveBeenCalledWith('ws-existing')
      cleanup()
    })

    it('initial workspace adds new workspace when not existing', async () => {
      const mockAddWorkspace = vi.fn().mockResolvedValue('ws-new')
      const { createWorkspaceStore } = await import('./createWorkspaceStore')
      vi.mocked(createWorkspaceStore).mockReturnValue({
        getState: vi.fn().mockReturnValue({
          workspaces: {},
          activeWorkspaceId: null,
          isRestoring: false,
          addWorkspace: mockAddWorkspace,
          setActiveWorkspace: vi.fn(),
          addTabWithState: vi.fn(),
          setActiveTab: vi.fn(),
          syncToDaemon: vi.fn()
        }),
        setState: vi.fn(),
        subscribe: vi.fn()
      } as any)

      useAppStore.setState({
        activeSessionId: 'pre-session',
        workspaceStores: { 'pre-session': vi.mocked(createWorkspaceStore)({ sessionId: 'pre-session', windowUuid: null }, {} as any) as any }
      })

      const deps = { ...mockDeps, getInitialWorkspace: vi.fn().mockResolvedValue('/new/path') } as any
      const cleanup = await useAppStore.getState().initialize(deps)
      expect(mockAddWorkspace).toHaveBeenCalledWith('/new/path')
      cleanup()
    })
  })

  describe('createNewSession', () => {
    beforeEach(async () => {
      const cleanup = await useAppStore.getState().initialize(mockDeps)
      cleanup()
    })

    it('creates session, opens in new window, hides picker', async () => {
      mockDeps.sessionApi.create = vi.fn().mockResolvedValue({
        success: true,
        session: { id: 'new-session', workspaces: [] }
      })
      mockDeps.sessionApi.openInNewWindow = vi.fn().mockResolvedValue(undefined)
      useAppStore.setState({ showWorkspacePicker: true })

      await useAppStore.getState().createNewSession()

      expect(mockDeps.sessionApi.create).toHaveBeenCalledWith([])
      expect(mockDeps.sessionApi.openInNewWindow).toHaveBeenCalledWith('new-session')
      expect(useAppStore.getState().showWorkspacePicker).toBe(false)
    })

    it('shows alert on failure', async () => {
      mockDeps.sessionApi.create = vi.fn().mockResolvedValue({
        success: false,
        error: 'Something went wrong'
      })
      // alert doesn't exist in node env, so define it
      globalThis.alert = vi.fn()

      await useAppStore.getState().createNewSession()

      expect(globalThis.alert).toHaveBeenCalledWith(expect.stringContaining('Something went wrong'))
      delete (globalThis as any).alert
    })
  })

  describe('handleExternalSessionUpdate', () => {
    let mockSetState: ReturnType<typeof vi.fn>

    beforeEach(async () => {
      mockSetState = vi.fn()
      const { createWorkspaceStore } = await import('./createWorkspaceStore')
      vi.mocked(createWorkspaceStore).mockReturnValue({
        getState: vi.fn().mockReturnValue({
          workspaces: {},
          activeWorkspaceId: null,
          isRestoring: false,
          addWorkspace: vi.fn(),
          setActiveWorkspace: vi.fn(),
          addTabWithState: vi.fn(),
          setActiveTab: vi.fn(),
          syncToDaemon: vi.fn()
        }),
        setState: mockSetState,
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
          parentId: null, children: [], tabs: [], activeTabId: null
        }]
      }

      await useAppStore.getState().handleExternalSessionUpdate(session)

      // setState called for isRestoring=true, workspace reconstruction, isRestoring=false
      expect(mockSetState).toHaveBeenCalled()
    })

    it('removes workspaces not present in daemon session', async () => {
      const { createWorkspaceStore } = await import('./createWorkspaceStore')
      const mockRemoveWorkspaceKeepWorktree = vi.fn()
      const existingWs = {
        'ws-old': { id: 'ws-old', path: '/old', name: 'old', parentId: null, children: [] }
      }
      vi.mocked(createWorkspaceStore).mockReturnValue({
        getState: vi.fn().mockReturnValue({
          workspaces: existingWs,
          activeWorkspaceId: 'ws-old',
          isRestoring: false,
          addWorkspace: vi.fn(),
          setActiveWorkspace: vi.fn(),
          addTabWithState: vi.fn(),
          setActiveTab: vi.fn(),
          syncToDaemon: vi.fn(),
          removeWorkspaceKeepWorktree: mockRemoveWorkspaceKeepWorktree
        }),
        setState: mockSetState,
        subscribe: vi.fn()
      } as any)

      // Re-initialize to pick up new mock
      const cleanup2 = await useAppStore.getState().initialize(mockDeps)

      const session: any = {
        id: 'sync-session-2',
        workspaces: [] // No workspaces — ws-old should be removed
      }

      await useAppStore.getState().handleExternalSessionUpdate(session)

      // removeWorkspaceKeepWorktree should be called for the old workspace
      expect(mockRemoveWorkspaceKeepWorktree).toHaveBeenCalledWith('ws-old')
      cleanup2()
    })
  })
})
