import { createStore } from 'zustand/vanilla'
import type { StoreApi } from 'zustand'
import { humanId } from 'human-id'
import { createWorkspaceStore } from './createWorkspaceStore'
import type { WorkspaceStore, WorkspaceStoreDeps } from './createWorkspaceStore'
import { createTtyStore } from './createTtyStore'
import type { Tty, TtyTerminalDeps } from './createTtyStore'
import type { PtyEvent } from '../../shared/ipc-types'
import { ConnectionStatus } from '../../shared/types'
import type {
  Workspace, Session, AppState, GitInfo,
  ConnectionInfo, ActivityState,
  TerminalApi, GitApi, FilesystemApi, ExecApi, SessionApi, Settings, WorktreeSettings,
  Application, SandboxConfig, TTYSessionInfo, LlmApi, GitHubApi, RunActionsApi
} from '../types'
import type { SessionLock } from '../../shared/types'

export enum WorkspaceEntryStatus {
  Loading = 'loading',
  Error = 'error',
  Loaded = 'loaded',
  OperationError = 'operation-error',
}

export type WorkspaceEntry =
  | { status: WorkspaceEntryStatus.Loading; name: string; message: string; output: string[] }
  | { status: WorkspaceEntryStatus.Error; name: string; error: string }
  | { status: WorkspaceEntryStatus.Loaded; data: Workspace; store: WorkspaceStore }
  | { status: WorkspaceEntryStatus.OperationError; data: Workspace; store: WorkspaceStore; error: string }

export type SessionEntry = { store: StoreApi<SessionState> }

export interface AppRegistryApi {
  get: (id: string) => Application | undefined
  getDefaultApp: (appId?: string) => Application | null
}

export interface SessionDeps {
  git: GitApi
  filesystem: FilesystemApi
  exec: ExecApi
  runActions: RunActionsApi
  sessionApi: SessionApi
  terminal: TerminalApi
  github: GitHubApi
  getSettings: () => Settings
  appRegistry: AppRegistryApi
  llm: LlmApi
  setActivityTabState: (tabId: string, state: ActivityState) => void
}

export interface SessionState {
  sessionId: string

  // Connection for this session (local or remote, transitions: connecting → connected/error)
  connection: ConnectionInfo

  createTty: (cwd: string, sandbox?: SandboxConfig, startupCommand?: string) => Promise<string>
  openTtyStream: (ptyId: string, onEvent: (event: PtyEvent) => void) => Promise<{ tty: Tty }>
  killTty: (ptyId: string) => void
  listTty: () => Promise<TTYSessionInfo[]>

  // Workspace collection
  workspaces: Map<string, WorkspaceEntry>
  activeWorkspaceId: string | null
  isRestoring: boolean
  sessionVersion: number
  sessionLock: SessionLock | null

  clearWorkspaceError: (id: string) => void
  dismissWorkspace: (id: string) => void
  /** Reactive cleanup when a workspace is no longer in the daemon session.
   *  Disposes all tab refs (renderer-side), git controller, and removes from map. */
  onWorkspaceRemoved: (id: string) => void
  addWorkspace: (path: string, options?: { skipDefaultTabs?: boolean; settings?: WorktreeSettings }) => string
  addChildWorkspace: (parentId: string, name: string, isDetached?: boolean, settings?: WorktreeSettings, description?: string) => { success: boolean; error?: string }
  adoptExistingWorktree: (parentId: string, worktreePath: string, branch: string, name: string, settings?: WorktreeSettings, description?: string) => Promise<{ success: boolean; error?: string }>
  createWorktreeFromBranch: (parentId: string, branch: string, isDetached: boolean, settings?: WorktreeSettings, description?: string) => { success: boolean; error?: string }
  createWorktreeFromRemote: (parentId: string, remoteBranch: string, isDetached: boolean, settings?: WorktreeSettings, description?: string) => { success: boolean; error?: string }
  removeWorkspace: (id: string) => Promise<void>
  removeWorkspaceKeepBranch: (id: string) => Promise<void>
  removeWorkspaceKeepBoth: (id: string) => Promise<void>
  mergeAndRemoveWorkspace: (id: string, squash: boolean) => Promise<{ success: boolean; error?: string }>
  mergeAndKeepWorkspace: (id: string, squash: boolean) => Promise<{ success: boolean; error?: string }>
  closeAndCleanWorkspace: (id: string) => Promise<{ success: boolean; error?: string }>
  setActiveWorkspace: (id: string | null) => void
  updateGitInfo: (id: string, gitInfo: GitInfo) => void
  refreshGitInfo: (id: string) => Promise<void>
  quickForkWorkspace: (workspaceId: string) => Promise<{ success: boolean; error?: string }>
  reorderWorkspace: (workspaceId: string, targetWorkspaceId: string, position: 'before' | 'after') => void
  moveWorkspace: (workspaceId: string, targetWorkspaceId: string, position: 'before' | 'after' | 'onto') => void
  syncToDaemon: () => Promise<void>
  forceUnlock: () => Promise<{ success: boolean; error?: string }>

  // Session lifecycle
  handleRestore: (session: Session) => Promise<void>
  handleExternalUpdate: (session: Session) => Promise<void>
}

function generateId(): string {
  return `ws-${String(Date.now())}-${Math.random().toString(36).slice(2, 9)}`
}

function generateTabId(): string {
  return `tab-${String(Date.now())}-${Math.random().toString(36).slice(2, 9)}`
}

function getNameFromPath(path: string): string {
  return path.split('/').pop() || path
}

