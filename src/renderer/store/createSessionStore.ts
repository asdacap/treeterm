import { createStore } from 'zustand/vanilla'
import type { StoreApi } from 'zustand'
import { humanId } from 'human-id'
import { createWorkspaceStore } from './createWorkspaceStore'
import type { WorkspaceStore, WorkspaceStoreDeps } from './createWorkspaceStore'
import { createTtyStore, createTtyWriter } from './createTtyStore'
import type { Tty, TtyWriter, TtyTerminalDeps } from './createTtyStore'
import type {
  Workspace, Session, AppState, GitInfo,
  ConnectionInfo, ActivityState,
  TerminalApi, GitApi, FilesystemApi, SessionApi, Settings, WorktreeSettings,
  Application, SandboxConfig, TTYSessionInfo, LlmApi, GitHubApi, RunActionsApi
} from '../types'

export type WorkspaceEntry =
  | { status: 'loading'; name: string; message: string; output: string[] }
  | { status: 'error'; name: string; error: string }
  | { status: 'loaded'; data: Workspace; store: WorkspaceStore }
  | { status: 'operation-error'; data: Workspace; store: WorkspaceStore; error: string }

export type SessionEntry = { store: StoreApi<SessionState> }

export interface AppRegistryApi {
  get: (id: string) => Application | undefined
  getDefaultApp: (appId?: string) => Application | null
}

export interface SessionDeps {
  git: GitApi
  filesystem: FilesystemApi
  runActions: RunActionsApi
  sessionApi: SessionApi
  terminal: TerminalApi
  github: GitHubApi
  getSettings: () => Settings
  appRegistry: AppRegistryApi
  llm: Pick<LlmApi, 'analyzeTerminal' | 'generateTitle'>
  setActivityTabState: (tabId: string, state: ActivityState) => void
}

export interface SessionState {
  sessionId: string

  // SSH connection for this session (transitions: connecting → connected/error)
  connection: ConnectionInfo | null

  // TTY writers (write-only, keyed by ptyId)
  ttyWriters: Record<string, TtyWriter>
  createTty: (cwd: string, sandbox?: SandboxConfig, startupCommand?: string) => Promise<string>
  openTtyStream: (ptyId: string) => Promise<{ tty: Tty }>
  getTtyWriter: (ptyId: string) => Promise<TtyWriter>
  killTty: (ptyId: string) => void
  listTty: () => Promise<TTYSessionInfo[]>

  // Workspace collection
  workspaces: Record<string, WorkspaceEntry>
  activeWorkspaceId: string | null
  isRestoring: boolean
  sessionVersion: number

