import { createStore } from 'zustand/vanilla'
import type { StoreApi } from 'zustand'
import { humanId } from 'human-id'
import { createWorkspaceStore } from './createWorkspaceStore'
import type { WorkspaceStore, WorkspaceStoreDeps } from './createWorkspaceStore'
import { createTtyStore } from './createTtyStore'
import type { Tty, TtyTerminalDeps } from './createTtyStore'
import type {
  Workspace, Session, AppState, GitInfo,
  ConnectionInfo,
  TerminalApi, GitApi, FilesystemApi, SessionApi, Settings, WorktreeSettings,
  Application, SandboxConfig, SessionInfo
} from '../types'

export interface AppRegistryApi {
  get: (id: string) => Application | undefined
  getDefaultApp: (appId?: string) => Application | null
}

export interface SessionDeps {
  git: GitApi
  filesystem: FilesystemApi
  sessionApi: SessionApi
  terminal: TerminalApi
  getSettings: () => Settings
  appRegistry: AppRegistryApi
}

export interface SessionState {
  sessionId: string

  // Single SSH connection for this session (set at creation, immutable)
  connection: ConnectionInfo | null

  // TTY sub-stores (enclosed ID pattern)
  ttyHandles: Record<string, Tty>
  createTty: (cwd: string, sandbox?: SandboxConfig, startupCommand?: string) => Promise<string>
  attachTty: (ptyId: string) => Promise<{ scrollback?: string[]; exitCode?: number }>
  getTty: (ptyId: string) => Tty | null
  listTty: () => Promise<SessionInfo[]>

  // Workspace collection
  workspaceStores: Record<string, WorkspaceStore>
  workspaces: Record<string, Workspace>
  activeWorkspaceId: string | null
  isRestoring: boolean

  getWorkspace: (id: string) => WorkspaceStore | null
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
  updateGitInfo: (id: string, gitInfo: GitInfo) => void
  refreshGitInfo: (id: string) => Promise<void>
  quickForkWorkspace: (workspaceId: string) => Promise<{ success: boolean; error?: string }>
  syncToDaemon: () => Promise<void>

  // Session lifecycle
  handleRestore: (session: Session) => Promise<void>
  handleExternalUpdate: (session: Session) => Promise<void>
}

function generateId(): string {
  return `ws-${Date.now()}-${Math.random().toString(36).slice(2, 9)}`
}

function generateTabId(): string {
  return `tab-${Date.now()}-${Math.random().toString(36).slice(2, 9)}`
}

function getNameFromPath(path: string): string {
  return path.split('/').pop() || path
}

function getDefaultAppForWorktree(
  deps: Pick<SessionDeps, 'appRegistry' | 'getSettings'>,
  settings?: WorktreeSettings,
  parentSettings?: WorktreeSettings
): Application | null | undefined {
  if (settings?.defaultApplicationId) {
    const app = deps.appRegistry.get(settings.defaultApplicationId)
    if (app) return app
  }
  if (parentSettings?.defaultApplicationId) {
    const app = deps.appRegistry.get(parentSettings.defaultApplicationId)
    if (app) return app
  }
  const globalSettings = deps.getSettings()
  if (globalSettings.globalDefaultApplicationId) {
    const app = deps.appRegistry.get(globalSettings.globalDefaultApplicationId)
    if (app) return app
  }
  return deps.appRegistry.getDefaultApp()
}

/**
 * Helper function to find unmerged sub-workspaces (worktrees with status 'active')
 */
export function getUnmergedSubWorkspaces(workspaces: Record<string, Workspace>): Workspace[] {
  return Object.values(workspaces).filter(
    (ws) => ws.isWorktree && ws.status === 'active'
  )
}