function getDefaultAppForWorktree(
  deps: SessionDeps,
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
export function getUnmergedSubWorkspaces(workspaces: Map<string, WorkspaceEntry>): Workspace[] {
  return Array.from(workspaces.values())
    .filter((e): e is Extract<WorkspaceEntry, { status: WorkspaceEntryStatus.Loaded | WorkspaceEntryStatus.OperationError }> =>
      e.status === WorkspaceEntryStatus.Loaded || e.status === WorkspaceEntryStatus.OperationError)
    .map(e => e.data)
    .filter(ws => ws.isWorktree && ws.status === 'active')
}

export function createSessionStore(
  config: { sessionId: string; connection: ConnectionInfo },
  deps: SessionDeps
): StoreApi<SessionState> {
  let syncDebounceTimer: ReturnType<typeof setTimeout> | null = null

  // Acquire session lock. Surfaces the real error on failure instead of a generic message.
  async function acquireLock(): Promise<{ acquired: true } | { acquired: false; error: string }> {
    const lockResult = await deps.sessionApi.lock(store.getState().connection.id, 60_000)
    if (lockResult.success && lockResult.acquired) return { acquired: true }
    if (!lockResult.success) return { acquired: false, error: lockResult.error }
    return { acquired: false, error: 'Session is locked by another window' }
  }

  function nextSortOrder(parentId: string | null): string {
    const workspaces = store.getState().workspaces
    let max = -1
    for (const entry of Array.from(workspaces.values())) {
      if (entry.status !== WorkspaceEntryStatus.Loaded && entry.status !== WorkspaceEntryStatus.OperationError) continue
      const ws = entry.data
      const isMatch = parentId === null ? !ws.parentId : ws.parentId === parentId
      if (isMatch) {
        const order = parseInt(ws.metadata.sortOrder || '0')
        if (order > max) max = order
      }
    }
    return String(max + 1)
  }

  function reindexSiblings(parentId: string | null, excludeId: string): void {
    const workspaces = store.getState().workspaces
    const siblings: { id: string; entry: Extract<WorkspaceEntry, { status: WorkspaceEntryStatus.Loaded | WorkspaceEntryStatus.OperationError }> }[] = []
    for (const [id, entry] of Array.from(workspaces.entries())) {
      if (id === excludeId) continue
      if (entry.status !== WorkspaceEntryStatus.Loaded && entry.status !== WorkspaceEntryStatus.OperationError) continue
      const isMatch = parentId === null ? !entry.data.parentId : entry.data.parentId === parentId
      if (isMatch) siblings.push({ id, entry })
    }
    siblings.sort((a, b) => parseInt(a.entry.data.metadata.sortOrder || '0') - parseInt(b.entry.data.metadata.sortOrder || '0'))
    for (let i = 0; i < siblings.length; i++) {
      const item = siblings[i]
      if (!item) continue
      const { id, entry } = item
      const newMetadata = { ...entry.data.metadata, sortOrder: String(i) }
      entry.store.setState(s => ({
        workspace: { ...s.workspace, metadata: newMetadata }
      }))
      store.setState(s => ({
        workspaces: new Map(s.workspaces).set(id, { ...entry, data: { ...entry.data, metadata: newMetadata } })
      }))
    }
  }

  async function syncSessionToDaemon(isRestoring: boolean = false): Promise<void> {
    try {
      const { workspaces, connection } = store.getState()
      console.log('[session] syncSessionToDaemon called - workspaces:', workspaces.size, 'isRestoring:', isRestoring)

      if (connection.status !== ConnectionStatus.Connected) {
        console.log('[session] connection not yet established, skipping sync')
        return
      }

      if (isRestoring) {
        console.log('[session] currently restoring, skipping sync')
        return
      }

      const daemonWorkspaces = Array.from(workspaces.values())
        .filter((e): e is Extract<WorkspaceEntry, { status: WorkspaceEntryStatus.Loaded }> => e.status === WorkspaceEntryStatus.Loaded)
        // eslint-disable-next-line @typescript-eslint/no-unused-vars
        .map(e => { const { createdAt: _createdAt, lastActivity: _lastActivity, ...ws } = e.data; return ws })

      console.log('[session] syncing to daemon:', daemonWorkspaces.length, 'workspaces', JSON.stringify(daemonWorkspaces))

      const currentVersion = store.getState().sessionVersion
      const connectionId = store.getState().connection.id
      console.log('[session] updating session via connection:', connectionId, 'expectedVersion:', currentVersion)
      const result = await deps.sessionApi.update(connectionId, daemonWorkspaces, connectionId, currentVersion)
      if (!result.success) {
        console.error('[session] failed to update session:', result.error)
      } else {
        if (result.session.version === currentVersion + 1) {
          // Update accepted
          store.setState({ sessionVersion: result.session.version, sessionLock: result.session.lock })
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

  let pendingSyncSnapshot: Record<string, string | null> | null = null

  function debouncedSyncToDaemon(): void {
    const currentSnapshot = getActiveTabSnapshot()
    if (syncDebounceTimer) {
      clearTimeout(syncDebounceTimer)
      console.log('[session] debounce: skipped state:', JSON.stringify(pendingSyncSnapshot), '-> new state:', JSON.stringify(currentSnapshot))
    }
    pendingSyncSnapshot = currentSnapshot
    syncDebounceTimer = setTimeout(() => {
      pendingSyncSnapshot = null
      void syncSessionToDaemon(store.getState().isRestoring)
    }, 100)
  }

  function getActiveTabSnapshot(): Record<string, string | null> {
    const snapshot: Record<string, string | null> = {}
    store.getState().workspaces.forEach((entry, id) => {
      if (entry.status === WorkspaceEntryStatus.Loaded || entry.status === WorkspaceEntryStatus.OperationError) {
        snapshot[id] = entry.store.getState().workspace.activeTabId
      }
    })
    return snapshot
  }

  function makeHandleDeps(): WorkspaceStoreDeps {
    return {
      appRegistry: deps.appRegistry,
      openTtyStream: (ptyId: string, onEvent: (event: PtyEvent) => void) => store.getState().openTtyStream(ptyId, onEvent),
      createTty: (cwd, sandbox?, startupCommand?) => store.getState().createTty(cwd, sandbox, startupCommand),
      connectionId: config.connection.id,
      git: deps.git,
      filesystem: deps.filesystem,
      exec: deps.exec,
      runActions: deps.runActions,
      getSettings: deps.getSettings,
      llm: deps.llm,
      setActivityTabState: deps.setActivityTabState,
      syncToDaemon: () => { debouncedSyncToDaemon(); },
      removeWorkspace: (id) => store.getState().removeWorkspace(id),
      removeWorkspaceKeepBranch: (id) => store.getState().removeWorkspaceKeepBranch(id),
      removeWorkspaceKeepBoth: (id) => store.getState().removeWorkspaceKeepBoth(id),
      mergeAndRemoveWorkspace: (id, squash) => store.getState().mergeAndRemoveWorkspace(id, squash),
      mergeAndKeepWorkspace: (id, squash) => store.getState().mergeAndKeepWorkspace(id, squash),
      closeAndCleanWorkspace: (id) => store.getState().closeAndCleanWorkspace(id),
      quickForkWorkspace: (id) => store.getState().quickForkWorkspace(id),
      refreshGitInfo: (id) => store.getState().refreshGitInfo(id),
      lookupWorkspace: (id) => {
        const entry = store.getState().workspaces.get(id)
        return entry && (entry.status === WorkspaceEntryStatus.Loaded || entry.status === WorkspaceEntryStatus.OperationError) ? entry.data : undefined
      },
      github: deps.github,
      getActiveWorkspaceId: () => store.getState().activeWorkspaceId,
    }
  }

  function createHandleForWorkspace(workspace: Workspace): WorkspaceStore {
    const handle = createWorkspaceStore(workspace, makeHandleDeps())

    // Keep the workspaces snapshot in sync when handle state changes
    handle.subscribe((state) => {
      store.setState((s) => {
        const entry = s.workspaces.get(state.workspace.id)
        if (!entry || (entry.status !== WorkspaceEntryStatus.Loaded && entry.status !== WorkspaceEntryStatus.OperationError)) return s
        return {
          workspaces: new Map(s.workspaces).set(state.workspace.id, { ...entry, data: state.workspace })
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
      gitOperation: (onProgress: (data: string) => void) => Promise<{ success: boolean; path?: string; branch?: string; error?: string }>
      preOperation?: () => Promise<void>
    }
  ): { success: true } {
    const state = store.getState()
    const parentEntry = state.workspaces.get(parentId)
    const parent = parentEntry && (parentEntry.status === WorkspaceEntryStatus.Loaded || parentEntry.status === WorkspaceEntryStatus.OperationError) ? parentEntry.data : undefined

    const id = generateId()

    store.setState((s) => ({
      workspaces: new Map(s.workspaces).set(id, { status: WorkspaceEntryStatus.Loading, name: worktreeName, message: options.message, output: [] }),
      activeWorkspaceId: id,
    }))

    const onProgress = (data: string): void => {
      const entry = store.getState().workspaces.get(id)
      if (entry?.status === WorkspaceEntryStatus.Loading) {
        store.setState(s => ({
          workspaces: new Map(s.workspaces).set(id, { ...entry, output: [...entry.output, data] })
        }))
      }
    }

    void (async () => {
      const lockStatus = await acquireLock()
      if (!lockStatus.acquired) {
        store.setState(s => ({
          workspaces: new Map(s.workspaces).set(id, { status: WorkspaceEntryStatus.Error, name: worktreeName, error: lockStatus.error })
        }))
        return
      }
      try {
        if (options.preOperation) {
          await options.preOperation()
        }

        const result = await options.gitOperation(onProgress)

        if (!result.success) {
          store.setState(s => ({
            workspaces: new Map(s.workspaces).set(id, { status: WorkspaceEntryStatus.Error, name: worktreeName, error: result.error || 'Operation failed' })
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
          path: result.path ?? '',
          parentId,
          status: 'active',
          isGitRepo: true,
          gitBranch: result.branch ?? '',
          gitRootPath: parent?.gitRootPath ?? null,
          isWorktree: true,
          isDetached: options.isDetached ?? false,
          appStates,
          activeTabId,
          settings: options.settings ?? { defaultApplicationId: '' },
          metadata: {
            sortOrder: nextSortOrder(parentId),
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
          workspaces: new Map(s.workspaces).set(id, { status: WorkspaceEntryStatus.Loaded, data: childWorkspace, store: handle })
        }))
        await syncSessionToDaemon(store.getState().isRestoring)
      } catch (err) {
        store.setState(s => ({
          workspaces: new Map(s.workspaces).set(id, { status: WorkspaceEntryStatus.Error, name: worktreeName, error: err instanceof Error ? err.message : String(err) })
        }))
      } finally {
        await deps.sessionApi.unlock(store.getState().connection.id).catch((e: unknown) => { console.error('[session] failed to unlock session:', e) })
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
    const parentEntry = store.getState().workspaces.get(parentId)
    const parent = parentEntry && (parentEntry.status === WorkspaceEntryStatus.Loaded || parentEntry.status === WorkspaceEntryStatus.OperationError) ? parentEntry.data : undefined

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
      metadata: { sortOrder: nextSortOrder(parentId), ...(options.metadata ?? {}) },
      createdAt: Date.now(),
      lastActivity: Date.now(),
    }

    const handle = createHandleForWorkspace(childWorkspace)

    store.setState((s) => ({
      workspaces: new Map(s.workspaces).set(id, { status: WorkspaceEntryStatus.Loaded, data: childWorkspace, store: handle }),
      activeWorkspaceId: id
    }))

    for (const tabId of Object.keys(appStates)) {
      handle.getState().initTab(tabId)
    }

    await syncSessionToDaemon(store.getState().isRestoring)
    return id
  }

  /** Destructive: removes workspace from daemon (kills PTYs, deletes worktree/branch).
   *  Renderer cleanup happens via onWorkspaceRemoved — do not call ref.dispose() here. */
  async function removeWorkspaceInternal(
    id: string,
    options: { keepBranch: boolean; keepWorktree: boolean; onProgress?: (data: string) => void }
  ): Promise<void> {
    const entry = store.getState().workspaces.get(id)
    if (!entry) return
    const workspace = (entry.status === WorkspaceEntryStatus.Loaded || entry.status === WorkspaceEntryStatus.OperationError) ? entry.data : undefined
    const handle = (entry.status === WorkspaceEntryStatus.Loaded || entry.status === WorkspaceEntryStatus.OperationError) ? entry.store : undefined

    // Recursively remove children first (derived from parentId)
    const childIds = Array.from(store.getState().workspaces.entries())
      .filter(([, e]) => (e.status === WorkspaceEntryStatus.Loaded || e.status === WorkspaceEntryStatus.OperationError) && e.data.parentId === id)
      .map(([childId]) => childId)
    for (const childId of childIds) {
      await removeWorkspaceInternal(childId, options)
    }

    // Kill daemon-side resources (PTYs) — renderer cleanup deferred to onWorkspaceRemoved
    if (handle && workspace) {
      for (const tabId of Object.keys(workspace.appStates)) {
        const ref = handle.getState().getTabRef(tabId)
        if (ref) ref.close()
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
          options.onProgress
        )
      } else if (!options.keepBranch && !workspace.isDetached && workspace.gitBranch) {
        await deps.git.deleteBranch(workspace.gitRootPath, workspace.gitBranch, options.onProgress)
      }
    }

    // Renderer cleanup + remove from map
    store.getState().onWorkspaceRemoved(id)

    await syncSessionToDaemon(store.getState().isRestoring)
  }

  // Helper: wraps removeWorkspaceInternal with loading state and session lock
  async function removeWorkspaceWithLoading(
    id: string,
    options: { keepBranch: boolean; keepWorktree: boolean }
  ): Promise<void> {
    const entry = store.getState().workspaces.get(id)
    if (!entry || (entry.status !== WorkspaceEntryStatus.Loaded && entry.status !== WorkspaceEntryStatus.OperationError)) return
    const { data, store: wsStore } = entry

    const lockStatus = await acquireLock()
    if (!lockStatus.acquired) {
      store.setState(s => ({
        workspaces: new Map(s.workspaces).set(id, { status: WorkspaceEntryStatus.OperationError, data, store: wsStore, error: lockStatus.error })
      }))
      return
    }

    // Temporarily show loading in the main pane — preserve data+store for recovery
    store.setState(s => ({
      workspaces: new Map(s.workspaces).set(id, { status: WorkspaceEntryStatus.Loaded, data, store: wsStore })
    }))
    try {
      await removeWorkspaceInternal(id, options)
    } catch (err) {
      store.setState(s => ({
        workspaces: new Map(s.workspaces).set(id, { status: WorkspaceEntryStatus.OperationError, data, store: wsStore, error: err instanceof Error ? err.message : String(err) })
      }))
    } finally {
      await deps.sessionApi.unlock(store.getState().connection.id).catch((e: unknown) => { console.error('[session] failed to unlock session:', e) })
    }
  }

  // Shared helper: validates workspace, sets loading state, auto-commits, and performs git merge.
  // Shared helper: validates workspace, sets loading state, auto-commits, and performs git merge.
  // Returns { success, error } — caller decides post-merge behavior.
  async function mergeWorkspaceCore(
    id: string,
    squash: boolean
  ): Promise<{ success: boolean; error?: string }> {

    const entry = store.getState().workspaces.get(id)
    if (!entry || (entry.status !== WorkspaceEntryStatus.Loaded && entry.status !== WorkspaceEntryStatus.OperationError)) {
      return { success: false, error: 'Workspace not found' }
    }
    const { data: workspace, store: wsStore } = entry

    if (!workspace.isWorktree || !workspace.parentId) {
      return { success: false, error: 'Not a worktree workspace' }
    }

    const parentEntry = store.getState().workspaces.get(workspace.parentId)
    const parent = parentEntry && (parentEntry.status === WorkspaceEntryStatus.Loaded || parentEntry.status === WorkspaceEntryStatus.OperationError) ? parentEntry.data : undefined
    if (!parent || !parent.gitRootPath || !parent.gitBranch) {
      return { success: false, error: 'Parent workspace not found or not a git repo' }
    }

    try {
      // Block merge if parent worktree has uncommitted changes
      const parentHasChanges = await deps.git.hasUncommittedChanges(parent.path)
      if (parentHasChanges) {
        store.setState(s => ({
          workspaces: new Map(s.workspaces).set(id, { status: WorkspaceEntryStatus.OperationError, data: workspace, store: wsStore, error: 'Parent workspace has uncommitted changes. Commit or stash them before merging.' })
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
            workspaces: new Map(s.workspaces).set(id, { status: WorkspaceEntryStatus.OperationError, data: workspace, store: wsStore, error: `Failed to commit changes: ${commitResult.error}` })
          }))
          return { success: false, error: `Failed to commit changes: ${commitResult.error}` }
        }
      }

      const mergeResult = await deps.git.merge(
        parent.path,
        workspace.gitBranch ?? '',
        squash
      )

      if (!mergeResult.success) {
        store.setState(s => ({
          workspaces: new Map(s.workspaces).set(id, { status: WorkspaceEntryStatus.OperationError, data: workspace, store: wsStore, error: `Merge failed: ${mergeResult.error}` })
        }))
        return { success: false, error: `Merge failed: ${mergeResult.error}` }
      }

      return { success: true }
    } catch (err) {
      store.setState(s => ({
        workspaces: new Map(s.workspaces).set(id, { status: WorkspaceEntryStatus.OperationError, data: workspace, store: wsStore, error: err instanceof Error ? err.message : String(err) })
      }))
      return { success: false, error: err instanceof Error ? err.message : String(err) }
    }
  }

  const connectionId = config.connection.id

  // Create a terminal wrapper with connectionId bound for tty stores
  const boundTerminal: TtyTerminalDeps = {
    write: deps.terminal.write,
    resize: deps.terminal.resize,
    kill: (sessionId: string) => { deps.terminal.kill(connectionId, sessionId); },
  }

  const store = createStore<SessionState>()((set, get) => ({
    sessionId: config.sessionId,
    workspaces: new Map<string, WorkspaceEntry>(),
    activeWorkspaceId: null,
    isRestoring: false,
    sessionVersion: 0,
    sessionLock: null,

    connection: config.connection,

    createTty: async (cwd: string, sandbox?: SandboxConfig, startupCommand?: string): Promise<string> => {
      const handle = crypto.randomUUID()
      const result = await deps.terminal.create(connectionId, handle, cwd, sandbox, startupCommand)
      if (!result.success) {
        throw new Error(result.error || 'Failed to create PTY')
      }
      return result.sessionId
    },

    openTtyStream: async (ptyId: string, onEvent: (event: PtyEvent) => void): Promise<{ tty: Tty }> => {
      const handle = crypto.randomUUID()
      const unsubscribe = deps.terminal.onEvent(handle, onEvent)
      const result = await deps.terminal.attach(connectionId, handle, ptyId)
      if (!result.success) {
        unsubscribe()
        throw new Error(result.error || 'Failed to attach to PTY')
      }
      const tty = createTtyStore(ptyId, handle, boundTerminal)
      return { tty }
    },

    killTty: (ptyId: string): void => {
      boundTerminal.kill(ptyId)
    },

    listTty: (): Promise<TTYSessionInfo[]> => {
      return deps.terminal.list(connectionId)
    },

    clearWorkspaceError: (id: string): void => {
      const entry = get().workspaces.get(id)
      if (!entry || entry.status !== WorkspaceEntryStatus.OperationError) return
      set((s) => ({
        workspaces: new Map(s.workspaces).set(id, { status: WorkspaceEntryStatus.Loaded, data: entry.data, store: entry.store })
      }))
    },

    dismissWorkspace: (id: string): void => {
      const entry = get().workspaces.get(id)
      if (!entry || (entry.status !== WorkspaceEntryStatus.Error && entry.status !== WorkspaceEntryStatus.Loading)) return
      set((s) => {
        const remaining = new Map(s.workspaces)
        remaining.delete(id)
        return {
          workspaces: remaining,
          activeWorkspaceId: s.activeWorkspaceId === id ? null : s.activeWorkspaceId
        }
      })
    },

    onWorkspaceRemoved: (id: string): void => {
      const entry = get().workspaces.get(id)
      if (!entry) return
      if (entry.status === WorkspaceEntryStatus.Loaded || entry.status === WorkspaceEntryStatus.OperationError) {
        entry.store.getState().gitController.getState().dispose()
        for (const tabId of Object.keys(entry.data.appStates)) {
          const ref = entry.store.getState().getTabRef(tabId)
          if (ref) ref.dispose()
        }
      }
      set((s) => {
        const remaining = new Map(s.workspaces)
        remaining.delete(id)
        return {
          workspaces: remaining,
          activeWorkspaceId: s.activeWorkspaceId === id ? null : s.activeWorkspaceId
        }
      })
    },

    addWorkspace: (path: string, options?: { skipDefaultTabs?: boolean; settings?: WorktreeSettings }) => {
      console.log('[session] addWorkspace called for path:', path)
      const id = generateId()
      const name = getNameFromPath(path)

      set((s) => ({
        workspaces: new Map(s.workspaces).set(id, { status: WorkspaceEntryStatus.Loading, name, message: 'Loading workspace...', output: [] }),
        activeWorkspaceId: id,
      }))

      // Fire-and-forget: resolve git info then create workspace+handle
      void deps.git.getInfo(path).then(gitInfo => {
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
          metadata: { sortOrder: nextSortOrder(null) },
          createdAt: Date.now(),
          lastActivity: Date.now(),
        }

        const handle = createHandleForWorkspace(workspace)
        for (const tabId of Object.keys(appStates)) {
          handle.getState().initTab(tabId)
        }

        set(s => ({
          workspaces: new Map(s.workspaces).set(id, { status: WorkspaceEntryStatus.Loaded, data: workspace, store: handle })
        }))
        void syncSessionToDaemon(get().isRestoring)
      }).catch((err: unknown) => {
        set(s => ({
          workspaces: new Map(s.workspaces).set(id, { status: WorkspaceEntryStatus.Error, name, error: err instanceof Error ? err.message : String(err) })
        }))
      })

      return id
    },

    addChildWorkspace: (parentId: string, name: string, isDetached: boolean = false, settings?: WorktreeSettings, description?: string) => {
      const parentEntry = get().workspaces.get(parentId)
      const parent = parentEntry && (parentEntry.status === WorkspaceEntryStatus.Loaded || parentEntry.status === WorkspaceEntryStatus.OperationError) ? parentEntry.data : undefined

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
        gitOperation: (onProgress) => {
          const currentParentEntry = get().workspaces.get(parentId)
          const currentParent = currentParentEntry && (currentParentEntry.status === WorkspaceEntryStatus.Loaded || currentParentEntry.status === WorkspaceEntryStatus.OperationError) ? currentParentEntry.data : undefined
          return deps.git.createWorktree(
            parent.gitRootPath ?? '',
            name,
            currentParent?.gitBranch ?? undefined,
            onProgress
          )
        },
      })
    },

    adoptExistingWorktree: async (parentId: string, worktreePath: string, branch: string, name: string, settings?: WorktreeSettings, description?: string) => {
      const parentEntry = get().workspaces.get(parentId)
      if (!parentEntry || (parentEntry.status !== WorkspaceEntryStatus.Loaded && parentEntry.status !== WorkspaceEntryStatus.OperationError)) {
        return { success: false, error: 'Parent workspace not found' }
      }

      const alreadyOpen = Array.from(get().workspaces.values()).some(
        e => (e.status === WorkspaceEntryStatus.Loaded || e.status === WorkspaceEntryStatus.OperationError) && e.data.path === worktreePath
      )
      if (alreadyOpen) {
        return { success: false, error: 'This worktree is already open' }
      }

      const lockStatus = await acquireLock()
      if (!lockStatus.acquired) {
        return { success: false, error: lockStatus.error }
      }

      try {
        const metadata: Record<string, string> = { branchIsUserDefined: 'true', ...(description ? { description } : {}) }
        await addChildWorkspaceFromResult(parentId, name, worktreePath, branch, { settings, metadata })
        return { success: true }
      } finally {
        await deps.sessionApi.unlock(store.getState().connection.id).catch((e: unknown) => { console.error('[session] failed to unlock session:', e) })
      }
    },

    createWorktreeFromBranch: (parentId: string, branch: string, isDetached: boolean, settings?: WorktreeSettings, description?: string) => {
      console.log('[session] createWorktreeFromBranch called:', { parentId, branch, isDetached })
      const parentEntry = get().workspaces.get(parentId)
      const parent = parentEntry && (parentEntry.status === WorkspaceEntryStatus.Loaded || parentEntry.status === WorkspaceEntryStatus.OperationError) ? parentEntry.data : undefined

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
        gitOperation: (onProgress) => deps.git.createWorktreeFromBranch(
          parent.gitRootPath ?? '',
          branch,
          worktreeName,
          onProgress
        ),
      })
    },

    createWorktreeFromRemote: (parentId: string, remoteBranch: string, isDetached: boolean, settings?: WorktreeSettings, description?: string) => {
      console.log('[session] createWorktreeFromRemote called:', { parentId, remoteBranch, isDetached })
      const parentEntry = get().workspaces.get(parentId)
      const parent = parentEntry && (parentEntry.status === WorkspaceEntryStatus.Loaded || parentEntry.status === WorkspaceEntryStatus.OperationError) ? parentEntry.data : undefined

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
        gitOperation: (onProgress) => deps.git.createWorktreeFromRemote(
          parent.gitRootPath ?? '',
          remoteBranch,
          worktreeName,
          onProgress
        ),
      })
    },

    removeWorkspace: (id: string) =>
      removeWorkspaceWithLoading(id, { keepBranch: false, keepWorktree: false }),

    removeWorkspaceKeepBranch: (id: string) =>
      removeWorkspaceWithLoading(id, { keepBranch: true, keepWorktree: false }),

    removeWorkspaceKeepBoth: (id: string) =>
      removeWorkspaceWithLoading(id, { keepBranch: true, keepWorktree: true }),

    setActiveWorkspace: (id: string | null) => {
      set({ activeWorkspaceId: id })
      if (id) {
        const entry = get().workspaces.get(id)
        if (entry && (entry.status === WorkspaceEntryStatus.Loaded || entry.status === WorkspaceEntryStatus.OperationError)) {
          entry.store.getState().gitController.getState().triggerRefresh()
        }
      }
    },

    updateGitInfo: (id: string, gitInfo: GitInfo) => {
      const entry = get().workspaces.get(id)
      if (!entry || (entry.status !== WorkspaceEntryStatus.Loaded && entry.status !== WorkspaceEntryStatus.OperationError)) return
      entry.store.setState(s => ({
        workspace: {
          ...s.workspace,
          isGitRepo: gitInfo.isRepo,
          gitBranch: gitInfo.isRepo ? gitInfo.branch : null,
          gitRootPath: gitInfo.isRepo ? gitInfo.rootPath : null
        }
      }))
      void syncSessionToDaemon(get().isRestoring).catch((e: unknown) => { console.error(e) })
    },

    refreshGitInfo: async (id: string) => {
      const entry = get().workspaces.get(id)
      if (!entry || (entry.status !== WorkspaceEntryStatus.Loaded && entry.status !== WorkspaceEntryStatus.OperationError)) return
      const gitInfo = await deps.git.getInfo(entry.data.path)
      get().updateGitInfo(id, gitInfo)
    },

    mergeAndRemoveWorkspace: async (id: string, squash: boolean) => {
      // Acquire session lock before merge (slow IO operation)
      const lockStatus = await acquireLock()
      if (!lockStatus.acquired) {
        return { success: false, error: lockStatus.error }
      }

      try {
        const result = await mergeWorkspaceCore(id, squash)
        if (!result.success) return result

        const entry = get().workspaces.get(id)
        if (entry && (entry.status === WorkspaceEntryStatus.Loaded || entry.status === WorkspaceEntryStatus.OperationError)) {
          entry.store.getState().updateStatus('merged')
        }

        try {
          await removeWorkspaceInternal(id, { keepBranch: false, keepWorktree: false })
        } catch (err) {
          // Merge succeeded but removal failed — show operation error
          const currentEntry = get().workspaces.get(id)
          if (currentEntry && (currentEntry.status === WorkspaceEntryStatus.Loaded || currentEntry.status === WorkspaceEntryStatus.OperationError)) {
            store.setState(s => ({
              workspaces: new Map(s.workspaces).set(id, { status: WorkspaceEntryStatus.OperationError, data: currentEntry.data, store: currentEntry.store, error: `Merge succeeded but cleanup failed: ${err instanceof Error ? err.message : String(err)}` })
            }))
          }
          return { success: false, error: err instanceof Error ? err.message : String(err) }
        }

        // Refresh parent's remote status after merge
        const wsData = entry && (entry.status === WorkspaceEntryStatus.Loaded || entry.status === WorkspaceEntryStatus.OperationError) ? entry.data : undefined
        if (wsData?.parentId) {
          const parentEntry = get().workspaces.get(wsData.parentId)
          if (parentEntry && (parentEntry.status === WorkspaceEntryStatus.Loaded || parentEntry.status === WorkspaceEntryStatus.OperationError)) {
            void parentEntry.store.getState().gitController.getState().refreshRemoteStatus()
          }
        }

        return { success: true }
      } finally {
        await deps.sessionApi.unlock(store.getState().connection.id).catch((e: unknown) => { console.error('[session] failed to unlock session:', e) })
      }
    },

    mergeAndKeepWorkspace: async (id: string, squash: boolean) => {
      // Acquire session lock before merge (slow IO operation)
      const lockStatus = await acquireLock()
      if (!lockStatus.acquired) {
        return { success: false, error: lockStatus.error }
      }

      try {
        const result = await mergeWorkspaceCore(id, squash)
        if (!result.success) return result

        // On success, ensure workspace is back to loaded status
        const entry = get().workspaces.get(id)
        if (entry && entry.status === WorkspaceEntryStatus.OperationError) {
          store.setState(s => ({
            workspaces: new Map(s.workspaces).set(id, { status: WorkspaceEntryStatus.Loaded, data: entry.data, store: entry.store })
          }))
        }

        // Refresh workspace diff status and git info
        const currentEntry = get().workspaces.get(id)
        if (currentEntry && currentEntry.status === WorkspaceEntryStatus.Loaded) {
          void currentEntry.store.getState().gitController.getState().refreshDiffStatus()
        }
        void get().refreshGitInfo(id)

        // Refresh parent's remote status after merge
        if (currentEntry && currentEntry.status === WorkspaceEntryStatus.Loaded && currentEntry.data.parentId) {
          const parentEntry = get().workspaces.get(currentEntry.data.parentId)
          if (parentEntry && (parentEntry.status === WorkspaceEntryStatus.Loaded || parentEntry.status === WorkspaceEntryStatus.OperationError)) {
            void parentEntry.store.getState().gitController.getState().refreshRemoteStatus()
          }
        }

        return { success: true }
      } finally {
        await deps.sessionApi.unlock(store.getState().connection.id).catch((e: unknown) => { console.error('[session] failed to unlock session:', e) })
      }
    },

    closeAndCleanWorkspace: async (id: string) => {
      const entry = get().workspaces.get(id)
      if (!entry || (entry.status !== WorkspaceEntryStatus.Loaded && entry.status !== WorkspaceEntryStatus.OperationError)) {
        return { success: false, error: 'Workspace not found' }
      }
      const workspace = entry.data

      if (!workspace.isWorktree || !workspace.parentId) {
        return { success: false, error: 'Not a worktree workspace' }
      }

      const parentEntry = get().workspaces.get(workspace.parentId)
      if (!parentEntry || (parentEntry.status !== WorkspaceEntryStatus.Loaded && parentEntry.status !== WorkspaceEntryStatus.OperationError) || !parentEntry.data.gitRootPath) {
        return { success: false, error: 'Parent workspace not found or not a git repo' }
      }

      await get().removeWorkspace(id)
      return { success: true }
    },

    quickForkWorkspace: async (workspaceId: string) => {
      const entry = get().workspaces.get(workspaceId)
      if (!entry || (entry.status !== WorkspaceEntryStatus.Loaded && entry.status !== WorkspaceEntryStatus.OperationError)) {
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

    reorderWorkspace: (workspaceId: string, targetWorkspaceId: string, position: 'before' | 'after') => {
      const workspaces = get().workspaces
      const dragEntry = workspaces.get(workspaceId)
      const targetEntry = workspaces.get(targetWorkspaceId)
      if (!dragEntry || !targetEntry) return
      if (dragEntry.status !== WorkspaceEntryStatus.Loaded && dragEntry.status !== WorkspaceEntryStatus.OperationError) return
      if (targetEntry.status !== WorkspaceEntryStatus.Loaded && targetEntry.status !== WorkspaceEntryStatus.OperationError) return
      if (workspaceId === targetWorkspaceId) return

      const dragParent = dragEntry.data.parentId
      const targetParent = targetEntry.data.parentId
      if (dragParent !== targetParent) return

      // Gather siblings sorted by current sortOrder
      const siblings: { id: string; entry: Extract<WorkspaceEntry, { status: WorkspaceEntryStatus.Loaded | WorkspaceEntryStatus.OperationError }> }[] = []
      for (const [id, entry] of Array.from(workspaces.entries())) {
        if (entry.status !== WorkspaceEntryStatus.Loaded && entry.status !== WorkspaceEntryStatus.OperationError) continue
        const isMatch = dragParent === null ? !entry.data.parentId : entry.data.parentId === dragParent
        if (isMatch) siblings.push({ id, entry })
      }
      siblings.sort((a, b) =>
        parseInt(a.entry.data.metadata.sortOrder || '0') - parseInt(b.entry.data.metadata.sortOrder || '0')
      )

      // Remove dragged, insert at target position
      const ordered = siblings.filter(s => s.id !== workspaceId)
      const targetIdx = ordered.findIndex(s => s.id === targetWorkspaceId)
      const insertIdx = position === 'before' ? targetIdx : targetIdx + 1
      const dragSibling = siblings.find(s => s.id === workspaceId)
      if (dragSibling) ordered.splice(insertIdx, 0, dragSibling)

      // Reassign sortOrder on all siblings
      for (let i = 0; i < ordered.length; i++) {
        const item = ordered[i]
        if (!item) continue
        const { id, entry } = item
        const newMetadata = { ...entry.data.metadata, sortOrder: String(i) }
        entry.store.setState(s => ({
          workspace: { ...s.workspace, metadata: newMetadata }
        }))
        // Also update the session store snapshot
        set(s => ({
          workspaces: new Map(s.workspaces).set(id, { ...entry, data: { ...entry.data, metadata: newMetadata } })
        }))
      }

      debouncedSyncToDaemon()
    },

    moveWorkspace: (workspaceId: string, targetWorkspaceId: string, position: 'before' | 'after' | 'onto') => {
      const workspaces = get().workspaces
      const dragEntry = workspaces.get(workspaceId)
      const targetEntry = workspaces.get(targetWorkspaceId)
      if (!dragEntry || !targetEntry) return
      if (dragEntry.status !== WorkspaceEntryStatus.Loaded && dragEntry.status !== WorkspaceEntryStatus.OperationError) return
      if (targetEntry.status !== WorkspaceEntryStatus.Loaded && targetEntry.status !== WorkspaceEntryStatus.OperationError) return
      if (workspaceId === targetWorkspaceId) return

      const dragParent = dragEntry.data.parentId

      // Cycle check: walk up from target to ensure dragged item is not an ancestor
      const checkAncestor = (startId: string): boolean => {
        let current: string | null = startId
        while (current) {
          if (current === workspaceId) return true
          const entry = workspaces.get(current)
          if (!entry || (entry.status !== WorkspaceEntryStatus.Loaded && entry.status !== WorkspaceEntryStatus.OperationError)) break
          current = entry.data.parentId
        }
        return false
      }

      if (position === 'onto') {
        // Reparent: make dragged item a child of target
        if (checkAncestor(targetWorkspaceId)) return

        const newSortOrder = nextSortOrder(targetWorkspaceId)
        const newData = { ...dragEntry.data, parentId: targetWorkspaceId, metadata: { ...dragEntry.data.metadata, sortOrder: newSortOrder } }

        dragEntry.store.setState(s => ({
          workspace: { ...s.workspace, parentId: targetWorkspaceId, metadata: { ...s.workspace.metadata, sortOrder: newSortOrder } }
        }))
        set(s => ({
          workspaces: new Map(s.workspaces).set(workspaceId, { ...dragEntry, data: newData })
        }))

        reindexSiblings(dragParent, workspaceId)
      } else {
        // before/after: reorder among target's siblings
        const newParentId = targetEntry.data.parentId

        if (dragParent === newParentId) {
          get().reorderWorkspace(workspaceId, targetWorkspaceId, position)
          return
        }

        // Cross-parent move
        if (newParentId && checkAncestor(newParentId)) return

        // Gather new siblings (excluding the dragged item)
        const newSiblings: { id: string; entry: Extract<WorkspaceEntry, { status: WorkspaceEntryStatus.Loaded | WorkspaceEntryStatus.OperationError }> }[] = []
        for (const [id, entry] of Array.from(workspaces.entries())) {
          if (entry.status !== WorkspaceEntryStatus.Loaded && entry.status !== WorkspaceEntryStatus.OperationError) continue
          const isMatch = newParentId === null ? !entry.data.parentId : entry.data.parentId === newParentId
          if (isMatch && id !== workspaceId) newSiblings.push({ id, entry })
        }
        newSiblings.sort((a, b) => parseInt(a.entry.data.metadata.sortOrder || '0') - parseInt(b.entry.data.metadata.sortOrder || '0'))

        const targetIdx = newSiblings.findIndex(s => s.id === targetWorkspaceId)
        const insertIdx = position === 'before' ? targetIdx : targetIdx + 1
        newSiblings.splice(insertIdx, 0, { id: workspaceId, entry: dragEntry })

        // Update all new siblings' sortOrder, and parentId on the dragged item
        for (let i = 0; i < newSiblings.length; i++) {
          const item = newSiblings[i]
          if (!item) continue
          const { id, entry } = item
          const isTheDraggedItem = id === workspaceId
          const newMetadata = { ...entry.data.metadata, sortOrder: String(i) }
          const parentId = isTheDraggedItem ? newParentId : entry.data.parentId

          entry.store.setState(s => ({
            workspace: { ...s.workspace, parentId, metadata: newMetadata }
          }))
          set(s => ({
            workspaces: new Map(s.workspaces).set(id, { ...entry, data: { ...entry.data, parentId, metadata: newMetadata } })
          }))
        }

        reindexSiblings(dragParent, workspaceId)
      }

      debouncedSyncToDaemon()
    },

    syncToDaemon: async () => {
      await syncSessionToDaemon(get().isRestoring)
    },

    forceUnlock: async () => {
      const result = await deps.sessionApi.forceUnlock(store.getState().connection.id)
      if (!result.success) return { success: false, error: result.error }
      set({ sessionLock: null })
      return { success: true }
    },

    // eslint-disable-next-line @typescript-eslint/require-await -- interface requires Promise<void> but implementation is synchronous
    handleRestore: async (daemonSession: Session) => {
      console.log('[Session] Restoring session', daemonSession.id, 'with', daemonSession.workspaces.length, 'workspaces, version:', daemonSession.version)

      set({ isRestoring: true, sessionVersion: daemonSession.version, sessionLock: daemonSession.lock })
      applySessionWorkspaces(store, daemonSession.workspaces, createHandleForWorkspace, { restoreExisting: true })
      set({ isRestoring: false })

      console.log('[Session] Session restore complete, workspace count:', get().workspaces.size)
    },

    // eslint-disable-next-line @typescript-eslint/require-await -- interface requires Promise<void> but implementation is synchronous
    handleExternalUpdate: async (daemonSession: Session) => {
      const currentVersion = get().sessionVersion
      if (daemonSession.version <= currentVersion) {
        console.log('[Session] Ignoring stale external update, incoming version:', daemonSession.version, 'current:', currentVersion)
        return
      }

      console.log('[Session] External session update received, version:', daemonSession.version, 'current:', currentVersion)

      set({ isRestoring: true, sessionVersion: daemonSession.version, sessionLock: daemonSession.lock })
      applySessionWorkspaces(store, daemonSession.workspaces, createHandleForWorkspace, { restoreExisting: false })

      // Remove workspaces not present in daemon session (skip non-loaded workspaces)
      const incomingPaths = new Set(daemonSession.workspaces.map(ws => ws.path))
      const updatedState = get()
      for (const [id, entry] of Array.from(updatedState.workspaces.entries())) {
        if (entry.status === WorkspaceEntryStatus.Loaded && !incomingPaths.has(entry.data.path)) {
          get().onWorkspaceRemoved(id)
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
  const entry = store.getState().workspaces.get(existingId)
  if (!entry || (entry.status !== WorkspaceEntryStatus.Loaded && entry.status !== WorkspaceEntryStatus.OperationError)) return
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
  for (const [id, entry] of Array.from(store.getState().workspaces.entries())) {
    if ((entry.status === WorkspaceEntryStatus.Loaded || entry.status === WorkspaceEntryStatus.OperationError) && entry.data.path === path) {
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
  const entry = store.getState().workspaces.get(workspaceId)
  if (!entry || (entry.status !== WorkspaceEntryStatus.Loaded && entry.status !== WorkspaceEntryStatus.OperationError)) return

  const wsState = entry.store.getState()
  const oldTabIds = Object.keys(wsState.workspace.appStates)
  const newTabIds = Object.keys(daemonWorkspace.appStates)
  const newTabIdSet = new Set(newTabIds)

  // Dispose resources for tabs removed externally
  for (const tabId of oldTabIds) {
    if (!newTabIdSet.has(tabId)) {
      wsState.disposeTabResources(tabId)
    }
  }

  const reconciledActiveTabId = daemonWorkspace.activeTabId || newTabIds[0] || null
  console.log('[session] restoreWorkspaceTabs: activeTabId changed to', reconciledActiveTabId, 'workspace:', daemonWorkspace.id, 'session:', store.getState().sessionId)
  entry.store.setState({
    workspace: {
      ...wsState.workspace,
      appStates: daemonWorkspace.appStates,
      activeTabId: reconciledActiveTabId
    }
  })

  // Only init genuinely new tabs
  const oldTabIdSet = new Set(oldTabIds)
  for (const tabId of newTabIds) {
    if (!oldTabIdSet.has(tabId)) {
      entry.store.getState().initTab(tabId)
    }
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

  const reconstructedActiveTabId = daemonWorkspace.activeTabId || (Object.keys(daemonWorkspace.appStates).length > 0 ? Object.keys(daemonWorkspace.appStates)[0] ?? null : null)
  console.log('[session] reconstructWorkspace: activeTabId set to', reconstructedActiveTabId, 'workspace:', id, 'session:', store.getState().sessionId)
  const workspace: Workspace = {
    ...daemonWorkspace,
    id,
    activeTabId: reconstructedActiveTabId
  }

  const handle = createHandleForWorkspace(workspace)

  store.setState((s) => ({
    workspaces: new Map(s.workspaces).set(id, { status: WorkspaceEntryStatus.Loaded, data: workspace, store: handle }),
    activeWorkspaceId: id
  }))

  for (const tabId of Object.keys(daemonWorkspace.appStates)) {
    handle.getState().initTab(tabId)
  }

  console.log('[Session] Reconstructed workspace:', daemonWorkspace.name, 'parentId:', parentId)
  return id
}