  clearWorkspaceError: (id: string) => void
  closeWorkspace: (id: string) => void
  addWorkspace: (path: string, options?: { skipDefaultTabs?: boolean; settings?: WorktreeSettings }) => string
  addChildWorkspace: (parentId: string, name: string, isDetached?: boolean, settings?: WorktreeSettings, description?: string) => { success: boolean; error?: string }
  adoptExistingWorktree: (parentId: string, worktreePath: string, branch: string, name: string, settings?: WorktreeSettings, description?: string) => Promise<{ success: boolean; error?: string }>
  createWorktreeFromBranch: (parentId: string, branch: string, isDetached: boolean, settings?: WorktreeSettings, description?: string) => { success: boolean; error?: string }
  createWorktreeFromRemote: (parentId: string, remoteBranch: string, isDetached: boolean, settings?: WorktreeSettings, description?: string) => { success: boolean; error?: string }
  removeWorkspace: (id: string) => Promise<void>
  removeWorkspaceKeepBranch: (id: string) => Promise<void>
  removeWorkspaceKeepBoth: (id: string) => Promise<void>
  removeOrphanWorkspace: (id: string) => void
  mergeAndRemoveWorkspace: (id: string, squash: boolean) => Promise<{ success: boolean; error?: string }>
  mergeAndKeepWorkspace: (id: string, squash: boolean) => Promise<{ success: boolean; error?: string }>
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
export function getUnmergedSubWorkspaces(workspaces: Record<string, WorkspaceEntry>): Workspace[] {
  return Object.values(workspaces)
    .filter((e): e is Extract<WorkspaceEntry, { status: 'loaded' | 'operation-error' }> =>
      e.status === 'loaded' || e.status === 'operation-error')
    .map(e => e.data)
    .filter(ws => ws.isWorktree && ws.status === 'active')
}

export function createSessionStore(
  config: { sessionId: string; windowUuid: string | null; connection?: ConnectionInfo },
  deps: SessionDeps
): StoreApi<SessionState> {
  let syncDebounceTimer: ReturnType<typeof setTimeout> | null = null

  async function syncSessionToDaemon(isRestoring: boolean = false): Promise<void> {
    try {
      const { workspaces, connection } = store.getState()
      console.log('[session] syncSessionToDaemon called - workspaces:', Object.keys(workspaces).length, 'isRestoring:', isRestoring)

      if (connection && connection.status !== 'connected') {
        console.log('[session] connection not yet established, skipping sync')
        return
      }

      if (isRestoring) {
        console.log('[session] currently restoring, skipping sync')
        return
      }

      const daemonWorkspaces = Object.values(workspaces)
        .filter((e): e is Extract<WorkspaceEntry, { status: 'loaded' }> => e.status === 'loaded')
        .map(e => { const { createdAt, lastActivity, ...ws } = e.data; return ws })

      console.log('[session] syncing to daemon:', daemonWorkspaces.length, 'workspaces', JSON.stringify(daemonWorkspaces))

      const currentVersion = store.getState().sessionVersion
      const { sessionId } = store.getState()
      console.log('[session] updating session:', sessionId, 'senderUuid:', config.windowUuid, 'expectedVersion:', currentVersion)
      const result = await deps.sessionApi.update(sessionId, daemonWorkspaces, config.windowUuid || undefined, currentVersion)
      if (!result.success) {
        console.error('[session] failed to update session:', result.error)
      } else if (result.session) {
        if (result.session.version === currentVersion + 1) {
          // Update accepted
          store.setState({ sessionVersion: result.session.version })
          console.log('[session] session updated successfully, version:', result.session.version)
        } else {
          // Update rejected (version mismatch) — reconcile from daemon's current state
          console.log('[session] session update rejected, expected version:', currentVersion + 1, 'got:', result.session.version, '— reconciling')
          await store.getState().handleExternalUpdate(result.session)
        }
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
      openTtyStream: (ptyId: string) => store.getState().openTtyStream(ptyId),
      getTtyWriter: (ptyId: string) => store.getState().getTtyWriter(ptyId),
      createTty: (cwd, sandbox?, startupCommand?) => store.getState().createTty(cwd, sandbox, startupCommand),
      connectionId: config.connection?.id ?? 'local',
      git: deps.git,
      filesystem: deps.filesystem,
      runActions: deps.runActions,
      getSettings: deps.getSettings,
      llm: deps.llm,
      setActivityTabState: deps.setActivityTabState,
      syncToDaemon: () => debouncedSyncToDaemon(),
      removeWorkspace: (id) => store.getState().removeWorkspace(id),
      removeWorkspaceKeepBranch: (id) => store.getState().removeWorkspaceKeepBranch(id),
      removeWorkspaceKeepBoth: (id) => store.getState().removeWorkspaceKeepBoth(id),
      mergeAndRemoveWorkspace: (id, squash) => store.getState().mergeAndRemoveWorkspace(id, squash),
      mergeAndKeepWorkspace: (id, squash) => store.getState().mergeAndKeepWorkspace(id, squash),
      closeAndCleanWorkspace: (id) => store.getState().closeAndCleanWorkspace(id),
      quickForkWorkspace: (id) => store.getState().quickForkWorkspace(id),
      refreshGitInfo: (id) => store.getState().refreshGitInfo(id),
      lookupWorkspace: (id) => {
        const entry = store.getState().workspaces[id]
        return entry && (entry.status === 'loaded' || entry.status === 'operation-error') ? entry.data : undefined
      },
      github: deps.github,
    }
  }

  function createHandleForWorkspace(workspace: Workspace): WorkspaceStore {
    const handle = createWorkspaceStore(workspace, makeHandleDeps())

    // Keep the workspaces snapshot in sync when handle state changes
    handle.subscribe((state) => {
      store.setState((s) => {
        const entry = s.workspaces[state.workspace.id]
        if (!entry || (entry.status !== 'loaded' && entry.status !== 'operation-error')) return s
        return {
          workspaces: { ...s.workspaces, [state.workspace.id]: { ...entry, data: state.workspace } }
        }
      })
    })

    return handle
  }

  // Shared helper: creates a placeholder child workspace with loading state and fires a git operation
  function createChildWithLoading(
    parentId: string,
    worktreeName: string,
    options: {
      isDetached?: boolean
      settings?: WorktreeSettings
      description?: string
      initialBranch?: string | null
      message: string
      gitOperation: (operationId: string) => Promise<{ success: boolean; path?: string; branch?: string; error?: string }>
      preOperation?: () => Promise<void>
    }
  ): { success: true } {
    const state = store.getState()
    const parentEntry = state.workspaces[parentId]
    const parent = parentEntry && (parentEntry.status === 'loaded' || parentEntry.status === 'operation-error') ? parentEntry.data : undefined

    const id = generateId()
    const operationId = generateId()

    store.setState((s) => ({
      workspaces: { ...s.workspaces, [id]: { status: 'loading' as const, name: worktreeName, message: options.message, output: [] } },
      activeWorkspaceId: id,
    }))

    const unsubOutput = deps.git.onOutput((opId, data) => {
      if (opId !== operationId) return
      const entry = store.getState().workspaces[id]
      if (entry?.status === 'loading') {
        store.setState(s => ({
          workspaces: { ...s.workspaces, [id]: { ...entry, output: [...entry.output, data] } }
        }))
      }
    })

    ;(async () => {
      try {
        if (options.preOperation) {
          await options.preOperation()
        }

        const result = await options.gitOperation(operationId)

        if (!result.success) {
          store.setState(s => ({
            workspaces: { ...s.workspaces, [id]: { status: 'error', name: worktreeName, error: result.error || 'Operation failed' } }
          }))
          return
        }

        // Build workspace data and store only on success
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
          name: worktreeName,
          path: result.path!,
          parentId,
          status: 'active',
          isGitRepo: true,
          gitBranch: result.branch!,
          gitRootPath: parent?.gitRootPath ?? null,
          isWorktree: true,
          isDetached: options.isDetached ?? false,
          appStates,
          activeTabId,
          settings: options.settings ?? { defaultApplicationId: '' },
          metadata: {
            ...(options.description ? { description: options.description } : {}),
            ...(options.initialBranch ? { branchIsUserDefined: 'true' } : {}),
          },
          createdAt: Date.now(),
          lastActivity: Date.now(),
        }

        const handle = createHandleForWorkspace(childWorkspace)
        for (const tabId of Object.keys(appStates)) {
          handle.getState().initTab(tabId)
        }

        store.setState(s => ({
          workspaces: { ...s.workspaces, [id]: { status: 'loaded', data: childWorkspace, store: handle } }
        }))
        await syncSessionToDaemon(store.getState().isRestoring)
      } catch (err) {
        store.setState(s => ({
          workspaces: { ...s.workspaces, [id]: { status: 'error', name: worktreeName, error: err instanceof Error ? err.message : String(err) } }
        }))
      } finally {
        unsubOutput()
      }
    })()

    return { success: true }
  }

  // Shared helper: creates a child workspace from a git operation result (used by adoptExistingWorktree)
  async function addChildWorkspaceFromResult(
    parentId: string,
    name: string,
    path: string,
    branch: string,
    options: { isDetached?: boolean; isWorktree?: boolean; settings?: WorktreeSettings; metadata?: Record<string, string> } = {}
  ): Promise<string> {
    const parentEntry = store.getState().workspaces[parentId]
    const parent = parentEntry && (parentEntry.status === 'loaded' || parentEntry.status === 'operation-error') ? parentEntry.data : undefined

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
      status: 'active',
      isGitRepo: true,
      gitBranch: branch,
      gitRootPath: parent?.gitRootPath ?? null,
      isWorktree: options.isWorktree ?? true,
      isDetached: options.isDetached ?? false,
      appStates,
      activeTabId,
      settings: options.settings ?? { defaultApplicationId: '' },
      metadata: options.metadata ?? {},
      createdAt: Date.now(),
      lastActivity: Date.now(),
    }

    const handle = createHandleForWorkspace(childWorkspace)

    store.setState((s) => ({
      workspaces: { ...s.workspaces, [id]: { status: 'loaded', data: childWorkspace, store: handle } },
      activeWorkspaceId: id
    }))

    for (const tabId of Object.keys(appStates)) {
      handle.getState().initTab(tabId)
    }

    await syncSessionToDaemon(store.getState().isRestoring)
    return id
  }

