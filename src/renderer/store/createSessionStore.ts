import { createStore } from 'zustand/vanilla'
import type { StoreApi } from 'zustand'
import { createWorkspaceStore } from './createWorkspaceStore'
import type { WorkspaceState } from './createWorkspaceStore'
import type {
  Workspace, Session,
  SSHConnectionConfig, ConnectionInfo,
  TerminalApi, GitApi, SessionApi, SSHApi, Settings
} from '../types'

export interface AppRegistryApi {
  get: (id: string) => import('../types').Application | undefined
  getDefaultApp: (appId?: string) => import('../types').Application | null
}

export interface SessionDeps {
  ssh: SSHApi
  git: GitApi
  sessionApi: SessionApi
  terminal: TerminalApi
  getSettings: () => Settings
  appRegistry: AppRegistryApi
}

type AddTabWithStateFn = <T>(workspaceId: string, applicationId: string, initialState: Partial<T>, existingTabId?: string) => string
type SetActiveTabFn = (workspaceId: string, tabId: string) => void

export interface SessionState {
  sessionId: string
  workspaceStore: StoreApi<WorkspaceState>

  // Single SSH connection for this session
  connection: ConnectionInfo | null
  connect: (config: SSHConnectionConfig) => Promise<void>
  disconnect: () => Promise<void>

  // Session lifecycle
  handleRestore: (session: Session) => Promise<void>
  handleExternalUpdate: (session: Session) => Promise<void>
}

export function createSessionStore(
  config: { sessionId: string; windowUuid: string | null },
  deps: SessionDeps
): StoreApi<SessionState> {
  const workspaceStore = createWorkspaceStore(
    { sessionId: config.sessionId, windowUuid: config.windowUuid },
    {
      git: deps.git,
      session: deps.sessionApi,
      terminal: deps.terminal,
      getSettings: deps.getSettings,
      appRegistry: deps.appRegistry,
    }
  )

  const store = createStore<SessionState>()((set, get) => ({
    sessionId: config.sessionId,
    workspaceStore,

    connection: null,

    connect: async (sshConfig: SSHConnectionConfig) => {
      const { ssh } = deps
      try {
        const info = await ssh.connect(sshConfig)
        if (info.status === 'error') {
          throw new Error(info.error || 'Connection failed')
        }
        set({ connection: info })
      } catch (error) {
        console.error('[Session] SSH connect failed:', error)
        throw error
      }
    },

    disconnect: async () => {
      const { connection } = get()
      if (!connection) return
      const { ssh } = deps
      await ssh.disconnect(connection.id)
      set({ connection: null })
    },

    handleRestore: async (daemonSession: Session) => {
      console.log('[Session] Restoring session', daemonSession.id, 'with', daemonSession.workspaces.length, 'workspaces')

      workspaceStore.setState({ isRestoring: true })
      applySessionWorkspaces(workspaceStore, daemonSession.workspaces, { restoreExisting: true })
      workspaceStore.setState({ isRestoring: false })

      console.log('[Session] Session restore complete, workspace count:', Object.keys(workspaceStore.getState().workspaces).length)
    },

    handleExternalUpdate: async (daemonSession: Session) => {
      console.log('[Session] External session update received', {
        sessionId: daemonSession.id,
        workspaces: daemonSession.workspaces.map(ws => ({ path: ws.path, metadata: ws.metadata })),
      })

      workspaceStore.setState({ isRestoring: true })
      applySessionWorkspaces(workspaceStore, daemonSession.workspaces, { restoreExisting: false })

      // Remove workspaces not present in daemon session.
      const incomingPaths = new Set(daemonSession.workspaces.map(ws => ws.path))
      const { removeOrphanWorkspace } = workspaceStore.getState()
      const updatedState = workspaceStore.getState()
      for (const [id, ws] of Object.entries(updatedState.workspaces)) {
        if (!incomingPaths.has(ws.path)) {
          removeOrphanWorkspace(id)
        }
      }

      workspaceStore.setState({ isRestoring: false })
      console.log('[Session] External session update applied')
    }
  }))

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

// Helper: restore workspace tabs
function restoreWorkspaceTabs(
  workspaceId: string,
  daemonWorkspace: Workspace,
  addTabWithState: AddTabWithStateFn,
  setActiveTab: SetActiveTabFn
): void {
  for (const [tabId, appState] of Object.entries(daemonWorkspace.appStates)) {
    addTabWithState(workspaceId, appState.applicationId, appState.state as Record<string, unknown>, tabId)
  }

  if (daemonWorkspace.activeTabId) {
    setActiveTab(workspaceId, daemonWorkspace.activeTabId)
  }
}

// Helper: reconstruct workspace preserving daemon IDs
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
    activeTabId: daemonWorkspace.activeTabId || (Object.keys(daemonWorkspace.appStates).length > 0 ? Object.keys(daemonWorkspace.appStates)[0] : null)
  }

  store.setState((state) => {
    const newWorkspaces = {
      ...state.workspaces,
      [id]: workspace,
    }

    if (parentId && state.workspaces[parentId]) {
      newWorkspaces[parentId] = {
        ...state.workspaces[parentId],
        children: [...state.workspaces[parentId].children, id]
      }
    }

    return { workspaces: newWorkspaces, activeWorkspaceId: id }
  })

  console.log('[Session] Reconstructed workspace:', daemonWorkspace.name, 'parentId:', parentId)
  return id
}
