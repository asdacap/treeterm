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
  FilesystemApi, ReviewsApi, STTApi, SandboxApi, SettingsApi
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
  reviews: ReviewsApi
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
  reviews: UNINITIALIZED,
  stt: UNINITIALIZED,
  sandbox: UNINITIALIZED,
  selectFolder: UNINITIALIZED,
  getRecentDirectories: UNINITIALIZED,
  getWindowUuid: UNINITIALIZED,
  getInitialWorkspace: UNINITIALIZED,

  windowUuid: null,
  daemonDisconnected: false,
  isSettingsOpen: false,
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
          const { workspaceStores, windowUuid } = get()
          if (!workspaceStores[session.id]) {
            const store = createWorkspaceStore(
              { sessionId: session.id, windowUuid },
              {
                git,
                session: sessionApi,
                getSettings: () => useSettingsStore.getState().settings,
                appRegistry: applicationRegistry,
              }
            )
            set((state) => ({
              workspaceStores: { ...state.workspaceStores, [session.id]: store },
              activeSessionId: session.id
            }))
          }
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

    const unsubShowSessions = sessionApi.onShowSessions(async () => {
      try {
        const result = await sessionApi.list()
        if (result.success && result.sessions && result.sessions.length > 0) {
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

  handleSessionRestore: async (daemonSession: Session) => {
    console.log('[App] Restoring session', daemonSession.id, 'with', daemonSession.workspaces.length, 'workspaces')

    const { workspaceStores, windowUuid, git, sessionApi } = get()

    // Create workspace store for this session if it doesn't exist
    let store = workspaceStores[daemonSession.id]
    if (!store) {
      store = createWorkspaceStore(
        { sessionId: daemonSession.id, windowUuid },
        {
          git,
          session: sessionApi,
          getSettings: () => useSettingsStore.getState().settings,
          appRegistry: applicationRegistry,
        }
      )
      set((state) => ({
        workspaceStores: { ...state.workspaceStores, [daemonSession.id]: store! },
        activeSessionId: daemonSession.id
      }))
    } else {
      set({ activeSessionId: daemonSession.id })
    }

    // Set isRestoring to prevent intermediate syncs during restoration
    store.setState({ isRestoring: true })
    console.log('[App] Set isRestoring to true for session', daemonSession.id)

    const { workspaces, addWorkspace, addTabWithState, setActiveWorkspace, setActiveTab } = store.getState()

    const rootWorkspaces = daemonSession.workspaces.filter(w => !w.parentId)
    const childWorkspaces = daemonSession.workspaces.filter(w => w.parentId)

    console.log('[App] Restoring', rootWorkspaces.length, 'root workspaces and', childWorkspaces.length, 'child workspaces')

    for (const daemonWorkspace of rootWorkspaces) {
      const existingWorkspace = Object.values(workspaces).find(
        (ws) => ws.path === daemonWorkspace.path
      )

      let workspaceId: string | null = null
      if (existingWorkspace) {
        workspaceId = existingWorkspace.id
        setActiveWorkspace(workspaceId)
      } else {
        workspaceId = await addWorkspace(daemonWorkspace.path, { skipDefaultTabs: true })
      }

      if (workspaceId) {
        restoreWorkspaceTabs(workspaceId, daemonWorkspace, addTabWithState, setActiveTab)
      }
    }

    const currentStoreState = store.getState()
    for (const daemonWorkspace of childWorkspaces) {
      const existingWorkspace = Object.values(currentStoreState.workspaces).find(
        (ws) => ws.path === daemonWorkspace.path
      )

      if (existingWorkspace) {
        restoreWorkspaceTabs(existingWorkspace.id, daemonWorkspace, addTabWithState, setActiveTab)
      } else {
        reconstructChildWorkspace(store, daemonWorkspace, addTabWithState, setActiveTab)
      }
    }

    store.setState({ isRestoring: false })
    console.log('[App] Set isRestoring to false, performing final sync')

    const finalState = store.getState()
    console.log('[App] Session restore complete, final workspace count:', Object.keys(finalState.workspaces).length)

    await finalState.syncToDaemon()
    console.log('[App] Final sync complete')

    set({ showWorkspacePicker: false })
  },

  handleExternalSessionUpdate: async (daemonSession: Session) => {
    const { workspaceStores, windowUuid, git, sessionApi } = get()

    let store = workspaceStores[daemonSession.id]
    if (!store) {
      store = createWorkspaceStore(
        { sessionId: daemonSession.id, windowUuid },
        {
          git,
          session: sessionApi,
          getSettings: () => useSettingsStore.getState().settings,
          appRegistry: applicationRegistry,
        }
      )
      set((state) => ({
        workspaceStores: { ...state.workspaceStores, [daemonSession.id]: store! }
      }))
    }

    store.setState({ isRestoring: true })

    const currentState = store.getState()
    const incomingPaths = new Set(daemonSession.workspaces.map(ws => ws.path))

    const rootWorkspaces = daemonSession.workspaces.filter(w => !w.parentId)
    const childWorkspaces = daemonSession.workspaces.filter(w => w.parentId)

    const { addWorkspace, addTabWithState, setActiveTab } = currentState

    for (const daemonWorkspace of rootWorkspaces) {
      const existing = Object.values(currentState.workspaces).find(ws => ws.path === daemonWorkspace.path)
      if (!existing) {
        const workspaceId = await addWorkspace(daemonWorkspace.path, { skipDefaultTabs: true })
        if (workspaceId) {
          restoreWorkspaceTabs(workspaceId, daemonWorkspace, addTabWithState, setActiveTab)
        }
      }
    }

    for (const daemonWorkspace of childWorkspaces) {
      const existing = Object.values(store.getState().workspaces).find(ws => ws.path === daemonWorkspace.path)
      if (!existing) {
        reconstructChildWorkspace(store, daemonWorkspace, addTabWithState, setActiveTab)
      }
    }

    const updatedState = store.getState()
    for (const [id, ws] of Object.entries(updatedState.workspaces)) {
      if (!incomingPaths.has(ws.path)) {
        store.setState((state) => {
          const newWorkspaces = { ...state.workspaces }
          delete newWorkspaces[id]
          if (ws.parentId && newWorkspaces[ws.parentId]) {
            newWorkspaces[ws.parentId] = {
              ...newWorkspaces[ws.parentId],
              children: newWorkspaces[ws.parentId].children.filter(c => c !== id)
            }
          }
          return { workspaces: newWorkspaces }
        })
      }
    }

    store.setState({ isRestoring: false })
    console.log('[App] External session update applied')
  }
}))

// Helper function to restore workspace tabs — preserves ptyId as-is without validation
function restoreWorkspaceTabs(
  workspaceId: string,
  daemonWorkspace: Workspace,
  addTabWithState: AddTabWithStateFn,
  setActiveTab: SetActiveTabFn
) {
  for (const daemonTab of daemonWorkspace.tabs) {
    addTabWithState(workspaceId, daemonTab.applicationId, daemonTab.state as Record<string, unknown>, daemonTab.id)
  }

  if (daemonWorkspace.activeTabId) {
    setActiveTab(workspaceId, daemonWorkspace.activeTabId)
  }
}

// Helper function to reconstruct child workspace with parent link
// Uses daemonWorkspace.parentId directly — no parent validation (lazy validation)
function reconstructChildWorkspace(
  store: StoreApi<WorkspaceState>,
  daemonWorkspace: Workspace,
  addTabWithState: AddTabWithStateFn,
  setActiveTab: SetActiveTabFn
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

  console.log('[App] Reconstructed child workspace:', daemonWorkspace.name, 'parentId:', parentId)
  return id
}