  // Shared helper: removes a workspace with configurable git cleanup behavior
  async function removeWorkspaceInternal(
    id: string,
    options: { keepBranch: boolean; keepWorktree: boolean; operationId?: string }
  ): Promise<void> {
    const entry = store.getState().workspaces[id]
    if (!entry) return
    const workspace = (entry.status === 'loaded' || entry.status === 'operation-error') ? entry.data : undefined
    const handle = (entry.status === 'loaded' || entry.status === 'operation-error') ? entry.store : undefined

    // Recursively remove children first (derived from parentId)
    const childIds = Object.entries(store.getState().workspaces)
      .filter(([, e]) => (e.status === 'loaded' || e.status === 'operation-error') && e.data.parentId === id)
      .map(([childId]) => childId)
    for (const childId of childIds) {
      await removeWorkspaceInternal(childId, options)
    }

    // Dispose tab refs (stops analyzers, kills PTYs, etc.)
    if (handle && workspace) {
      handle.getState().gitController.getState().dispose()
      for (const tabId of Object.keys(workspace.appStates)) {
        const ref = handle.getState().getTabRef(tabId)
        if (ref) ref.dispose()
      }
    }

    // Git cleanup
    if (workspace?.isWorktree && workspace.gitRootPath) {
      if (!options.keepWorktree) {
        const deleteBranch = !options.keepBranch && !workspace.isDetached
        await deps.git.removeWorktree(
          workspace.gitRootPath,
          workspace.path,
          deleteBranch,
          options.operationId
        )
      } else if (!options.keepBranch && !workspace.isDetached && workspace.gitBranch) {
        await deps.git.deleteBranch(workspace.gitRootPath, workspace.gitBranch, options.operationId)
      }
    }

    // Remove workspace entry
    store.setState((s) => {
      const { [id]: _, ...remaining } = s.workspaces
      return {
        workspaces: remaining,
        activeWorkspaceId: s.activeWorkspaceId === id ? null : s.activeWorkspaceId
      }
    })

    await syncSessionToDaemon(store.getState().isRestoring)
  }

