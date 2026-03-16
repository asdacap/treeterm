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

// Mock window.electron
const mockElectron = {
  getWindowUuid: vi.fn().mockResolvedValue('test-uuid'),
  getInitialWorkspace: vi.fn().mockResolvedValue(null),
  settings: { onOpen: vi.fn().mockReturnValue(() => {}) },
  app: {
    onCloseConfirm: vi.fn().mockReturnValue(() => {}),
    onReady: vi.fn().mockReturnValue(() => {}),
    confirmClose: vi.fn(),
    cancelClose: vi.fn()
  },
  session: {
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
  }
}

;(globalThis as unknown as { window: unknown }).window = { electron: mockElectron }

describe('useAppStore', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    useAppStore.setState({
      electron: null,
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

  describe('initialize', () => {
    it('fetches window UUID on initialization', async () => {
      const cleanup = await useAppStore.getState().initialize()
      expect(mockElectron.getWindowUuid).toHaveBeenCalled()
      expect(useAppStore.getState().windowUuid).toBe('test-uuid')
      cleanup()
    })

    it('returns a cleanup function', async () => {
      const cleanup = await useAppStore.getState().initialize()
      expect(typeof cleanup).toBe('function')
      // Should not throw
      cleanup()
    })

    it('wires IPC event listeners', async () => {
      const cleanup = await useAppStore.getState().initialize()
      expect(mockElectron.settings.onOpen).toHaveBeenCalled()
      expect(mockElectron.app.onCloseConfirm).toHaveBeenCalled()
      expect(mockElectron.app.onReady).toHaveBeenCalled()
      expect(mockElectron.session.onSync).toHaveBeenCalled()
      expect(mockElectron.daemon.onDisconnected).toHaveBeenCalled()
      expect(mockElectron.terminal.onNewTerminal).toHaveBeenCalled()
      expect(mockElectron.session.onShowSessions).toHaveBeenCalled()
      cleanup()
    })
  })
})