export function createSessionStore(
  config: { sessionId: string; windowUuid: string | null; connection?: ConnectionInfo },
  deps: SessionDeps
): StoreApi<SessionState> {
  let syncDebounceTimer: ReturnType<typeof setTimeout> | null = null

  async function syncSessionToDaemon(isRestoring: boolean = false): Promise<void> {
    try {
      const settings = deps.getSettings()
      const workspaces = store.getState().workspaces
      console.log('[session] syncSessionToDaemon called - workspaces:', Object.keys(workspaces).length, 'isRestoring:', isRestoring)

      if (isRestoring) {
        console.log('[session] currently restoring, skipping sync')
        return
      }

      const daemonWorkspaces = Object.values(workspaces).map(({ createdAt, lastActivity, ...ws }) => ws)

      console.log('[session] syncing to daemon:', daemonWorkspaces.length, 'workspaces')

      if (daemonWorkspaces.length === 0) {
        if (config.sessionId) {
          console.log('[session] deleting session:', config.sessionId)
          await deps.sessionApi.delete(config.sessionId)
        }
        return
      }

      console.log('[session] updating session:', config.sessionId, 'senderUuid:', config.windowUuid)
      const result = await deps.sessionApi.update(config.sessionId, daemonWorkspaces, config.windowUuid || undefined)
      if (!result.success) {
        console.error('[session] failed to update session:', result.error)
      } else {
        console.log('[session] session updated successfully')
      }
    } catch (error) {
      console.error('[session] failed to sync session to daemon:', error)
    }
  }

  function debouncedSyncToDaemon(): void {
    if (syncDebounceTimer) {
      clearTimeout(syncDebounceTimer)
    }
    syncDebounceTimer = setTimeout(() => {
      syncSessionToDaemon(store.getState().isRestoring)
    }, 500)
  }

  function makeHandleDeps(): WorkspaceStoreDeps {
    return {
      appRegistry: deps.appRegistry,
      getTty: (ptyId: string) => store.getState().getTty(ptyId),
      git: deps.git,
      filesystem: deps.filesystem,
      syncToDaemon: () => debouncedSyncToDaemon(),
      removeWorkspace: (id) => store.getState().removeWorkspace(id),
      removeWorkspaceKeepBranch: (id) => store.getState().removeWorkspaceKeepBranch(id),
      removeWorkspaceKeepWorktree: (id) => store.getState().removeWorkspaceKeepWorktree(id),
      removeWorkspaceKeepBoth: (id) => store.getState().removeWorkspaceKeepBoth(id),
      mergeAndRemoveWorkspace: (id, squash) => store.getState().mergeAndRemoveWorkspace(id, squash),
      closeAndCleanWorkspace: (id) => store.getState().closeAndCleanWorkspace(id),
      quickForkWorkspace: (id) => store.getState().quickForkWorkspace(id),
      refreshGitInfo: (id) => store.getState().refreshGitInfo(id),
      lookupWorkspace: (id) => store.getState().workspaces[id],
    }
  }

  function createHandleForWorkspace(workspace: Workspace): WorkspaceStore {
    const handle = createWorkspaceStore(workspace, makeHandleDeps())

    // Keep the workspaces snapshot in sync when handle state changes
    handle.subscribe((state) => {
      store.setState((s) => ({
        workspaces: { ...s.workspaces, [state.workspace.id]: state.workspace }
      }))
    })

    return handle
  }

  // Shared helper: creates a child workspace from a git operation result
  async function addChildWorkspaceFromResult(
    parentId: string,
    name: string,
    path: string,
    branch: string,
    options: { isDetached?: boolean; isWorktree?: boolean; settings?: WorktreeSettings; metadata?: Record<string, string> } = {}
  ): Promise<string> {
    const state = store.getState()
    const parent = state.workspaces[parentId]

    const id = generateId()
    const appStates: Record<string, AppState> = {}
    let activeTabId: string | null = null

    const defaultApp = getDefaultAppForWorktree(deps, options.settings, parent?.settings)
    if (defaultApp) {
      const tabId = generateTabId()
      appStates[tabId] = {
        applicationId: defaultApp.id,
        title: defaultApp.name,
        state: defaultApp.createInitialState()
      }
      activeTabId = tabId
    }

    const childWorkspace: Workspace = {
      id,
      name,
      path,
      parentId,
      children: [],
      status: 'active',
      isGitRepo: true,
      gitBranch: branch,
      gitRootPath: parent?.gitRootPath ?? null,
      isWorktree: options.isWorktree ?? true,
      isDetached: options.isDetached,
      appStates,
      activeTabId,
      settings: options.settings,
      metadata: options.metadata ?? {},
      createdAt: Date.now(),
      lastActivity: Date.now(),
    }

    const handle = createHandleForWorkspace(childWorkspace)

    // Update parent children and add handle
    store.setState((s) => {
      const parentWs = s.workspaces[parentId]
      const updatedWorkspaces = {
        ...s.workspaces,
        [id]: childWorkspace,
        ...(parentWs ? {
          [parentId]: { ...parentWs, children: [...parentWs.children, id] }
        } : {})
      }
      // Also update the parent handle's workspace data
      const parentHandle = s.workspaceStores[parentId]
      if (parentHandle && parentWs) {
        parentHandle.setState({ workspace: { ...parentWs, children: [...parentWs.children, id] } })
      }
      return {
        workspaceStores: { ...s.workspaceStores, [id]: handle },
        workspaces: updatedWorkspaces,
        activeWorkspaceId: id
      }
    })

    await syncSessionToDaemon(store.getState().isRestoring)
    return id
  }

  // Shared helper: removes a workspace with configurable git cleanup behavior
  async function removeWorkspaceInternal(
    id: string,
    options: { keepBranch: boolean; keepWorktree: boolean }
  ): Promise<void> {
    const state = store.getState()
    const workspace = state.workspaces[id]
    if (!workspace) return

    // Recursively remove children first
    for (const childId of workspace.children) {
      await removeWorkspaceInternal(childId, options)
    }

    // Cleanup tabs
    for (const [tabId, appState] of Object.entries(workspace.appStates)) {
      const tab = { ...appState, id: tabId }
      const app = deps.appRegistry.get(tab.applicationId)
      if (app?.cleanup) {
        await app.cleanup(tab, workspace)
      }
    }

    // Git cleanup
    if (workspace.isWorktree && workspace.gitRootPath) {
      if (!options.keepWorktree) {
        const deleteBranch = !options.keepBranch && !workspace.isDetached
        await deps.git.removeWorktree(
          workspace.gitRootPath,
          workspace.path,
          deleteBranch
        )
      } else if (!options.keepBranch && !workspace.isDetached && workspace.gitBranch) {
        await deps.git.deleteBranch(workspace.gitRootPath, workspace.gitBranch)
      }
    }

    // Update parent's children list
    if (workspace.parentId) {
      const parentHandle = store.getState().workspaceStores[workspace.parentId]
      if (parentHandle) {
        const parentWs = parentHandle.getState().workspace
        parentHandle.setState({
          workspace: { ...parentWs, children: parentWs.children.filter((cid) => cid !== id) }
        })
      }
    }

    // Remove handle and workspace
    store.setState((s) => {
      const { [id]: _removedHandle, ...remainingHandles } = s.workspaceStores
      const { [id]: _removedWs, ...remainingWorkspaces } = s.workspaces
      return {
        workspaceStores: remainingHandles,
        workspaces: remainingWorkspaces,
        activeWorkspaceId: s.activeWorkspaceId === id ? null : s.activeWorkspaceId
      }
    })

    await syncSessionToDaemon(store.getState().isRestoring)
  }

  const connectionId = config.connection?.id ?? 'local'

  // Create a terminal wrapper with connectionId bound for tty stores
  const boundTerminal: TtyTerminalDeps = {
    write: deps.terminal.write,
    resize: deps.terminal.resize,
    kill: (sessionId: string) => deps.terminal.kill(connectionId, sessionId),
    isAlive: (id: string) => deps.terminal.isAlive(connectionId, id),
    onData: deps.terminal.onData,
    onExit: deps.terminal.onExit,
  }

  const store = createStore<SessionState>()((set, get) => ({
    sessionId: config.sessionId,
    ttyHandles: {},
    workspaceStores: {},
    workspaces: {},
    activeWorkspaceId: null,
    isRestoring: false,

    connection: config.connection ?? null,

    createTty: async (cwd: string, sandbox?: SandboxConfig, startupCommand?: string): Promise<string> => {
      const result = await deps.terminal.create(connectionId, cwd, sandbox, startupCommand)
      if (!result) {
        throw new Error('Failed to create PTY')
      }
      const tty = createTtyStore(result.sessionId, result.handle, boundTerminal)
      set((s) => ({
        ttyHandles: { ...s.ttyHandles, [result.sessionId]: tty }
      }))
      return result.sessionId
    },

    attachTty: async (ptyId: string): Promise<{ scrollback?: string[]; exitCode?: number }> => {
      const result = await deps.terminal.attach(connectionId, ptyId)
      if (!result.success || !result.handle) {
        throw new Error(result.error || 'Failed to attach to PTY')
      }
      const tty = createTtyStore(ptyId, result.handle, boundTerminal)
      set((s) => ({
        ttyHandles: { ...s.ttyHandles, [ptyId]: tty }
      }))
      return { scrollback: result.scrollback, exitCode: result.exitCode }
    },

    getTty: (ptyId: string): Tty | null => {
      return get().ttyHandles[ptyId] ?? null
    },

    listTty: (): Promise<SessionInfo[]> => {
      return deps.terminal.list(connectionId)
    },

    getWorkspace: (id: string): WorkspaceStore | null => {
      return get().workspaceStores[id] ?? null
    },

    addWorkspace: async (path: string, options?: { skipDefaultTabs?: boolean; settings?: WorktreeSettings }) => {
      console.log('[session] addWorkspace called for path:', path)
      const id = generateId()

      const gitInfo = await deps.git.getInfo(path)

      const appStates: Record<string, AppState> = {}
      let activeTabId: string | null = null

      if (!options?.skipDefaultTabs) {
        const defaultApp = getDefaultAppForWorktree(deps, options?.settings, undefined)
        if (defaultApp) {
          const tabId = generateTabId()
          appStates[tabId] = {
            applicationId: defaultApp.id,
            title: defaultApp.name,
            state: defaultApp.createInitialState()
          }
          activeTabId = tabId
        }
      }

      const workspace: Workspace = {
        id,
        name: getNameFromPath(path),
        path,
        parentId: null,
        children: [],
        status: 'active',
        isGitRepo: gitInfo.isRepo,
        gitBranch: gitInfo.branch,
        gitRootPath: gitInfo.rootPath,
        isWorktree: false,
        appStates,
        activeTabId,
        settings: options?.settings,
        metadata: {},
        createdAt: Date.now(),
        lastActivity: Date.now(),
      }

      const handle = createHandleForWorkspace(workspace)

      set((s) => ({
        workspaceStores: { ...s.workspaceStores, [id]: handle },
        workspaces: { ...s.workspaces, [id]: workspace },
        activeWorkspaceId: id
      }))

      await syncSessionToDaemon(get().isRestoring)
      return id
    },

    addChildWorkspace: async (parentId: string, name: string, isDetached: boolean = false, settings?: WorktreeSettings, description?: string) => {
      const state = get()
      const parent = state.workspaces[parentId]

      if (!parent) {
        return { success: false, error: 'Parent workspace not found' }
      }

      if (!parent.isGitRepo || !parent.gitRootPath) {
        return { success: false, error: 'Parent workspace is not a git repository' }
      }

      const currentGitInfo = await deps.git.getInfo(parent.path)
      const currentBranch = currentGitInfo.branch

      const result = await deps.git.createWorktree(
        parent.gitRootPath,
        name,
        currentBranch || undefined
      )

      if (!result.success) {
        return { success: false, error: result.error }
      }

      if (currentBranch && currentBranch !== parent.gitBranch) {
        get().updateGitInfo(parentId, currentGitInfo)
      }

      const metadata = description ? { description } : undefined
      await addChildWorkspaceFromResult(parentId, name, result.path!, result.branch!, { isDetached, settings, metadata })
      return { success: true }
    },

    adoptExistingWorktree: async (parentId: string, worktreePath: string, branch: string, name: string, settings?: WorktreeSettings, description?: string) => {
      const state = get()
      const parent = state.workspaces[parentId]

      if (!parent) {
        return { success: false, error: 'Parent workspace not found' }
      }

      const existingWorkspace = Object.values(state.workspaces).find(
        ws => ws.path === worktreePath
      )
      if (existingWorkspace) {
        return { success: false, error: 'This worktree is already open' }
      }

      const metadata = description ? { description } : undefined
      await addChildWorkspaceFromResult(parentId, name, worktreePath, branch, { settings, metadata })
      return { success: true }
    },

    createWorktreeFromBranch: async (parentId: string, branch: string, isDetached: boolean, settings?: WorktreeSettings, description?: string) => {
      console.log('[session] createWorktreeFromBranch called:', { parentId, branch, isDetached })
      const state = get()
      const parent = state.workspaces[parentId]

      if (!parent) {
        return { success: false, error: 'Parent workspace not found' }
      }

      if (!parent.isGitRepo || !parent.gitRootPath) {
        return { success: false, error: 'Parent workspace is not a git repository' }
      }

      const worktreeName = branch.split('/').pop() || branch
      const result = await deps.git.createWorktreeFromBranch(
        parent.gitRootPath,
        branch,
        worktreeName
      )

      if (!result.success) {
        return { success: false, error: result.error }
      }

      const metadata = description ? { description } : undefined
      await addChildWorkspaceFromResult(parentId, worktreeName, result.path!, result.branch!, { isDetached, settings, metadata })
      return { success: true }
    },

    createWorktreeFromRemote: async (parentId: string, remoteBranch: string, isDetached: boolean, settings?: WorktreeSettings, description?: string) => {
      console.log('[session] createWorktreeFromRemote called:', { parentId, remoteBranch, isDetached })
      const state = get()
      const parent = state.workspaces[parentId]

      if (!parent) {
        return { success: false, error: 'Parent workspace not found' }
      }

      if (!parent.isGitRepo || !parent.gitRootPath) {
        return { success: false, error: 'Parent workspace is not a git repository' }
      }

      const worktreeName = remoteBranch.split('/').pop() || remoteBranch
      const result = await deps.git.createWorktreeFromRemote(
        parent.gitRootPath,
        remoteBranch,
        worktreeName
      )

      if (!result.success) {
        return { success: false, error: result.error }
      }

      const metadata = description ? { description } : undefined
      await addChildWorkspaceFromResult(parentId, worktreeName, result.path!, result.branch!, { isDetached, settings, metadata })
      return { success: true }
    },

    removeWorkspace: async (id: string) => {
      await removeWorkspaceInternal(id, { keepBranch: false, keepWorktree: false })
    },

    removeWorkspaceKeepBranch: async (id: string) => {
      await removeWorkspaceInternal(id, { keepBranch: true, keepWorktree: false })
    },

    removeWorkspaceKeepWorktree: async (id: string) => {
      await removeWorkspaceInternal(id, { keepBranch: false, keepWorktree: true })
    },

    removeWorkspaceKeepBoth: async (id: string) => {
      await removeWorkspaceInternal(id, { keepBranch: true, keepWorktree: true })
    },

    removeOrphanWorkspace: (id: string) => {
      const state = get()
      const workspace = state.workspaces[id]
      if (!workspace) return

      if (workspace.parentId) {
        const parentHandle = state.workspaceStores[workspace.parentId]
        if (parentHandle) {
          const parentWs = parentHandle.getState().workspace
          parentHandle.setState({
            workspace: { ...parentWs, children: parentWs.children.filter((cid) => cid !== id) }
          })
        }
      }

      set((s) => {
        const { [id]: _removedHandle, ...remainingHandles } = s.workspaceStores
        const { [id]: _removedWs, ...remainingWorkspaces } = s.workspaces
        return {
          workspaceStores: remainingHandles,
          workspaces: remainingWorkspaces,
          activeWorkspaceId: s.activeWorkspaceId === id ? null : s.activeWorkspaceId
        }
      })
    },

    setActiveWorkspace: (id: string | null) => {
      set({ activeWorkspaceId: id })
    },

    updateGitInfo: (id: string, gitInfo: GitInfo) => {
      const handle = get().workspaceStores[id]
      if (!handle) return
      const ws = handle.getState().workspace
      handle.setState({
        workspace: {
          ...ws,
          isGitRepo: gitInfo.isRepo,
          gitBranch: gitInfo.branch,
          gitRootPath: gitInfo.rootPath
        }
      })
      syncSessionToDaemon(get().isRestoring).catch(console.error)
    },

    refreshGitInfo: async (id: string) => {
      const workspace = get().workspaces[id]
      if (!workspace) return
      const gitInfo = await deps.git.getInfo(workspace.path)
      get().updateGitInfo(id, gitInfo)
    },

    mergeAndRemoveWorkspace: async (id: string, squash: boolean) => {
      const state = get()
      const workspace = state.workspaces[id]

      if (!workspace) {
        return { success: false, error: 'Workspace not found' }
      }

      if (!workspace.isWorktree || !workspace.parentId) {
        return { success: false, error: 'Not a worktree workspace' }
      }

      const parent = state.workspaces[workspace.parentId]
      if (!parent || !parent.gitRootPath || !parent.gitBranch) {
        return { success: false, error: 'Parent workspace not found or not a git repo' }
      }

      const hasChanges = await deps.git.hasUncommittedChanges(workspace.path)
      if (hasChanges) {
        const commitResult = await deps.git.commitAll(
          workspace.path,
          `WIP: Auto-commit before merge from ${workspace.name}`
        )
        if (!commitResult.success) {
          return { success: false, error: `Failed to commit changes: ${commitResult.error}` }
        }
      }

      const mergeResult = await deps.git.merge(
        parent.gitRootPath,
        workspace.gitBranch!,
        parent.gitBranch,
        squash
      )

      if (!mergeResult.success) {
        return { success: false, error: `Merge failed: ${mergeResult.error}` }
      }

      // Update status on the handle
      const handle = get().workspaceStores[id]
      if (handle) {
        handle.getState().updateStatus('merged')
      }
      await get().removeWorkspace(id)

      return { success: true }
    },

    closeAndCleanWorkspace: async (id: string) => {
      const state = get()
      const workspace = state.workspaces[id]

      if (!workspace) {
        return { success: false, error: 'Workspace not found' }
      }

      if (!workspace.isWorktree || !workspace.parentId) {
        return { success: false, error: 'Not a worktree workspace' }
      }

      const parent = state.workspaces[workspace.parentId]
      if (!parent || !parent.gitRootPath) {
        return { success: false, error: 'Parent workspace not found or not a git repo' }
      }

      await get().removeWorkspace(id)
      return { success: true }
    },

    quickForkWorkspace: async (workspaceId: string) => {
      const state = get()
      const ws = state.workspaces[workspaceId]

      if (!ws) {
        return { success: false, error: 'Workspace not found' }
      }

      if (!ws.gitRootPath) {
        return { success: false, error: 'Workspace has no git root path' }
      }

      const existingBranches = await deps.git.listLocalBranches(ws.gitRootPath)
      const parentBranch = ws.gitBranch || ''

      let name: string | null = null
      for (let i = 0; i < 3; i++) {
        const candidate = humanId({ separator: '-', capitalize: false })
        const fullBranch = parentBranch ? `${parentBranch}/${candidate}` : candidate
        if (!existingBranches.includes(fullBranch)) {
          name = candidate
          break
        }
      }

      if (!name) {
        return { success: false, error: 'Failed to generate unique branch name' }
      }

      return get().addChildWorkspace(workspaceId, name, false)
    },

    syncToDaemon: async () => {
      await syncSessionToDaemon(get().isRestoring)
    },

    handleRestore: async (daemonSession: Session) => {
      console.log('[Session] Restoring session', daemonSession.id, 'with', daemonSession.workspaces.length, 'workspaces')

      set({ isRestoring: true })
      applySessionWorkspaces(store, daemonSession.workspaces, createHandleForWorkspace, { restoreExisting: true })
      set({ isRestoring: false })

      console.log('[Session] Session restore complete, workspace count:', Object.keys(get().workspaces).length)
    },

    handleExternalUpdate: async (daemonSession: Session) => {
      console.log('[Session] External session update received', {
        sessionId: daemonSession.id,
        workspaces: daemonSession.workspaces.map(ws => ({ path: ws.path, metadata: ws.metadata })),
      })

      set({ isRestoring: true })
      applySessionWorkspaces(store, daemonSession.workspaces, createHandleForWorkspace, { restoreExisting: false })

      // Remove workspaces not present in daemon session
      const incomingPaths = new Set(daemonSession.workspaces.map(ws => ws.path))
      const updatedState = get()
      for (const [id, ws] of Object.entries(updatedState.workspaces)) {
        if (!incomingPaths.has(ws.path)) {
          get().removeOrphanWorkspace(id)
        }
      }

      set({ isRestoring: false })
      console.log('[Session] External session update applied')
    }
  }))

  return store
}