  // Helper: wraps removeWorkspaceInternal with loading state + output streaming
  async function removeWorkspaceWithLoading(
    id: string,
    options: { keepBranch: boolean; keepWorktree: boolean }
  ): Promise<void> {
    const entry = store.getState().workspaces[id]
    if (!entry || (entry.status !== 'loaded' && entry.status !== 'operation-error')) return
    const { data, store: wsStore } = entry

    const operationId = generateId()
    // Temporarily show loading in the main pane — preserve data+store for recovery
    store.setState(s => ({
      workspaces: { ...s.workspaces, [id]: { status: 'loaded', data, store: wsStore } }
    }))
    const unsubOutput = deps.git.onOutput((opId, _data) => {
      if (opId !== operationId) return
      // Output streaming not needed for remove — workspace is removed on success
    })
    try {
      await removeWorkspaceInternal(id, { ...options, operationId })
    } catch (err) {
      store.setState(s => ({
        workspaces: { ...s.workspaces, [id]: { status: 'operation-error', data, store: wsStore, error: err instanceof Error ? err.message : String(err) } }
      }))
    } finally {
      unsubOutput()
    }
  }

  // Shared helper: validates workspace, sets loading state, auto-commits, and performs git merge.
  // Returns { success, error, operationId } — caller decides post-merge behavior.
  async function mergeWorkspaceCore(
    id: string,
    squash: boolean
  ): Promise<{ success: boolean; error?: string; operationId?: string }> {
    const entry = store.getState().workspaces[id]
    if (!entry || (entry.status !== 'loaded' && entry.status !== 'operation-error')) {
      return { success: false, error: 'Workspace not found' }
    }
    const { data: workspace, store: wsStore } = entry

    if (!workspace.isWorktree || !workspace.parentId) {
      return { success: false, error: 'Not a worktree workspace' }
    }

    const parentEntry = store.getState().workspaces[workspace.parentId]
    const parent = parentEntry && (parentEntry.status === 'loaded' || parentEntry.status === 'operation-error') ? parentEntry.data : undefined
    if (!parent || !parent.gitRootPath || !parent.gitBranch) {
      return { success: false, error: 'Parent workspace not found or not a git repo' }
    }

    const operationId = generateId()
    // Keep data+store accessible during merge (for recovery on error)
    const unsubOutput = deps.git.onOutput((opId, _data) => {
      if (opId !== operationId) return
      // Merge output not streamed to UI — workspace stays in loaded state
    })

    try {
      // Block merge if parent worktree has uncommitted changes
      const parentHasChanges = await deps.git.hasUncommittedChanges(parent.path)
      if (parentHasChanges) {
        store.setState(s => ({
          workspaces: { ...s.workspaces, [id]: { status: 'operation-error', data: workspace, store: wsStore, error: 'Parent workspace has uncommitted changes. Commit or stash them before merging.' } }
        }))
        return { success: false, error: 'Parent workspace has uncommitted changes. Commit or stash them before merging.' }
      }

      const hasChanges = await deps.git.hasUncommittedChanges(workspace.path)
      if (hasChanges) {
        const commitResult = await deps.git.commitAll(
          workspace.path,
          `WIP: Auto-commit before merge from ${workspace.name}`
        )
        if (!commitResult.success) {
          store.setState(s => ({
            workspaces: { ...s.workspaces, [id]: { status: 'operation-error', data: workspace, store: wsStore, error: `Failed to commit changes: ${commitResult.error}` } }
          }))
          return { success: false, error: `Failed to commit changes: ${commitResult.error}` }
        }
      }

      const mergeResult = await deps.git.merge(
        parent.path,
        workspace.gitBranch!,
        squash,
        operationId
      )

      if (!mergeResult.success) {
        store.setState(s => ({
          workspaces: { ...s.workspaces, [id]: { status: 'operation-error', data: workspace, store: wsStore, error: `Merge failed: ${mergeResult.error}` } }
        }))
        return { success: false, error: `Merge failed: ${mergeResult.error}` }
      }

      return { success: true, operationId }
    } catch (err) {
      store.setState(s => ({
        workspaces: { ...s.workspaces, [id]: { status: 'operation-error', data: workspace, store: wsStore, error: err instanceof Error ? err.message : String(err) } }
      }))
      return { success: false, error: err instanceof Error ? err.message : String(err) }
    } finally {
      unsubOutput()
    }
  }

