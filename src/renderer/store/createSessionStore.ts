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
  Application, SandboxConfig, SessionInfo, LlmApi
} from '../types'

export type WorkspaceLoadState =
  | { status: 'loading'; message: string; output: string[] }
  | { status: 'error'; error: string }

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
  llm: Pick<LlmApi, 'analyzeTerminal' | 'generateTitle'>
  setActivityTabState: (tabId: string, state: ActivityState) => void
}

export interface SessionState {
  sessionId: string

  // Single SSH connection for this session (set at creation, immutable)
  connection: ConnectionInfo | null

  // TTY writers (write-only, keyed by ptyId)
  ttyWriters: Record<string, TtyWriter>
  createTty: (cwd: string, sandbox?: SandboxConfig, startupCommand?: string) => Promise<string>
  openTtyStream: (ptyId: string) => Promise<{ tty: Tty; scrollback?: string[]; exitCode?: number; cols?: number; rows?: number }>
  getTtyWriter: (ptyId: string) => Promise<TtyWriter>
  killTty: (ptyId: string) => void
  listTty: () => Promise<SessionInfo[]>

  // Workspace collection
  workspaceStores: Record<string, WorkspaceStore>
  workspaces: Record<string, Workspace>
  activeWorkspaceId: string | null
  isRestoring: boolean
  workspaceLoadStates: Record<string, WorkspaceLoadState>

  getWorkspace: (id: string) => WorkspaceStore | null
  dismissFailedWorkspace: (id: string) => void
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

      const loadStates = store.getState().workspaceLoadStates
      const daemonWorkspaces = Object.values(workspaces).filter(ws => !loadStates[ws.id]).map(({ createdAt, lastActivity, ...ws }) => ws)

      console.log('[session] syncing to daemon:', daemonWorkspaces.length, 'workspaces', JSON.stringify(daemonWorkspaces))

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
      openTtyStream: (ptyId: string) => store.getState().openTtyStream(ptyId),
      getTtyWriter: (ptyId: string) => store.getState().getTtyWriter(ptyId),
      createTty: (cwd, sandbox?, startupCommand?) => store.getState().createTty(cwd, sandbox, startupCommand),
      connectionId: config.connection?.id ?? 'local',
      git: deps.git,
      filesystem: deps.filesystem,
      getSettings: deps.getSettings,
      llm: deps.llm,
      setActivityTabState: deps.setActivityTabState,
      syncToDaemon: () => debouncedSyncToDaemon(),
      removeWorkspace: (id) => store.getState().removeWorkspace(id),
      removeWorkspaceKeepBranch: (id) => store.getState().removeWorkspaceKeepBranch(id),
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
    const parent = state.workspaces[parentId]