// Helper: sync metadata and name from daemon workspace to existing workspace handle
function updateWorkspaceFields(
  store: StoreApi<SessionState>,
  existingId: string,
  daemonWorkspace: Workspace
): void {
  const handle = store.getState().workspaceStores[existingId]
  if (!handle) return
  const ws = handle.getState().workspace
  handle.setState({
    workspace: { ...ws, metadata: daemonWorkspace.metadata, name: daemonWorkspace.name }
  })
}

// Helper: apply daemon workspaces to the session store
function applySessionWorkspaces(
  store: StoreApi<SessionState>,
  daemonWorkspaces: Workspace[],
  createHandleForWorkspace: (ws: Workspace) => WorkspaceStore,
  options: { restoreExisting: boolean }
): void {
  const rootWorkspaces = daemonWorkspaces.filter(w => !w.parentId)
  const childWorkspaces = daemonWorkspaces.filter(w => w.parentId)

  for (const daemonWorkspace of rootWorkspaces) {
    const existing = Object.values(store.getState().workspaces).find(
      ws => ws.path === daemonWorkspace.path
    )

    if (existing) {
      if (options.restoreExisting) {
        store.getState().setActiveWorkspace(existing.id)
        restoreWorkspaceTabs(store, existing.id, daemonWorkspace)
      } else {
        updateWorkspaceFields(store, existing.id, daemonWorkspace)
      }
    } else {
      reconstructWorkspace(store, daemonWorkspace, createHandleForWorkspace)
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
      reconstructWorkspace(store, daemonWorkspace, createHandleForWorkspace)
    }
  }
}