  const connectionId = config.connection?.id ?? 'local'

  // Create a terminal wrapper with connectionId bound for tty stores
  const boundTerminal: TtyTerminalDeps = {
    write: deps.terminal.write,
    resize: deps.terminal.resize,
    kill: (sessionId: string) => deps.terminal.kill(connectionId, sessionId),
    isAlive: (id: string) => deps.terminal.isAlive(connectionId, id),
    onEvent: deps.terminal.onEvent,
  }

  const store = createStore<SessionState>()((set, get) => ({
    sessionId: config.sessionId,
    ttyWriters: {},
    workspaces: {},
    activeWorkspaceId: null,
    isRestoring: false,
    sessionVersion: 0,

    connection: config.connection ?? null,

    createTty: async (cwd: string, sandbox?: SandboxConfig, startupCommand?: string): Promise<string> => {
      const result = await deps.terminal.create(connectionId, cwd, sandbox, startupCommand)
      if (!result.success) {
        throw new Error(result.error || 'Failed to create PTY')
      }
      const writer = createTtyWriter(result.sessionId, result.handle, boundTerminal)
      set((s) => ({
        ttyWriters: { ...s.ttyWriters, [result.sessionId]: writer }
      }))
      return result.sessionId
    },

    openTtyStream: async (ptyId: string): Promise<{ tty: Tty }> => {
      const result = await deps.terminal.attach(connectionId, ptyId)
      if (!result.success) {
        throw new Error(result.error || 'Failed to attach to PTY')
      }
      const tty = createTtyStore(ptyId, result.handle, boundTerminal)
      return { tty }
    },

    getTtyWriter: async (ptyId: string): Promise<TtyWriter> => {
      const cached = get().ttyWriters[ptyId]
      if (cached) return cached
      const result = await deps.terminal.attach(connectionId, ptyId)
      if (!result.success) {
        throw new Error(result.error || 'Failed to attach to PTY')
      }
      const writer = createTtyWriter(ptyId, result.handle, boundTerminal)
      set((s) => ({
        ttyWriters: { ...s.ttyWriters, [ptyId]: writer }
      }))
      return writer
    },

    killTty: (ptyId: string): void => {
      boundTerminal.kill(ptyId)
    },

    listTty: (): Promise<TTYSessionInfo[]> => {
      return deps.terminal.list(connectionId)
    },

    clearWorkspaceError: (id: string): void => {
      const entry = get().workspaces[id]
      if (!entry || entry.status !== 'operation-error') return
      set((s) => ({
        workspaces: { ...s.workspaces, [id]: { status: 'loaded', data: entry.data, store: entry.store } }
      }))
    },

    closeWorkspace: (id: string): void => {
      set((s) => {
        const { [id]: _, ...rest } = s.workspaces
        return {
          workspaces: rest,
          activeWorkspaceId: s.activeWorkspaceId === id ? null : s.activeWorkspaceId
        }
      })
    },

    addWorkspace: (path: string, options?: { skipDefaultTabs?: boolean; settings?: WorktreeSettings }) => {
      console.log('[session] addWorkspace called for path:', path)
      const id = generateId()
      const name = getNameFromPath(path)

      set((s) => ({
        workspaces: { ...s.workspaces, [id]: { status: 'loading' as const, name, message: 'Loading workspace...', output: [] } },
        activeWorkspaceId: id,
      }))

      // Fire-and-forget: resolve git info then create workspace+handle
      deps.git.getInfo(path).then(gitInfo => {
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
          name,
          path,
          parentId: null,
          status: 'active',
          isGitRepo: gitInfo.isRepo,
          gitBranch: gitInfo.isRepo ? gitInfo.branch : null,
          gitRootPath: gitInfo.isRepo ? gitInfo.rootPath : null,
          isWorktree: false,
          isDetached: false,
          appStates,
          activeTabId,
          settings: options?.settings ?? { defaultApplicationId: '' },
          metadata: {},
          createdAt: Date.now(),
          lastActivity: Date.now(),
        }

        const handle = createHandleForWorkspace(workspace)
        for (const tabId of Object.keys(appStates)) {
          handle.getState().initTab(tabId)
        }

        set(s => ({
          workspaces: { ...s.workspaces, [id]: { status: 'loaded', data: workspace, store: handle } }
        }))
        syncSessionToDaemon(get().isRestoring)
      }).catch(err => {
        set(s => ({
          workspaces: { ...s.workspaces, [id]: { status: 'error', name, error: err instanceof Error ? err.message : String(err) } }
        }))
      })

      return id
    },

    addChildWorkspace: (parentId: string, name: string, isDetached: boolean = false, settings?: WorktreeSettings, description?: string) => {
      const parentEntry = get().workspaces[parentId]
      const parent = parentEntry && (parentEntry.status === 'loaded' || parentEntry.status === 'operation-error') ? parentEntry.data : undefined

      if (!parent) {
        return { success: false, error: 'Parent workspace not found' }
      }

      if (!parent.isGitRepo || !parent.gitRootPath) {
        return { success: false, error: 'Parent workspace is not a git repository' }
      }

      return createChildWithLoading(parentId, name, {
        isDetached, settings, description,
        message: 'Creating worktree...',
        preOperation: async () => {
          const currentGitInfo = await deps.git.getInfo(parent.path)
          if (currentGitInfo.isRepo && currentGitInfo.branch !== parent.gitBranch) {
            get().updateGitInfo(parentId, currentGitInfo)
          }
        },
        gitOperation: (operationId) => {
          const currentParentEntry = get().workspaces[parentId]
          const currentParent = currentParentEntry && (currentParentEntry.status === 'loaded' || currentParentEntry.status === 'operation-error') ? currentParentEntry.data : undefined
          return deps.git.createWorktree(
            parent.gitRootPath!,
            name,
            currentParent?.gitBranch || undefined,
            operationId
          )
        },
      })
    },

    adoptExistingWorktree: async (parentId: string, worktreePath: string, branch: string, name: string, settings?: WorktreeSettings, description?: string) => {
      const parentEntry = get().workspaces[parentId]
      if (!parentEntry || (parentEntry.status !== 'loaded' && parentEntry.status !== 'operation-error')) {
        return { success: false, error: 'Parent workspace not found' }
      }

      const alreadyOpen = Object.values(get().workspaces).some(
        e => (e.status === 'loaded' || e.status === 'operation-error') && e.data.path === worktreePath
      )
      if (alreadyOpen) {
        return { success: false, error: 'This worktree is already open' }
      }

      const metadata: Record<string, string> = { branchIsUserDefined: 'true', ...(description ? { description } : {}) }
      await addChildWorkspaceFromResult(parentId, name, worktreePath, branch, { settings, metadata })
      return { success: true }
    },

    createWorktreeFromBranch: (parentId: string, branch: string, isDetached: boolean, settings?: WorktreeSettings, description?: string) => {
      console.log('[session] createWorktreeFromBranch called:', { parentId, branch, isDetached })
      const parentEntry = get().workspaces[parentId]
      const parent = parentEntry && (parentEntry.status === 'loaded' || parentEntry.status === 'operation-error') ? parentEntry.data : undefined

      if (!parent) {
        return { success: false, error: 'Parent workspace not found' }
      }

      if (!parent.isGitRepo || !parent.gitRootPath) {
        return { success: false, error: 'Parent workspace is not a git repository' }
      }

      const worktreeName = branch.split('/').pop() || branch
      return createChildWithLoading(parentId, worktreeName, {
        isDetached, settings, description,
        initialBranch: branch,
        message: 'Creating worktree from branch...',
        gitOperation: (operationId) => deps.git.createWorktreeFromBranch(
          parent.gitRootPath!,
          branch,
          worktreeName,
          operationId
        ),
      })
    },

    createWorktreeFromRemote: (parentId: string, remoteBranch: string, isDetached: boolean, settings?: WorktreeSettings, description?: string) => {
      console.log('[session] createWorktreeFromRemote called:', { parentId, remoteBranch, isDetached })
      const parentEntry = get().workspaces[parentId]
      const parent = parentEntry && (parentEntry.status === 'loaded' || parentEntry.status === 'operation-error') ? parentEntry.data : undefined

      if (!parent) {
        return { success: false, error: 'Parent workspace not found' }
      }

      if (!parent.isGitRepo || !parent.gitRootPath) {
        return { success: false, error: 'Parent workspace is not a git repository' }
      }

      const worktreeName = remoteBranch.split('/').pop() || remoteBranch
      return createChildWithLoading(parentId, worktreeName, {
        isDetached, settings, description,
        initialBranch: remoteBranch,
        message: 'Creating worktree from remote...',
        gitOperation: (operationId) => deps.git.createWorktreeFromRemote(
          parent.gitRootPath!,
          remoteBranch,
          worktreeName,
          operationId
        ),
      })
    },

    removeWorkspace: (id: string) =>
      removeWorkspaceWithLoading(id, { keepBranch: false, keepWorktree: false }),

    removeWorkspaceKeepBranch: (id: string) =>
      removeWorkspaceWithLoading(id, { keepBranch: true, keepWorktree: false }),

    removeWorkspaceKeepBoth: (id: string) =>
      removeWorkspaceWithLoading(id, { keepBranch: true, keepWorktree: true }),

    removeOrphanWorkspace: (id: string) => {
      if (!get().workspaces[id]) return
      set((s) => {
        const { [id]: _, ...remaining } = s.workspaces
        return {
          workspaces: remaining,
          activeWorkspaceId: s.activeWorkspaceId === id ? null : s.activeWorkspaceId
        }
      })
    },

    setActiveWorkspace: (id: string | null) => {
      set({ activeWorkspaceId: id })
    },

    updateGitInfo: (id: string, gitInfo: GitInfo) => {
      const entry = get().workspaces[id]
      if (!entry || (entry.status !== 'loaded' && entry.status !== 'operation-error')) return
      entry.store.setState(s => ({
        workspace: {
          ...s.workspace,
          isGitRepo: gitInfo.isRepo,
          gitBranch: gitInfo.isRepo ? gitInfo.branch : null,
          gitRootPath: gitInfo.isRepo ? gitInfo.rootPath : null
        }
      }))
      syncSessionToDaemon(get().isRestoring).catch(console.error)
    },

    refreshGitInfo: async (id: string) => {
      const entry = get().workspaces[id]
      if (!entry || (entry.status !== 'loaded' && entry.status !== 'operation-error')) return
      const gitInfo = await deps.git.getInfo(entry.data.path)
      get().updateGitInfo(id, gitInfo)
    },

    mergeAndRemoveWorkspace: async (id: string, squash: boolean) => {
      const result = await mergeWorkspaceCore(id, squash)
      if (!result.success) return result

      const entry = get().workspaces[id]
      if (entry && (entry.status === 'loaded' || entry.status === 'operation-error')) {
        entry.store.getState().updateStatus('merged')
      }

      try {
        await removeWorkspaceInternal(id, { keepBranch: false, keepWorktree: false, operationId: result.operationId })
      } catch (err) {
        // Merge succeeded but removal failed — show operation error
        const currentEntry = get().workspaces[id]
        if (currentEntry && (currentEntry.status === 'loaded' || currentEntry.status === 'operation-error')) {
          store.setState(s => ({
            workspaces: { ...s.workspaces, [id]: { status: 'operation-error', data: currentEntry.data, store: currentEntry.store, error: `Merge succeeded but cleanup failed: ${err instanceof Error ? err.message : String(err)}` } }
          }))
        }
        return { success: false, error: err instanceof Error ? err.message : String(err) }
      }

      // Refresh parent's remote status after merge
      const wsData = entry && (entry.status === 'loaded' || entry.status === 'operation-error') ? entry.data : undefined
      if (wsData?.parentId) {
        const parentEntry = get().workspaces[wsData.parentId]
        if (parentEntry && (parentEntry.status === 'loaded' || parentEntry.status === 'operation-error')) {
          parentEntry.store.getState().gitController.getState().refreshRemoteStatus()
        }
      }

      return { success: true }
    },

    mergeAndKeepWorkspace: async (id: string, squash: boolean) => {
      const result = await mergeWorkspaceCore(id, squash)
      if (!result.success) return result

      // On success, ensure workspace is back to loaded status
      const entry = get().workspaces[id]
      if (entry && entry.status === 'operation-error') {
        store.setState(s => ({
          workspaces: { ...s.workspaces, [id]: { status: 'loaded', data: entry.data, store: entry.store } }
        }))
      }

      // Refresh workspace diff status and git info
      const currentEntry = get().workspaces[id]
      if (currentEntry && currentEntry.status === 'loaded') {
        currentEntry.store.getState().gitController.getState().refreshDiffStatus()
      }
      get().refreshGitInfo(id)

      // Refresh parent's remote status after merge
      if (currentEntry && currentEntry.status === 'loaded' && currentEntry.data.parentId) {
        const parentEntry = get().workspaces[currentEntry.data.parentId]
        if (parentEntry && (parentEntry.status === 'loaded' || parentEntry.status === 'operation-error')) {
          parentEntry.store.getState().gitController.getState().refreshRemoteStatus()
        }
      }

      return { success: true }
    },

    closeAndCleanWorkspace: async (id: string) => {
      const entry = get().workspaces[id]
      if (!entry || (entry.status !== 'loaded' && entry.status !== 'operation-error')) {
        return { success: false, error: 'Workspace not found' }
      }
      const workspace = entry.data

      if (!workspace.isWorktree || !workspace.parentId) {
        return { success: false, error: 'Not a worktree workspace' }
      }

      const parentEntry = get().workspaces[workspace.parentId]
      if (!parentEntry || (parentEntry.status !== 'loaded' && parentEntry.status !== 'operation-error') || !parentEntry.data.gitRootPath) {
        return { success: false, error: 'Parent workspace not found or not a git repo' }
      }

      await get().removeWorkspace(id)
      return { success: true }
    },

    quickForkWorkspace: async (workspaceId: string) => {
      const entry = get().workspaces[workspaceId]
      if (!entry || (entry.status !== 'loaded' && entry.status !== 'operation-error')) {
        return { success: false, error: 'Workspace not found' }
      }
      const ws = entry.data

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
      console.log('[Session] Restoring session', daemonSession.id, 'with', daemonSession.workspaces.length, 'workspaces, version:', daemonSession.version)

      set({ isRestoring: true, sessionVersion: daemonSession.version })
      applySessionWorkspaces(store, daemonSession.workspaces, createHandleForWorkspace, { restoreExisting: true })
      set({ isRestoring: false })

      console.log('[Session] Session restore complete, workspace count:', Object.keys(get().workspaces).length)
    },

    handleExternalUpdate: async (daemonSession: Session) => {
      const currentVersion = get().sessionVersion
      if (daemonSession.version <= currentVersion) {
        console.log('[Session] Ignoring stale external update, incoming version:', daemonSession.version, 'current:', currentVersion)
        return
      }

      console.log('[Session] External session update received, version:', daemonSession.version, 'current:', currentVersion)

      set({ isRestoring: true, sessionVersion: daemonSession.version })
      applySessionWorkspaces(store, daemonSession.workspaces, createHandleForWorkspace, { restoreExisting: false })

      // Remove workspaces not present in daemon session (skip non-loaded workspaces)
      const incomingPaths = new Set(daemonSession.workspaces.map(ws => ws.path))
      const updatedState = get()
      for (const [id, entry] of Object.entries(updatedState.workspaces)) {
        if (entry.status === 'loaded' && !incomingPaths.has(entry.data.path)) {
          get().removeOrphanWorkspace(id)
        }
      }

      set({ isRestoring: false })
      console.log('[Session] External session update applied, version:', daemonSession.version)
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
  const entry = store.getState().workspaces[existingId]
  if (!entry || (entry.status !== 'loaded' && entry.status !== 'operation-error')) return
  const ws = entry.store.getState().workspace
  entry.store.setState({
    workspace: { ...ws, metadata: daemonWorkspace.metadata, name: daemonWorkspace.name }
  })
}

// Helper: find existing loaded workspace by path
function findLoadedByPath(
  store: StoreApi<SessionState>,
  path: string
): { id: string } | undefined {
  for (const [id, entry] of Object.entries(store.getState().workspaces)) {
    if ((entry.status === 'loaded' || entry.status === 'operation-error') && entry.data.path === path) {
      return { id }
    }
  }
  return undefined
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
    const existing = findLoadedByPath(store, daemonWorkspace.path)

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
    const existing = findLoadedByPath(store, daemonWorkspace.path)

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
  const entry = store.getState().workspaces[workspaceId]
  if (!entry || (entry.status !== 'loaded' && entry.status !== 'operation-error')) return
  const ws = entry.store.getState().workspace
  entry.store.setState({
    workspace: {
      ...ws,
      appStates: daemonWorkspace.appStates,
      activeTabId: daemonWorkspace.activeTabId || Object.keys(daemonWorkspace.appStates)[0] || null
    }
  })

  for (const tabId of Object.keys(daemonWorkspace.appStates)) {
    entry.store.getState().initTab(tabId)
  }
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
    activeTabId: daemonWorkspace.activeTabId || (Object.keys(daemonWorkspace.appStates).length > 0 ? Object.keys(daemonWorkspace.appStates)[0] : null)
  }

  const handle = createHandleForWorkspace(workspace)

  store.setState((s) => ({
    workspaces: { ...s.workspaces, [id]: { status: 'loaded' as const, data: workspace, store: handle } },
    activeWorkspaceId: id
  }))

  for (const tabId of Object.keys(daemonWorkspace.appStates)) {
    handle.getState().initTab(tabId)
  }

  console.log('[Session] Reconstructed workspace:', daemonWorkspace.name, 'parentId:', parentId)
  return id
}
