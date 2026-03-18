import { create } from 'zustand'
import type { StoreApi } from 'zustand'
import { createWorkspaceStore } from './createWorkspaceStore'
import type { WorkspaceState } from './createWorkspaceStore'
import { useSettingsStore } from './settings'
import { getUnmergedSubWorkspaces } from './createWorkspaceStore'
import { applicationRegistry } from '../registry/applicationRegistry'
import type {
  Workspace, Session,
  Platform, TerminalApi, GitApi, SessionApi, AppApi, DaemonApi,
  FilesystemApi, STTApi, SandboxApi, SettingsApi
} from '../types'

type AddTabWithStateFn = <T>(workspaceId: string, applicationId: string, initialState: Partial<T>, existingTabId?: string) => string
type SetActiveTabFn = (workspaceId: string, tabId: string) => void

export interface AppDeps {
  platform: Platform
  terminal: TerminalApi
  git: GitApi
  sessionApi: SessionApi
  settingsApi: SettingsApi
  appApi: AppApi
  daemon: DaemonApi
  filesystem: FilesystemApi
  stt: STTApi
  sandbox: SandboxApi
  selectFolder: () => Promise<string | null>
  getRecentDirectories: () => Promise<string[]>
  getWindowUuid: () => Promise<string>
  getInitialWorkspace: () => Promise<string | null>
}

interface AppState extends AppDeps {
  // Lifecycle
  windowUuid: string | null
  daemonDisconnected: boolean
  isSettingsOpen: boolean
  isActiveProcessesOpen: boolean
  showCloseConfirm: boolean
  unmergedWorkspaces: Workspace[]
  showWorkspacePicker: boolean
  daemonSessions: Session[]

  // Session management
  activeSessionId: string | null
  workspaceStores: Record<string, StoreApi<WorkspaceState>>

  // Actions
  initialize: (deps: AppDeps) => Promise<() => void>
  switchSession: (sessionId: string) => void
  getActiveWorkspaceStore: () => StoreApi<WorkspaceState> | null

  // Internal (moved from App.tsx)
  createNewSession: () => Promise<void>
  handleSessionRestore: (session: Session) => Promise<void>
  handleExternalSessionUpdate: (session: Session) => Promise<void>
}

// Placeholder used before initialize() injects real deps.
// Safe because initialize() is called before any component renders.
const UNINITIALIZED = null as never

