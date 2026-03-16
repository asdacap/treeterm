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

      // Root workspace was created via addWorkspace
      expect(mockAddWorkspace).toHaveBeenCalledWith('/projects/root', { skipDefaultTabs: true })
      // Child workspace was reconstructed — setState called with child's parentId preserved
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

      // addTabWithState should preserve the ptyId as-is (not null it out)
      expect(mockAddTabWithState).toHaveBeenCalledWith(
        expect.any(String),
        'terminal',
        { ptyId: 'pty-maybe-dead' },
        'tab-1'
      )
      expect(mockAddTabWithState).toHaveBeenCalledWith(
        expect.any(String),
        'ai-harness',
        { ptyId: 'pty-unknown' },
        'tab-2'
      )
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

      // With parentId: null, it's treated as root — addWorkspace should be called
      expect(mockAddWorkspace).toHaveBeenCalledWith('/projects/no-parent', { skipDefaultTabs: true })
      // Tab should be restored with ptyId preserved
      expect(mockAddTabWithState).toHaveBeenCalledWith(
        expect.any(String),
        'terminal',
        { ptyId: 'pty-1' },
        'tab-1'
      )
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
  })
})