    const id = generateId()
    const operationId = generateId()

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
      path: `${parent?.gitRootPath}/.worktrees/${worktreeName}`,
      parentId,
      status: 'active',
      isGitRepo: true,
      gitBranch: options.initialBranch ?? null,
      gitRootPath: parent?.gitRootPath ?? null,
      isWorktree: true,
      isDetached: options.isDetached,
      appStates,
      activeTabId,
      settings: options.settings,
      metadata: {
        ...(options.description ? { description: options.description } : {}),
        ...(options.initialBranch ? { branchIsUserDefined: 'true' } : {}),
      },
      createdAt: Date.now(),
      lastActivity: Date.now(),
    }

    const handle = createHandleForWorkspace(childWorkspace)

    store.setState((s) => ({
      workspaceStores: { ...s.workspaceStores, [id]: handle },
      workspaces: { ...s.workspaces, [id]: childWorkspace },
      activeWorkspaceId: id,
      workspaceLoadStates: { ...s.workspaceLoadStates, [id]: { status: 'loading' as const, message: options.message, output: [] } }
    }))

    const unsubOutput = deps.git.onOutput((opId, data) => {
      if (opId !== operationId) return
      const loadState = store.getState().workspaceLoadStates[id]
      if (loadState?.status === 'loading') {
        store.setState(s => ({
          workspaceLoadStates: { ...s.workspaceLoadStates, [id]: { ...loadState, output: [...loadState.output, data] } }
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
            workspaceLoadStates: { ...s.workspaceLoadStates, [id]: { status: 'error', error: result.error || 'Operation failed' } }
          }))
          return
        }

        const wsHandle = store.getState().workspaceStores[id]
        if (wsHandle) {
          wsHandle.setState(s => ({
            workspace: { ...s.workspace, path: result.path!, gitBranch: result.branch! }
          }))
          for (const tabId of Object.keys(appStates)) {
            wsHandle.getState().initTab(tabId)
          }
        }

        store.setState(s => {
          const { [id]: _, ...rest } = s.workspaceLoadStates
          return { workspaceLoadStates: rest }
        })
        await syncSessionToDaemon(store.getState().isRestoring)
      } catch (err) {
        store.setState(s => ({
          workspaceLoadStates: { ...s.workspaceLoadStates, [id]: { status: 'error', error: err instanceof Error ? err.message : String(err) } }
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

    store.setState((s) => ({
      workspaceStores: { ...s.workspaceStores, [id]: handle },
      workspaces: { ...s.workspaces, [id]: childWorkspace },
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
    const state = store.getState()
    const workspace = state.workspaces[id]
    if (!workspace) return

    // Recursively remove children first (derived from parentId)
    const childIds = Object.values(state.workspaces)
      .filter(ws => ws.parentId === id)
      .map(ws => ws.id)
    for (const childId of childIds) {
      await removeWorkspaceInternal(childId, options)
    }

    // Dispose tab refs (stops analyzers, kills PTYs, etc.)
    const handle = state.workspaceStores[id]
    if (handle) {
      handle.getState().disposeGitController()
      for (const tabId of Object.keys(workspace.appStates)) {
        const ref = handle.getState().getTabRef(tabId)
        if (ref) ref.dispose()
      }
    }

    // Git cleanup
    if (workspace.isWorktree && workspace.gitRootPath) {
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

    // Remove handle, workspace, and any stale load state
    store.setState((s) => {
      const { [id]: _removedHandle, ...remainingHandles } = s.workspaceStores
      const { [id]: _removedWs, ...remainingWorkspaces } = s.workspaces
      const { [id]: _removedLoadState, ...remainingLoadStates } = s.workspaceLoadStates
      return {
        workspaceStores: remainingHandles,
        workspaces: remainingWorkspaces,
        workspaceLoadStates: remainingLoadStates,
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
    const operationId = generateId()
    store.setState(s => ({
      workspaceLoadStates: { ...s.workspaceLoadStates, [id]: { status: 'loading' as const, message: 'Removing workspace...', output: [] } }
    }))
    const unsubOutput = deps.git.onOutput((opId, data) => {
      if (opId !== operationId) return
      const loadState = store.getState().workspaceLoadStates[id]
      if (loadState?.status === 'loading') {
        store.setState(s => ({
          workspaceLoadStates: { ...s.workspaceLoadStates, [id]: { ...loadState, output: [...loadState.output, data] } }
        }))
      }
    })
    try {
      await removeWorkspaceInternal(id, { ...options, operationId })
    } catch (err) {
      store.setState(s => ({
        workspaceLoadStates: { ...s.workspaceLoadStates, [id]: { status: 'error', error: err instanceof Error ? err.message : String(err) } }
      }))
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
    workspaceStores: {},
    workspaces: {},
    activeWorkspaceId: null,
    isRestoring: false,
    workspaceLoadStates: {},

    connection: config.connection ?? null,

    createTty: async (cwd: string, sandbox?: SandboxConfig, startupCommand?: string): Promise<string> => {
      const result = await deps.terminal.create(connectionId, cwd, sandbox, startupCommand)
      if (!result) {
        throw new Error('Failed to create PTY')
      }
      const writer = createTtyWriter(result.sessionId, result.handle, boundTerminal)
      set((s) => ({
        ttyWriters: { ...s.ttyWriters, [result.sessionId]: writer }
      }))
      return result.sessionId
    },

    openTtyStream: async (ptyId: string): Promise<{ tty: Tty; scrollback?: string[]; exitCode?: number; cols?: number; rows?: number }> => {
      const result = await deps.terminal.attach(connectionId, ptyId)
      if (!result.success || !result.handle) {
        throw new Error(result.error || 'Failed to attach to PTY')
      }
      const tty = createTtyStore(ptyId, result.handle, boundTerminal)
      return { tty, scrollback: result.scrollback, exitCode: result.exitCode, cols: result.cols, rows: result.rows }
    },

    getTtyWriter: async (ptyId: string): Promise<TtyWriter> => {
      const cached = get().ttyWriters[ptyId]
      if (cached) return cached
      const result = await deps.terminal.attach(connectionId, ptyId)
      if (!result.success || !result.handle) {
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

    listTty: (): Promise<SessionInfo[]> => {
      return deps.terminal.list(connectionId)
    },

    dismissFailedWorkspace: (id: string): void => {
      get().removeOrphanWorkspace(id)
      set((s) => {
        const { [id]: _, ...rest } = s.workspaceLoadStates
        return { workspaceLoadStates: rest }
      })
    },

    getWorkspace: (id: string): WorkspaceStore | null => {
      return get().workspaceStores[id] ?? null
    },

    addWorkspace: (path: string, options?: { skipDefaultTabs?: boolean; settings?: WorktreeSettings }) => {
      console.log('[session] addWorkspace called for path:', path)
      const id = generateId()

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
        status: 'active',
        isGitRepo: false,
        gitBranch: null,
        gitRootPath: null,
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
        activeWorkspaceId: id,
        workspaceLoadStates: { ...s.workspaceLoadStates, [id]: { status: 'loading' as const, message: 'Loading workspace...', output: [] } }
      }))

      // Fire-and-forget: resolve git info
      deps.git.getInfo(path).then(gitInfo => {
        const wsHandle = get().workspaceStores[id]
        if (wsHandle) {
          wsHandle.setState(s => ({
            workspace: { ...s.workspace, isGitRepo: gitInfo.isRepo, gitBranch: gitInfo.branch, gitRootPath: gitInfo.rootPath }
          }))
          for (const tabId of Object.keys(appStates)) {
            wsHandle.getState().initTab(tabId)
          }
        }
        set(s => {
          const { [id]: _, ...rest } = s.workspaceLoadStates
          return { workspaceLoadStates: rest }
        })
        syncSessionToDaemon(get().isRestoring)
      }).catch(err => {
        set(s => ({
          workspaceLoadStates: { ...s.workspaceLoadStates, [id]: { status: 'error', error: err instanceof Error ? err.message : String(err) } }
        }))
      })

      return id
    },

    addChildWorkspace: (parentId: string, name: string, isDetached: boolean = false, settings?: WorktreeSettings, description?: string) => {
      const state = get()
      const parent = state.workspaces[parentId]

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
          if (currentGitInfo.branch && currentGitInfo.branch !== parent.gitBranch) {
            get().updateGitInfo(parentId, currentGitInfo)
          }
        },
        gitOperation: (operationId) => {
          const currentParent = get().workspaces[parentId]
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

      const metadata: Record<string, string> = { branchIsUserDefined: 'true', ...(description ? { description } : {}) }
      await addChildWorkspaceFromResult(parentId, name, worktreePath, branch, { settings, metadata })
      return { success: true }
    },

    createWorktreeFromBranch: (parentId: string, branch: string, isDetached: boolean, settings?: WorktreeSettings, description?: string) => {
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
      const state = get()
      const parent = state.workspaces[parentId]

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
      const state = get()
      const workspace = state.workspaces[id]
      if (!workspace) return

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

      const operationId = generateId()
      store.setState(s => ({
        workspaceLoadStates: { ...s.workspaceLoadStates, [id]: { status: 'loading' as const, message: squash ? 'Squash merging workspace...' : 'Merging workspace...', output: [] } }
      }))
      const unsubOutput = deps.git.onOutput((opId, data) => {
        if (opId !== operationId) return
        const loadState = store.getState().workspaceLoadStates[id]
        if (loadState?.status === 'loading') {
          store.setState(s => ({
            workspaceLoadStates: { ...s.workspaceLoadStates, [id]: { ...loadState, output: [...loadState.output, data] } }
          }))
        }
      })

      try {
        const hasChanges = await deps.git.hasUncommittedChanges(workspace.path)
        if (hasChanges) {
          const commitResult = await deps.git.commitAll(
            workspace.path,
            `WIP: Auto-commit before merge from ${workspace.name}`
          )
          if (!commitResult.success) {
            store.setState(s => ({
              workspaceLoadStates: { ...s.workspaceLoadStates, [id]: { status: 'error', error: `Failed to commit changes: ${commitResult.error}` } }
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
            workspaceLoadStates: { ...s.workspaceLoadStates, [id]: { status: 'error', error: `Merge failed: ${mergeResult.error}` } }
          }))
          return { success: false, error: `Merge failed: ${mergeResult.error}` }
        }

        // Update status on the handle
        const handle = get().workspaceStores[id]
        if (handle) {
          handle.getState().updateStatus('merged')
        }
        await removeWorkspaceInternal(id, { keepBranch: false, keepWorktree: false, operationId })

        return { success: true }
      } catch (err) {
        store.setState(s => ({
          workspaceLoadStates: { ...s.workspaceLoadStates, [id]: { status: 'error', error: err instanceof Error ? err.message : String(err) } }
        }))
        return { success: false, error: err instanceof Error ? err.message : String(err) }
      } finally {
        unsubOutput()
      }
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
      console.log('[Session] External session update received', JSON.stringify(daemonSession))

      set({ isRestoring: true })
      applySessionWorkspaces(store, daemonSession.workspaces, createHandleForWorkspace, { restoreExisting: false })

      // Remove workspaces not present in daemon session (skip loading workspaces)
      const incomingPaths = new Set(daemonSession.workspaces.map(ws => ws.path))
      const updatedState = get()
      for (const [id, ws] of Object.entries(updatedState.workspaces)) {
        if (!incomingPaths.has(ws.path) && !updatedState.workspaceLoadStates[id]) {
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

  for (const tabId of Object.keys(daemonWorkspace.appStates)) {
    handle.getState().initTab(tabId)
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
    workspaceStores: { ...s.workspaceStores, [id]: handle },
    workspaces: { ...s.workspaces, [id]: workspace },
    activeWorkspaceId: id
  }))

  for (const tabId of Object.keys(daemonWorkspace.appStates)) {
    handle.getState().initTab(tabId)
  }

  console.log('[Session] Reconstructed workspace:', daemonWorkspace.name, 'parentId:', parentId)
  return id
}