export const useAppStore = create<AppState>()((set, get) => ({
  // Injected APIs — overwritten by initialize(deps) before first use
  platform: UNINITIALIZED,
  terminal: UNINITIALIZED,
  git: UNINITIALIZED,
  sessionApi: UNINITIALIZED,
  settingsApi: UNINITIALIZED,
  appApi: UNINITIALIZED,
  daemon: UNINITIALIZED,
  filesystem: UNINITIALIZED,
  stt: UNINITIALIZED,
  sandbox: UNINITIALIZED,
  selectFolder: UNINITIALIZED,
  getRecentDirectories: UNINITIALIZED,
  getWindowUuid: UNINITIALIZED,
  getInitialWorkspace: UNINITIALIZED,

  windowUuid: null,
  daemonDisconnected: false,
  isSettingsOpen: false,
  isActiveProcessesOpen: false,
  showCloseConfirm: false,
  unmergedWorkspaces: [],
  showWorkspacePicker: false,
  daemonSessions: [],
  activeSessionId: null,
  workspaceStores: {},

  initialize: async (deps: AppDeps) => {
    set(deps)
    const { terminal, git, sessionApi, settingsApi, appApi, daemon, getWindowUuid, getInitialWorkspace } = deps

    useSettingsStore.getState().init(settingsApi, terminal.kill.bind(terminal))

    // Fetch this window's UUID
    try {
      const uuid = await getWindowUuid()
      if (uuid) {
        set({ windowUuid: uuid })
        console.log('[App] Window UUID:', uuid)
      }
    } catch (error) {
      console.error('[App] Failed to fetch window UUID:', error)
    }

    const unsubSettings = settingsApi.onOpen(() => {
      set({ isSettingsOpen: true })
    })

    const unsubClose = appApi.onCloseConfirm(() => {
      const activeStore = get().getActiveWorkspaceStore()
      if (!activeStore) {
        appApi.confirmClose()
        return
      }
      const unmerged = getUnmergedSubWorkspaces(activeStore.getState().workspaces)
      if (unmerged.length > 0) {
        set({ unmergedWorkspaces: unmerged, showCloseConfirm: true })
      } else {
        appApi.confirmClose()
      }
    })

    const unsubReady = appApi.onReady((session) => {
      console.log('[App] Received app:ready with session:', session?.id)
      if (session) {
        if (session.workspaces && session.workspaces.length > 0) {
          get().handleSessionRestore(session)
        } else {
          // Ensure we have a store for this session even with no workspaces
          getOrCreateSessionStore(session.id, get, set)
          set({ activeSessionId: session.id })
        }
      }
    })

    const unsubSync = sessionApi.onSync((session) => {
      console.log('[App] Received session:sync with', session.workspaces.length, 'workspaces')
      get().handleExternalSessionUpdate(session)
    })

    const unsubDisconnect = daemon.onDisconnected(() => {
      console.error('[App] Daemon disconnected')
      set({ daemonDisconnected: true })
    })

    const unsubNewTerminal = terminal.onNewTerminal(() => {
      const activeStore = get().getActiveWorkspaceStore()
      if (!activeStore) return
      const { activeWorkspaceId, addTab } = activeStore.getState()
      if (activeWorkspaceId) {
        addTab(activeWorkspaceId, 'terminal')
      }
    })

    const unsubActiveProcesses = terminal.onActiveProcessesOpen(() => {
      set({ isActiveProcessesOpen: true })
    })

    const unsubShowSessions = sessionApi.onShowSessions(async () => {
      try {
        const result = await sessionApi.list()
        if (result.success && result.sessions) {
          set({ daemonSessions: result.sessions, showWorkspacePicker: true })
        }
      } catch (error) {
        console.error('Failed to list daemon sessions:', error)
        alert(`Failed to list sessions: ${error instanceof Error ? error.message : 'Unknown error'}`)
      }
    })

    // Handle initial workspace from CLI
    const initialPath = await getInitialWorkspace()
    if (initialPath) {
      const activeStore = get().getActiveWorkspaceStore()
      if (activeStore) {
        const { workspaces, addWorkspace, setActiveWorkspace } = activeStore.getState()
        const existingWorkspace = Object.values(workspaces).find(ws => ws.path === initialPath)
        if (existingWorkspace) {
          setActiveWorkspace(existingWorkspace.id)
        } else {
          await addWorkspace(initialPath)
        }
      }
    }

    return () => {
      unsubSettings()
      unsubClose()
      unsubReady()
      unsubSync()
      unsubDisconnect()
      unsubNewTerminal()
      unsubActiveProcesses()
      unsubShowSessions()
    }
  },

  switchSession: (sessionId: string) => {
    set({ activeSessionId: sessionId })
  },

  getActiveWorkspaceStore: () => {
    const { activeSessionId, workspaceStores } = get()
    if (!activeSessionId) return null
    return workspaceStores[activeSessionId] || null
  },

  createNewSession: async () => {
    const { sessionApi } = get()
    try {
      const result = await sessionApi.create([])
      if (!result.success || !result.session) {
        throw new Error(result.error || 'Failed to create session')
      }
      await sessionApi.openInNewWindow(result.session.id)
      set({ showWorkspacePicker: false })
    } catch (error) {
      alert(`Failed to create session: ${error instanceof Error ? error.message : 'Unknown error'}`)
    }
  },

  handleSessionRestore: async (daemonSession: Session) => {
    console.log('[App] Restoring session', daemonSession.id, 'with', daemonSession.workspaces.length, 'workspaces')

    const store = getOrCreateSessionStore(daemonSession.id, get, set)
    set({ activeSessionId: daemonSession.id })

    store.setState({ isRestoring: true })
    applySessionWorkspaces(store, daemonSession.workspaces, { restoreExisting: true })
    store.setState({ isRestoring: false })

    console.log('[App] Session restore complete, workspace count:', Object.keys(store.getState().workspaces).length)
    set({ showWorkspacePicker: false })
  },

  handleExternalSessionUpdate: async (daemonSession: Session) => {
    console.log('[App] External session update received', {
      sessionId: daemonSession.id,
      workspaces: daemonSession.workspaces.map(ws => ({ path: ws.path, metadata: ws.metadata })),
    })
    const store = getOrCreateSessionStore(daemonSession.id, get, set)

    store.setState({ isRestoring: true })
    applySessionWorkspaces(store, daemonSession.workspaces, { restoreExisting: false })

    // Remove workspaces not present in daemon session.
    // This is an update from the daemon, likely due to a merge/abandon from another window.
    // Only clean up local state — the originating window already handled git operations.
    const incomingPaths = new Set(daemonSession.workspaces.map(ws => ws.path))
    const { removeOrphanWorkspace } = store.getState()
    const updatedState = store.getState()
    for (const [id, ws] of Object.entries(updatedState.workspaces)) {
      if (!incomingPaths.has(ws.path)) {
        removeOrphanWorkspace(id)
      }
    }

    store.setState({ isRestoring: false })
    console.log('[App] External session update applied')
  }
}))

