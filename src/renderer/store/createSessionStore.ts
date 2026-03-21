import { createStore } from 'zustand/vanilla'
import type { StoreApi } from 'zustand'
import { createWorkspaceStore } from './createWorkspaceStore'
import type { WorkspaceState, WorkspaceHandle } from './createWorkspaceStore'
import type {
  Workspace, Session,
  SSHConnectionConfig, ConnectionInfo,
  TerminalApi, GitApi, SessionApi, SSHApi, Settings, WorktreeSettings
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

export interface SessionState {
  sessionId: string
  workspaceStore: StoreApi<WorkspaceState>

  // Single SSH connection for this session
  connection: ConnectionInfo | null
  connect: (config: SSHConnectionConfig) => Promise<void>
  disconnect: () => Promise<void>

  // Workspace collection management (delegates to workspace store)
  workspaces: Record<string, Workspace>
  activeWorkspaceId: string | null
  getWorkspace: (id: string) => WorkspaceHandle | null
  addWorkspace: (path: string, options?: { skipDefaultTabs?: boolean; settings?: WorktreeSettings }) => Promise<string>
  addChildWorkspace: (parentId: string, name: string, isDetached?: boolean, settings?: WorktreeSettings, description?: string) => Promise<{ success: boolean; error?: string }>
  adoptExistingWorktree: (parentId: string, worktreePath: string, branch: string, name: string, settings?: WorktreeSettings, description?: string) => Promise<{ success: boolean; error?: string }>
  createWorktreeFromBranch: (parentId: string, branch: string, isDetached: boolean, settings?: WorktreeSettings, description?: string) => Promise<{ success: boolean; error?: string }>
  createWorktreeFromRemote: (parentId: string, remoteBranch: string, isDetached: boolean, settings?: WorktreeSettings, description?: string) => Promise<{ success: boolean; error?: string }>
  removeWorkspace: (id: string) => Promise<void>
  removeWorkspaceKeepBranch: (id: string) => Promise<void>
  removeWorkspaceKeepWorktree: (id: string) => Promise<void>
  removeWorkspaceKeepBoth: (id: string) => Promise<void>
  removeOrphanWorkspace: (id: string) => void
  mergeAndRemoveWorkspace: (id: string, squash: boolean) => Promise<{ success: boolean; error?: string }>
  closeAndCleanWorkspace: (id: string) => Promise<{ success: boolean; error?: string }>
  setActiveWorkspace: (id: string | null) => void
  quickForkWorkspace: (workspaceId: string) => Promise<{ success: boolean; error?: string }>

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

  // Keep session store's workspace snapshot in sync with workspace store
  workspaceStore.subscribe((wsState) => {
    store.setState({
      workspaces: wsState.workspaces,
      activeWorkspaceId: wsState.activeWorkspaceId,
    })
  })

  const store = createStore<SessionState>()((set, get) => ({
    sessionId: config.sessionId,
    workspaceStore,

    // Workspace collection — synced from workspace store
    workspaces: workspaceStore.getState().workspaces,
    activeWorkspaceId: workspaceStore.getState().activeWorkspaceId,

    // Delegates to workspace store
    getWorkspace: (id: string) => workspaceStore.getState().getWorkspace(id),
    addWorkspace: (path, options?) => workspaceStore.getState().addWorkspace(path, options),
    addChildWorkspace: (parentId, name, isDetached?, settings?, description?) =>
      workspaceStore.getState().addChildWorkspace(parentId, name, isDetached, settings, description),
    adoptExistingWorktree: (parentId, worktreePath, branch, name, settings?, description?) =>
      workspaceStore.getState().adoptExistingWorktree(parentId, worktreePath, branch, name, settings, description),
    createWorktreeFromBranch: (parentId, branch, isDetached, settings?, description?) =>
      workspaceStore.getState().createWorktreeFromBranch(parentId, branch, isDetached, settings, description),
    createWorktreeFromRemote: (parentId, remoteBranch, isDetached, settings?, description?) =>
      workspaceStore.getState().createWorktreeFromRemote(parentId, remoteBranch, isDetached, settings, description),
    removeWorkspace: (id) => workspaceStore.getState().removeWorkspace(id),
    removeWorkspaceKeepBranch: (id) => workspaceStore.getState().removeWorkspaceKeepBranch(id),
    removeWorkspaceKeepWorktree: (id) => workspaceStore.getState().removeWorkspaceKeepWorktree(id),
    removeWorkspaceKeepBoth: (id) => workspaceStore.getState().removeWorkspaceKeepBoth(id),
    removeOrphanWorkspace: (id) => workspaceStore.getState().removeOrphanWorkspace(id),
    mergeAndRemoveWorkspace: (id, squash) => workspaceStore.getState().mergeAndRemoveWorkspace(id, squash),
    closeAndCleanWorkspace: (id) => workspaceStore.getState().closeAndCleanWorkspace(id),
    setActiveWorkspace: (id) => workspaceStore.getState().setActiveWorkspace(id),
    quickForkWorkspace: (workspaceId) => workspaceStore.getState().quickForkWorkspace(workspaceId),

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
  const { setActiveWorkspace } = store.getState()

  const rootWorkspaces = daemonWorkspaces.filter(w => !w.parentId)
  const childWorkspaces = daemonWorkspaces.filter(w => w.parentId)

  for (const daemonWorkspace of rootWorkspaces) {
    const existing = Object.values(store.getState().workspaces).find(
      ws => ws.path === daemonWorkspace.path
    )

    if (existing) {
      if (options.restoreExisting) {
        setActiveWorkspace(existing.id)
        restoreWorkspaceTabs(store, existing.id, daemonWorkspace)
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
        restoreWorkspaceTabs(store, existing.id, daemonWorkspace)
      } else {
        updateWorkspaceFields(store, existing.id, daemonWorkspace)
      }
    } else {
      reconstructWorkspace(store, daemonWorkspace)
    }
  }
}

// Helper: restore workspace tabs by setting appStates directly
function restoreWorkspaceTabs(
  store: StoreApi<WorkspaceState>,
  workspaceId: string,
  daemonWorkspace: Workspace
): void {
  store.setState((state) => {
    const ws = state.workspaces[workspaceId]
    if (!ws) return state
    return {
      workspaces: {
        ...state.workspaces,
        [workspaceId]: {
          ...ws,
          appStates: daemonWorkspace.appStates,
          activeTabId: daemonWorkspace.activeTabId || Object.keys(daemonWorkspace.appStates)[0] || null
        }
      }
    }
  })
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