// Helper: restore workspace tabs by updating the handle's workspace state
function restoreWorkspaceTabs(
  store: StoreApi<SessionState>,
  workspaceId: string,
  daemonWorkspace: Workspace
): void {
  const handle = store.getState().workspaceStores[workspaceId]
  if (!handle) return
  const ws = handle.getState().workspace
  handle.setState({
    workspace: {
      ...ws,
      appStates: daemonWorkspace.appStates,
      activeTabId: daemonWorkspace.activeTabId || Object.keys(daemonWorkspace.appStates)[0] || null
    }
  })
}

// Helper: reconstruct workspace preserving daemon IDs
function reconstructWorkspace(
  store: StoreApi<SessionState>,
  daemonWorkspace: Workspace,
  createHandleForWorkspace: (ws: Workspace) => WorkspaceStore
): string {
  const id = daemonWorkspace.id
  const parentId = daemonWorkspace.parentId

  const workspace: Workspace = {
    ...daemonWorkspace,
    id,
    children: [],
    activeTabId: daemonWorkspace.activeTabId || (Object.keys(daemonWorkspace.appStates).length > 0 ? Object.keys(daemonWorkspace.appStates)[0] : null)
  }

  const handle = createHandleForWorkspace(workspace)

  store.setState((s) => {
    const newWorkspaces = { ...s.workspaces, [id]: workspace }
    const newHandles = { ...s.workspaceStores, [id]: handle }

    if (parentId && s.workspaces[parentId]) {
      newWorkspaces[parentId] = {
        ...s.workspaces[parentId],
        children: [...s.workspaces[parentId].children, id]
      }
      // Update parent handle too
      const parentHandle = s.workspaceStores[parentId]
      if (parentHandle) {
        parentHandle.setState({ workspace: newWorkspaces[parentId] })
      }
    }

    return {
      workspaceStores: newHandles,
      workspaces: newWorkspaces,
      activeWorkspaceId: id
    }
  })

  console.log('[Session] Reconstructed workspace:', daemonWorkspace.name, 'parentId:', parentId)
  return id
}