// Helper: get or create a workspace store for a session
function getOrCreateSessionStore(
  sessionId: string,
  get: () => AppState,
  set: (partial: Partial<AppState> | ((state: AppState) => Partial<AppState>)) => void
): StoreApi<WorkspaceState> {
  const { workspaceStores, windowUuid, git, sessionApi } = get()
  let store = workspaceStores[sessionId]
  if (!store) {
    store = createWorkspaceStore(
      { sessionId, windowUuid },
      {
        git,
        session: sessionApi,
        getSettings: () => useSettingsStore.getState().settings,
        appRegistry: applicationRegistry,
      }
    )
    set((state) => ({
      workspaceStores: { ...state.workspaceStores, [sessionId]: store! }
    }))
  }
  return store
}

// Helper: sync metadata and name from daemon workspace to existing workspace
function updateWorkspaceFields(
  store: StoreApi<WorkspaceState>,
  existingId: string,
  daemonWorkspace: Workspace
): void {
  store.setState((state) => {
    const existing = state.workspaces[existingId]
    if (!existing) return state
    return {
      workspaces: {
        ...state.workspaces,
        [existingId]: {
          ...existing,
          metadata: daemonWorkspace.metadata,
          name: daemonWorkspace.name,
        }
      }
    }
  })
}

// Helper: apply daemon workspaces to a store
// restoreExisting: true = restore tabs on existing workspaces (session restore)
// restoreExisting: false = skip existing workspaces (external sync)
function applySessionWorkspaces(
  store: StoreApi<WorkspaceState>,
  daemonWorkspaces: Workspace[],
  options: { restoreExisting: boolean }
): void {
  const { addTabWithState, setActiveTab, setActiveWorkspace } = store.getState()

  const rootWorkspaces = daemonWorkspaces.filter(w => !w.parentId)
  const childWorkspaces = daemonWorkspaces.filter(w => w.parentId)

  for (const daemonWorkspace of rootWorkspaces) {
    const existing = Object.values(store.getState().workspaces).find(
      ws => ws.path === daemonWorkspace.path
    )

    if (existing) {
      if (options.restoreExisting) {
        setActiveWorkspace(existing.id)
        restoreWorkspaceTabs(existing.id, daemonWorkspace, addTabWithState, setActiveTab)
      } else {
        updateWorkspaceFields(store, existing.id, daemonWorkspace)
      }
    } else {
      reconstructWorkspace(store, daemonWorkspace)
    }
  }

  for (const daemonWorkspace of childWorkspaces) {
    const existing = Object.values(store.getState().workspaces).find(
      ws => ws.path === daemonWorkspace.path
    )

    if (existing) {
      if (options.restoreExisting) {
        restoreWorkspaceTabs(existing.id, daemonWorkspace, addTabWithState, setActiveTab)
      } else {
        updateWorkspaceFields(store, existing.id, daemonWorkspace)
      }
    } else {
      reconstructWorkspace(store, daemonWorkspace)
    }
  }
}

// Helper: restore workspace tabs — preserves ptyId as-is without validation
function restoreWorkspaceTabs(
  workspaceId: string,
  daemonWorkspace: Workspace,
  addTabWithState: AddTabWithStateFn,
  setActiveTab: SetActiveTabFn
): void {
  for (const daemonTab of daemonWorkspace.tabs) {
    addTabWithState(workspaceId, daemonTab.applicationId, daemonTab.state as Record<string, unknown>, daemonTab.id)
  }

  if (daemonWorkspace.activeTabId) {
    setActiveTab(workspaceId, daemonWorkspace.activeTabId)
  }
}

// Helper: reconstruct workspace preserving daemon IDs
// Uses daemonWorkspace.parentId directly — no parent validation (lazy validation)
function reconstructWorkspace(
  store: StoreApi<WorkspaceState>,
  daemonWorkspace: Workspace
): string {
  const id = daemonWorkspace.id
  const parentId = daemonWorkspace.parentId

  const workspace: Workspace = {
    ...daemonWorkspace,
    id,
    children: [],
    activeTabId: daemonWorkspace.activeTabId || (daemonWorkspace.tabs.length > 0 ? daemonWorkspace.tabs[0].id : null)
  }

  store.setState((state) => {
    const newWorkspaces = {
      ...state.workspaces,
      [id]: workspace,
    }

    // Update parent's children array only if parent exists in state
    if (parentId && state.workspaces[parentId]) {
      newWorkspaces[parentId] = {
        ...state.workspaces[parentId],
        children: [...state.workspaces[parentId].children, id]
      }
    }

    return { workspaces: newWorkspaces, activeWorkspaceId: id }
  })

  console.log('[App] Reconstructed workspace:', daemonWorkspace.name, 'parentId:', parentId)
  return id
}
