/* eslint-disable custom/no-string-literal-comparison -- TODO: migrate existing string-literal comparisons to enums */
import { createStore } from 'zustand/vanilla'
import type { StoreApi } from 'zustand'
import { humanId } from 'human-id'
import { createWorkspaceStore } from './createWorkspaceStore'
import type { WorkspaceStore, WorkspaceStoreDeps } from './createWorkspaceStore'
import { createTtyStore } from './createTtyStore'
import type { Tty, TtyTerminalDeps } from './createTtyStore'
import { FileWatchEventType, type PtyEvent, type FileWatchEvent } from '../../shared/ipc-types'
import { ConnectionStatus, WorkspaceStatus } from '../../shared/types'
import type {
  Workspace, Session, AppState, GitInfo, WorkspaceRef,
  ConnectionInfo, ActivityState, IpcResult,
  TerminalApi, GitApi, FilesystemApi, ExecApi, SessionApi, Settings, WorktreeSettings,
  Application, SandboxConfig, TTYSessionInfo, LlmApi, GitHubApi, RunActionsApi
} from '../types'
import { toDisposable } from '../../shared/lifecycle'
import type { SessionLock } from '../../shared/types'
import { defaultWorktreeSettings, parseWorkspaceFile, toStoredWorkspaceFile } from '../../shared/workspaceFile'
import type { WorktreeRegistryApi } from '../lib/worktreeRegistry'

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
  worktreeRegistry: WorktreeRegistryApi
  getSettings: () => Settings
  appRegistry: AppRegistryApi
  llm: LlmApi
  setActivityTabState: (tabId: string, state: ActivityState) => void
}

/** One worktree to load in an {@link SessionState.autoOpenWorktrees} batch. */
export interface AutoOpenWorktreeItem {
  path: string
  branch: string
  name: string
  /** Parent worktree path, or `null` to attach under the root workspace. */
  parentPath: string | null
  displayName?: string
  description?: string
}

export interface SessionState {
  sessionId: string

  // Connection for this session (local or remote, transitions: connecting → connected/error)
  connection: ConnectionInfo
  /** Apply a connection status update. On a transition back to Connected, flushes
   *  workspace bodies whose writes were deferred while disconnected. */
  handleConnectionStatusChange: (info: ConnectionInfo) => void

  createTty: (cwd: string, sandbox?: SandboxConfig, startupCommand?: string, ptyHandle?: string) => Promise<string>
  /** The returned Tty owns its event subscription — give it to a DisposableStore.
   *  There is no separate cleanup value for a caller to drop, nor for a narrowed
   *  dependency type to silently erase. */
  openTtyStream: (ptyId: string, onEvent: (event: PtyEvent) => void) => Promise<Tty>
  killTty: (ptyId: string) => void
  listTty: () => Promise<TTYSessionInfo[]>

  // Workspace collection
  workspaces: Map<string, WorkspaceEntry>
  activeWorkspaceId: string | undefined
  isRestoring: boolean
  sessionVersion: number
  sessionLock: SessionLock | undefined
  /** Daemon-advertised directory holding the per-workspace JSON files. Empty
   *  until the first SessionWatch event arrives; writes are guarded on it. */
  workspaceDataDir: string

  clearWorkspaceError: (id: string) => void
  dismissWorkspace: (id: string) => void
  /** Reactive cleanup when a workspace is no longer in the daemon session.
   *  Disposes all tab refs (renderer-side), git controller, and removes from map. */
  onWorkspaceRemoved: (id: string) => void
  addWorkspace: (path: string, options?: { skipDefaultTabs?: boolean; settings?: WorktreeSettings }) => string
  addChildWorkspace: (parentId: string, name: string, isDetached?: boolean, settings?: WorktreeSettings, description?: string) => { success: boolean; error?: string }
  adoptExistingWorktree: (parentId: string, worktreePath: string, branch: string, name: string, settings?: WorktreeSettings, description?: string, displayName?: string) => Promise<{ success: boolean; error?: string }>
  /** Batch-adopt multiple worktrees, preserving the detected parent/child hierarchy.
   *  Already-open worktrees are skipped. */
  autoOpenWorktrees: (rootWorkspaceId: string, items: AutoOpenWorktreeItem[]) => Promise<{ success: boolean; error?: string }>
  createWorktreeFromBranch: (parentId: string, branch: string, isDetached: boolean, settings?: WorktreeSettings, description?: string) => { success: boolean; error?: string }
  createWorktreeFromRemote: (parentId: string, remoteBranch: string, isDetached: boolean, settings?: WorktreeSettings, description?: string) => { success: boolean; error?: string }
  /** Open a workspace from an open PR: fetch, create a worktree from the PR head branch, and set the PR title as the workspace title. */
  createWorktreeFromPr: (parentId: string, headRefName: string, title: string, isDetached: boolean, settings?: WorktreeSettings) => { success: boolean; error?: string }
  removeWorkspace: (id: string) => Promise<void>
  removeWorkspaceKeepBranch: (id: string) => Promise<void>
  removeWorkspaceKeepBoth: (id: string) => Promise<void>
  mergeAndRemoveWorkspace: (id: string, squash: boolean) => Promise<{ success: boolean; error?: string }>
  mergeAndKeepWorkspace: (id: string, squash: boolean) => Promise<{ success: boolean; error?: string }>
  closeAndCleanWorkspace: (id: string) => Promise<{ success: boolean; error?: string }>
  setActiveWorkspace: (id: string | undefined) => void
  updateGitInfo: (id: string, gitInfo: GitInfo) => void
  refreshGitInfo: (id: string) => Promise<void>
  quickForkWorkspace: (workspaceId: string) => Promise<{ success: boolean; error?: string }>
  reorderWorkspace: (workspaceId: string, targetWorkspaceId: string, position: 'before' | 'after') => void
  moveWorkspace: (workspaceId: string, targetWorkspaceId: string, position: 'before' | 'after' | 'onto') => void
  syncToDaemon: (reason: string) => Promise<void>
  /** Write every workspace body whose write was deferred or failed, and wait for
   *  those writes to settle. Called on reconnect before this store is replaced. */
  flushDeferredWrites: () => Promise<void>
  forceUnlock: () => Promise<{ success: boolean; error?: string }>

  // Session lifecycle
  handleRestore: (session: Session) => Promise<void>
  handleExternalUpdate: (session: Session) => Promise<void>
  /** Tear the session down: stop every workspace file watch, drop sync bookkeeping,
   *  and dispose each workspace's resources. Must be called before the store is
   *  dropped (reconnect rebuild, session removal) — otherwise the leaked file
   *  watches keep the store alive as a ghost that still writes the same JSON files
   *  as its replacement, causing sustained CAS conflicts. */
  dispose: () => void
}

/** JSON.stringify with sorted keys so field ordering doesn't affect comparison */
function stableStringify(value: unknown): string {
  return JSON.stringify(value, (_key, val: unknown) => {
    if (val && typeof val === 'object' && !Array.isArray(val)) {
      const sorted: Record<string, unknown> = {}
      for (const k of Object.keys(val as Record<string, unknown>).sort()) {
        sorted[k] = (val as Record<string, unknown>)[k]
      }
      return sorted
    }
    return val
  })
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
    .filter(ws => ws.isWorktree && ws.status === WorkspaceStatus.Active)
}

/** Lowercase-hex SHA-256 of a string's UTF-8 bytes. Matches the daemon's
 *  `sha256_hex`, so a locally-computed hash can be compared against a watch
 *  event's sha to recognise (and suppress) the echo of our own write. */
async function sha256Hex(text: string): Promise<string> {
  const bytes = new TextEncoder().encode(text)
  const digest = await crypto.subtle.digest('SHA-256', bytes)
  return Array.from(new Uint8Array(digest)).map(b => b.toString(16).padStart(2, '0')).join('')
}

export function createSessionStore(
  config: { sessionId: string; connection: ConnectionInfo },
  deps: SessionDeps
): StoreApi<SessionState> {
  // Acquire session lock. Serialized through the same queue as syncs so lock/unlock
  // RPCs never race with UpdateSession on the daemon side.
  function acquireLock(): Promise<{ acquired: true } | { acquired: false; error: string }> {
    return enqueueOp(async () => {
      const lockResult = await deps.sessionApi.lock(store.getState().connection.id, 60_000)
      if (lockResult.success && lockResult.acquired) {
        store.setState({
          sessionVersion: lockResult.session.version,
          sessionLock: lockResult.session.lock,
        })
        return { acquired: true } as const
      }
      if (!lockResult.success) return { acquired: false, error: lockResult.error } as const
      return { acquired: false, error: 'Session is locked by another window' } as const
    })
  }

  // Release session lock. Serialized through the queue. `sessionVersion` is taken
  // directly from the daemon's response — no optimistic increments.
  function releaseLock(): Promise<void> {
    return enqueueOp(async () => {
      const unlockResult = await deps.sessionApi.unlock(store.getState().connection.id)
      if (unlockResult.success) {
        store.setState({
          sessionVersion: unlockResult.session.version,
          sessionLock: unlockResult.session.lock,
        })
      }
    })
  }

  function nextSortOrder(parentId: string | undefined): string {
    const workspaces = store.getState().workspaces
    let max = -1
    for (const entry of Array.from(workspaces.values())) {
      if (entry.status !== WorkspaceEntryStatus.Loaded && entry.status !== WorkspaceEntryStatus.OperationError) continue
      const ws = entry.data
      const isMatch = parentId ===undefined ? !ws.parentId : ws.parentId === parentId
      if (isMatch) {
        const order = parseInt(entry.store.getState().metadata.sortOrder || '0')
        if (order > max) max = order
      }
    }
    return String(max + 1)
  }

  function reindexSiblings(parentId: string | undefined, excludeId: string): void {
    const workspaces = store.getState().workspaces
    const siblings: { id: string; entry: Extract<WorkspaceEntry, { status: WorkspaceEntryStatus.Loaded | WorkspaceEntryStatus.OperationError }> }[] = []
    for (const [id, entry] of Array.from(workspaces.entries())) {
      if (id === excludeId) continue
      if (entry.status !== WorkspaceEntryStatus.Loaded && entry.status !== WorkspaceEntryStatus.OperationError) continue
      const isMatch = parentId ===undefined ? !entry.data.parentId : entry.data.parentId === parentId
      if (isMatch) siblings.push({ id, entry })
    }
    siblings.sort((a, b) => parseInt(a.entry.store.getState().metadata.sortOrder || '0') - parseInt(b.entry.store.getState().metadata.sortOrder || '0'))
    for (let i = 0; i < siblings.length; i++) {
      const item = siblings[i]
      if (!item) continue
      const { id, entry } = item
      const newMetadata = { ...entry.store.getState().metadata, sortOrder: String(i) }
      const newData = { ...entry.data, metadata: newMetadata }
      entry.store.getState().setWorkspace(newData)
      store.setState(s => ({
        workspaces: new Map(s.workspaces).set(id, { ...entry, data: newData })
      }))
    }
  }

  // The ref list (membership) we last sent to / received from the daemon, as a
  // stable JSON string. Used to suppress echoes and skip no-op ref syncs.
  let lastSyncedRefsJson = ''

  // Ids of workspaces created here whose ref the daemon has not yet acknowledged.
  // Ref reconciliation (from a rejected sync or an external session event) must not
  // treat them as "removed elsewhere": they were never published in the first place.
  const unpublishedIds = new Set<string>()
  // id → path of every ref the daemon currently lists (maintained by reconcileRefs).
  // A ref whose body has not arrived, or failed to parse, is only a Loading/Error
  // placeholder locally; currentRefs still has to re-publish it or a sync from this
  // window would drop another window's workspace from the session.
  const knownRefs = new Map<string, string>()
  // Consecutive rejected ref syncs; past the cap the unpublished workspaces are
  // surfaced as errors instead of retrying forever. Reset on acceptance or a new add.
  const MAX_REF_SYNC_ATTEMPTS = 5
  let refSyncAttempts = 0
  // A ref sync was rejected while another window held the session lock. The lock
  // release is broadcast to every watcher, so the next session event retries.
  let refSyncRetryOnNextEvent = false

  // Per-workspace content-sync bookkeeping. The JSON body of each workspace is
  // written via CAS WriteFile and observed via WatchFile, independently of the
  // ref-list (membership) sync below.
  //
  // How many recent self-write hashes to retain for echo suppression. A backstop
  // bound: if a write's echo is ever coalesced away by the watcher and never
  // observed, its hash would otherwise linger forever.
  const RECENT_HASHES_MAX = 32
  interface WorkspaceSyncState {
    // sha256 of the body we believe the daemon currently holds ('' = absent/unknown).
    // Used as the CAS guard for, and chained as the parentHash of, the next write.
    lastSeenSha: string
    // Last body we serialized and wrote, to skip redundant writes.
    lastWrittenJson: string
    // sha256s of the recent bodies we have written or applied, newest last. The watch
    // event for our own write can arrive before writeFile resolves (so before lastSeenSha
    // advances), so we record each body's hash up front — synchronously, before the write.
    // parentHash chaining makes every body's hash distinct (even when its logical content
    // reverts), so matching an echo by sha against this ring reliably tells our own writes
    // apart from a genuine external edit. Without it we re-apply our own write, rebuilding
    // the workspace object graph and tearing down the just-mounted terminal tab. Bounded to
    // RECENT_HASHES_MAX, oldest evicted first.
    recentHashes: string[]
    // Active file-watch handle (undefined until the watch is opened).
    unsubscribe?: () => void
    // Per-workspace serial write queue + coalesce flag.
    tail: Promise<unknown>
    pending: boolean
    // A write was skipped while disconnected; flushed when the connection returns.
    dirty: boolean
    // A CAS write conflicted before its guard sha advanced. The next watch event
    // (which carries the winning sha) re-enqueues a write so local-only state
    // (e.g. a freshly-created ptyId) still lands instead of silently diverging.
    retryOnNextEvent: boolean
    // Consecutive CAS conflicts; surfaced as a workspace error past a threshold.
    conflictStreak: number
    // A genuine external body was applied since the current write captured its guard.
    // A conflict after that has no losing delta left to rewrite: local state *is* the
    // winner, and re-writing it would only publish the same body under a new parentHash.
    externalApplied: boolean
    // Consecutive watch-stream errors while connected; the watch is reopened with
    // backoff up to MAX_WATCH_RETRIES, then surfaced as a workspace error.
    watchRetries: number
    watchRetryTimer?: ReturnType<typeof setTimeout>
  }
  const wsSync = new Map<string, WorkspaceSyncState>()

  // Set once dispose() runs. Guards every watch/write path so a file-watch callback
  // that fires during teardown (the unsubscribe and the event can race) can neither
  // write nor re-init tabs on a store that is being thrown away.
  let disposed = false

  function getOrCreateSync(id: string): WorkspaceSyncState {
    let s = wsSync.get(id)
    if (!s) {
      s = { lastSeenSha: '', lastWrittenJson: '', recentHashes: [], unsubscribe: undefined, tail: Promise.resolve(), pending: false, dirty: false, retryOnNextEvent: false, conflictStreak: 0, externalApplied: false, watchRetries: 0, watchRetryTimer: undefined }
      wsSync.set(id, s)
    }
    return s
  }

  // Remember a body's sha as one of ours, bounding the ring (oldest evicted first).
  function rememberHash(sync: WorkspaceSyncState, sha: string): void {
    if (sync.recentHashes.includes(sha)) return
    sync.recentHashes.push(sha)
    if (sync.recentHashes.length > RECENT_HASHES_MAX) sync.recentHashes.shift()
  }

  // Open a content watch for a workspace's JSON file (idempotent). The first event
  // is the current state; subsequent events reconcile external edits.
  function ensureWatch(id: string, path: string): void {
    if (disposed) return
    const sync = getOrCreateSync(id)
    if (sync.unsubscribe) return
    const { workspaceDataDir: dataDir, connection } = store.getState()
    if (!dataDir) return
    // Opening a watch on a dead connection only yields an Error event. The reconnect
    // rebuilds the store, and with it every watch, so there is nothing to keep here.
    if (connection.status !== ConnectionStatus.Connected) return
    const handle = deps.filesystem.watchFile(
      dataDir,
      `${id}.json`,
      (event) => { onFileEvent(id, path, event) }
    )
    sync.unsubscribe = handle.unsubscribe
  }

  // How many times a watch stream that errors while connected is reopened before
  // the workspace is surfaced as broken. Backoff doubles from WATCH_RETRY_BASE_MS.
  const MAX_WATCH_RETRIES = 3
  const WATCH_RETRY_BASE_MS = 1000

  function clearWatchRetry(sync: WorkspaceSyncState): void {
    if (sync.watchRetryTimer !== undefined) {
      clearTimeout(sync.watchRetryTimer)
      sync.watchRetryTimer = undefined
    }
  }

  // A watch stream errored while the connection itself is healthy (e.g. the daemon
  // drained subscribers because the file became unreadable). Drop the dead handle
  // and reopen: without this ensureWatch is a permanent no-op and the workspace
  // stops seeing external edits, while any later CAS conflict waits forever for a
  // winner event that never arrives.
  function reopenWatchAfterError(id: string, path: string, sync: WorkspaceSyncState, message: string): void {
    if (sync.unsubscribe) sync.unsubscribe()
    sync.unsubscribe = undefined
    sync.watchRetries++
    if (sync.watchRetries > MAX_WATCH_RETRIES) {
      setWorkspaceFileError(id, `Workspace file watch failed: ${message}`)
      return
    }
    clearWatchRetry(sync)
    const delay = WATCH_RETRY_BASE_MS * 2 ** (sync.watchRetries - 1)
    sync.watchRetryTimer = setTimeout(() => {
      sync.watchRetryTimer = undefined
      ensureWatch(id, path)
    }, delay)
  }

  function setWorkspaceFileError(id: string, error: string): void {
    // A dead connection fails every workspace's file sync at once. That is one
    // connection fault, not N workspace faults: the connection banner owns it and
    // the reconnect rebuilds the session from the daemon. Surfacing it per
    // workspace buries the banner under a dialog whose buttons all need the very
    // connection that just died.
    if (store.getState().connection.status !== ConnectionStatus.Connected) return

    const entry = store.getState().workspaces.get(id)
    if (!entry) return
    if (entry.status === WorkspaceEntryStatus.Loaded || entry.status === WorkspaceEntryStatus.OperationError) {
      store.setState(s => ({
        workspaces: new Map(s.workspaces).set(id, { status: WorkspaceEntryStatus.OperationError, data: entry.data, store: entry.store, error })
      }))
    } else {
      store.setState(s => ({
        workspaces: new Map(s.workspaces).set(id, { status: WorkspaceEntryStatus.Error, name: entry.status === WorkspaceEntryStatus.Loading ? entry.name : id, error })
      }))
    }
  }

  // Apply a workspace body received from a file-watch event into the store.
  function onFileEvent(id: string, path: string, event: FileWatchEvent): void {
    if (disposed) return // teardown in progress; the watch may fire before unsubscribe lands
    const sync = wsSync.get(id)
    if (!sync) return // unsubscribed (workspace removed)

    if (event.type === FileWatchEventType.Present) {
      sync.watchRetries = 0
      // Echo of one of our own writes. The watch event can arrive before writeFile()
      // resolves (so before lastSeenSha advances), so we matched against the ring of
      // hashes we recorded up front. parentHash chaining guarantees each body's hash is
      // distinct, so a sha hit means we wrote it: re-applying it would needlessly rebuild
      // the workspace and tear down a just-mounted terminal.
      if (sync.recentHashes.includes(event.sha256)) {
        sync.lastSeenSha = event.sha256
        // A conflicted write lost to one of our own earlier writes: local state still
        // holds the losing delta, so rewrite it on top of the winning sha.
        if (sync.retryOnNextEvent) {
          sync.retryOnNextEvent = false
          enqueueContentSync(id)
        }
        return
      }
      sync.lastSeenSha = event.sha256
      // A conflicted write lost to a genuine external edit: applying the winning body
      // below replaces local state, so there is no losing delta left to rewrite. That
      // also holds for a conflict response still in flight (externalApplied), and the
      // divergence a conflict streak was counting is resolved by adopting the winner.
      sync.retryOnNextEvent = false
      sync.externalApplied = true
      sync.conflictStreak = 0
      let workspace: Workspace
      try {
        workspace = parseWorkspaceFile(id, path, event.content)
      } catch (err) {
        setWorkspaceFileError(id, `Invalid workspace file: ${err instanceof Error ? err.message : String(err)}`)
        return
      }
      // A genuine external edit is now the current body; record it so its own later echo
      // (or a redundant re-read) is recognized rather than re-applied.
      rememberHash(sync, event.sha256)
      applyWorkspaceFile(store, workspace, createHandleForWorkspace)
    } else if (event.type === FileWatchEventType.Absent) {
      sync.watchRetries = 0
      // The file backing a known ref is gone. Surface loudly — membership removal
      // goes through ref-list reconciliation, not the content watch.
      setWorkspaceFileError(id, 'Workspace file is missing')
    } else {
      // While disconnected the stream error just mirrors the connection fault: the
      // banner owns it and the reconnect rebuilds the store (and every watch).
      if (store.getState().connection.status !== ConnectionStatus.Connected) return
      reopenWatchAfterError(id, path, sync, event.message)
    }
  }

  // Write a workspace's current body to its JSON file via CAS. Coalesced and
  // serialized per workspace id (see enqueueContentSync).
  // How many consecutive CAS conflicts a workspace tolerates before the divergence
  // is surfaced as a workspace error and retrying stops. The streak resets when a
  // write lands or when an external body is adopted (the divergence is then gone).
  const MAX_CONFLICT_STREAK = 5

  // Reset the "an external body was applied" marker for the write about to start.
  // Done through a call rather than inline so the flow analysis does not carry the
  // `false` across the awaits — onFileEvent flips it while the RPC is in flight.
  function beginWriteAttempt(sync: WorkspaceSyncState): void {
    sync.externalApplied = false
  }

  async function writeWorkspaceContent(id: string): Promise<void> {
    if (disposed) return
    const { workspaces, connection, workspaceDataDir } = store.getState()
    const sync = wsSync.get(id)
    if (!sync) return
    if (connection.status !== ConnectionStatus.Connected) {
      // Not droppable: local state may already hold changes (e.g. a fresh ptyId)
      // the disk copy lacks. Flushed by handleConnectionStatusChange on reconnect.
      sync.dirty = true
      return
    }
    if (!workspaceDataDir) return
    const entry = workspaces.get(id)
    if (!entry || (entry.status !== WorkspaceEntryStatus.Loaded && entry.status !== WorkspaceEntryStatus.OperationError)) return
    sync.dirty = false

    // Chain the parent's hash into the body so its own hash is distinct even if the
    // logical content reverts to an earlier state (parentHash == the CAS guard sha).
    const guardSha = sync.lastSeenSha
    beginWriteAttempt(sync)
    const json = stableStringify(toStoredWorkspaceFile(entry.data, guardSha))
    if (json === sync.lastWrittenJson) return

    // Record the hash before writing: the daemon can emit the watch echo before
    // writeFile() resolves, and onFileEvent must already know to suppress it by sha.
    const newSha = await sha256Hex(json)
    rememberHash(sync, newSha)

    let result: Awaited<ReturnType<typeof deps.filesystem.writeFile>>
    try {
      result = await deps.filesystem.writeFile(workspaceDataDir, `${id}.json`, json, guardSha)
    } catch (err) {
      // The writeFile RPC itself rejected — typically the transport dropping
      // mid-call. An unhandled rejection here would also silently lose this body:
      // route it through the same lost-delta guard as a {success:false} failure.
      handleWriteFailure(id, sync, err instanceof Error ? err.message : String(err))
      return
    }
    if (result.success) {
      sync.lastWrittenJson = json
      sync.lastSeenSha = newSha
      sync.conflictStreak = 0
    } else {
      // The write did not land; its hash stays in the bounded ring and ages out
      // harmlessly (its echo will never arrive).
      if ('conflict' in result) {
        // The winner's event already arrived and replaced local state with the
        // winning body. Nothing of ours is left to rewrite; a retry would only
        // republish the winner under a new parentHash, which every other window
        // would then apply as an "external edit" of its own.
        if (sync.externalApplied) return
        // Another write won. Local state may hold changes the winning body lacks, so
        // rewrite it on top of the winner instead of dropping it — that silent
        // divergence is what left ptyId:null on disk and orphaned PTYs on reconnect.
        sync.conflictStreak++
        if (sync.conflictStreak >= MAX_CONFLICT_STREAK) {
          setWorkspaceFileError(id, `Workspace file keeps conflicting with another writer (${String(sync.conflictStreak)} attempts)`)
          return
        }
        if (sync.lastSeenSha !== guardSha) {
          // The winning body's watch event already arrived — retry on top of it now.
          enqueueContentSync(id)
        } else {
          // The winner's event is still in flight; onFileEvent retries when it lands.
          sync.retryOnNextEvent = true
        }
      } else {
        handleWriteFailure(id, sync, result.error)
      }
    }
  }

  // A non-conflict write failure (RPC returned {success:false} or rejected). The body
  // is always marked dirty so the next reconnect flush re-writes it: a transport drop
  // can reject the RPC *before* the Reconnecting status reaches this store, and the
  // status handler then clears the error it would otherwise leave behind — without
  // the dirty flag that path silently lost the delta (e.g. a fresh ptyId) and the
  // reconnect rebuilt the workspace from the stale disk copy. While the connection
  // still reads healthy it is also a genuine failure: fail loudly.
  function handleWriteFailure(id: string, sync: WorkspaceSyncState, error: string): void {
    sync.dirty = true
    if (store.getState().connection.status !== ConnectionStatus.Connected) return
    setWorkspaceFileError(id, `Failed to save workspace: ${error}`)
  }

  // Enqueue every dirty body and wait for those writes to settle. Used on reconnect
  // before the store is replaced, so the flush cannot race the new store's first read.
  async function flushDeferredWrites(): Promise<void> {
    for (const [id, sync] of Array.from(wsSync.entries())) {
      if (sync.dirty) enqueueContentSync(id)
    }
    await Promise.all(Array.from(wsSync.values()).map(sync => sync.tail))
  }

  // Enqueue a content write for a workspace. Coalesces rapid mutations: if a write
  // is already queued (not yet started), it will pick up the latest state.
  function enqueueContentSync(id: string): void {
    if (disposed) return
    const sync = getOrCreateSync(id)
    if (sync.pending) return
    sync.pending = true
    const run = async (): Promise<void> => {
      sync.pending = false
      await writeWorkspaceContent(id)
    }
    sync.tail = sync.tail.then(run, run)
  }

  // Enqueue a content write for every loaded workspace. Used after multi-workspace
  // mutations (reorder/move) that touch several siblings' sortOrder/parentId. Each
  // write is skipped if the body is unchanged, so over-enqueuing is cheap.
  function syncAllContent(): void {
    for (const [id, entry] of Array.from(store.getState().workspaces.entries())) {
      if (entry.status === WorkspaceEntryStatus.Loaded || entry.status === WorkspaceEntryStatus.OperationError) {
        enqueueContentSync(id)
      }
    }
  }

  // Write the initial JSON file for a newly-created workspace, then open its watch.
  // Writing first (CAS expected_sha256 '' → must-not-exist) means any other window
  // that later sees the published ref finds the file already present. The watch is
  // opened only after the write so its first (Present) event matches lastSeenSha
  // and is suppressed rather than racing an Absent read against the pending write.
  //
  // The write goes through the same per-workspace serial queue as enqueueContentSync:
  // a follow-up mutation (e.g. the tab's ptyId landing after PTY creation) enqueues
  // while this write is still in flight, and both would otherwise CAS against the
  // same pre-create sha — the loser was dropped, leaving ptyId:null on disk.
  async function createWorkspaceFile(id: string, path: string): Promise<void> {
    unpublishedIds.add(id)
    refSyncAttempts = 0
    const sync = getOrCreateSync(id)
    const write = sync.tail.then(() => writeWorkspaceContent(id), () => writeWorkspaceContent(id))
    sync.tail = write.catch(() => undefined)
    await write
    ensureWatch(id, path)
  }

  // Single serial queue for every op that bumps the daemon's session version
  // (UpdateSession, LockSession, UnlockSession). Ordering on the wire follows
  // enqueue order, so no two daemon-version-bumping RPCs can race on the
  // daemon's mutex. That removes the original ptyId/connectionId divergence
  // bug where a racing UnlockSession caused a rejected UpdateSession to look
  // accepted (because daemon.version coincidentally == expected + 1).
  interface PendingSync {
    reasons: string[]
    settlers: { resolve: () => void; reject: (err: unknown) => void }[]
  }
  let queueTail: Promise<unknown> = Promise.resolve()
  let pendingSync: PendingSync | undefined = undefined

  function chain<T>(op: () => Promise<T>): Promise<T> {
    const next = queueTail.then(op, op)
    queueTail = next.catch(() => undefined)
    return next
  }

  // Enqueue a sync. If the next queued op is an un-started sync, collapse into it
  // so rapid triggers coalesce into one UpdateSession.
  function enqueueSync(reason: string): Promise<void> {
    if (pendingSync !== undefined) {
      pendingSync.reasons.push(reason)
      return new Promise<void>((resolve, reject) => {
        // eslint-disable-next-line @typescript-eslint/no-non-null-assertion -- checked above; closure reads the captured reference
        pendingSync!.settlers.push({ resolve, reject })
      })
    }
    const slot: PendingSync = { reasons: [reason], settlers: [] }
    pendingSync = slot
    return new Promise<void>((resolve, reject) => {
      slot.settlers.push({ resolve, reject })
      void chain(async () => {
        // Freeze the collapse window: any further sync must create a new slot.
        if (pendingSync === slot) pendingSync = undefined
        try {
          await syncRefs(slot.reasons.join(', '))
          for (const s of slot.settlers) s.resolve()
        } catch (err) {
          for (const s of slot.settlers) s.reject(err)
        }
      })
    })
  }

  // Enqueue a non-sync op (lock/unlock). Subsequent syncs can't collapse past
  // this boundary — they'll create a fresh slot behind it.
  function enqueueOp<T>(op: () => Promise<T>): Promise<T> {
    pendingSync = undefined
    return chain(op)
  }

  // Build the current ref list (membership): every loaded workspace, plus the
  // daemon-listed refs that are still placeholders here (see knownRefs). A local
  // placeholder the daemon does not know (addWorkspace before its git info resolves)
  // is not a member yet.
  function currentRefs(): WorkspaceRef[] {
    const refs: WorkspaceRef[] = []
    for (const [id, e] of Array.from(store.getState().workspaces.entries())) {
      if (e.status === WorkspaceEntryStatus.Loaded || e.status === WorkspaceEntryStatus.OperationError) {
        refs.push({ id: e.data.id, path: e.data.path })
        continue
      }
      const knownPath = knownRefs.get(id)
      if (knownPath !== undefined) refs.push({ id, path: knownPath })
    }
    return refs
  }

  // Reconcile the local workspace set against the daemon's ref list: open watches
  // and create placeholders for new refs, remove loaded entries no longer present.
  function reconcileRefs(refs: WorkspaceRef[]): void {
    store.setState({ isRestoring: true })
    const incomingIds = new Set(refs.map(r => r.id))
    for (const id of Array.from(knownRefs.keys())) {
      if (!incomingIds.has(id)) knownRefs.delete(id)
    }

    for (const ref of refs) {
      knownRefs.set(ref.id, ref.path)
      unpublishedIds.delete(ref.id) // the daemon knows it now, however it got there
      ensureWatch(ref.id, ref.path)
      const entry = store.getState().workspaces.get(ref.id)
      if (!entry) {
        // Placeholder until the file watch delivers the body.
        store.setState(s => ({
          workspaces: new Map(s.workspaces).set(ref.id, {
            status: WorkspaceEntryStatus.Loading, name: getNameFromPath(ref.path), message: 'Loading workspace...', output: []
          })
        }))
      }
    }

    for (const [id, entry] of Array.from(store.getState().workspaces.entries())) {
      if (incomingIds.has(id)) continue
      // Added here and not yet published: absent from the daemon's list because our
      // sync has not landed (or was rejected), not because another window removed it.
      if (unpublishedIds.has(id)) continue
      if (entry.status === WorkspaceEntryStatus.Loaded || entry.status === WorkspaceEntryStatus.OperationError) {
        store.getState().onWorkspaceRemoved(id)
      }
    }
    store.setState({ isRestoring: false })
  }

  // Push the current ref list (membership) to the daemon. Goes through the serial
  // queue so it can't race lock/unlock. Content bodies sync separately.
  async function syncRefs(reason: string): Promise<void> {
    // Captured outside the try so a thrown RPC also rolls back the optimistic
    // lastSyncedRefsJson below; otherwise the next sync of the same refs is a no-op
    // and the membership change never reaches the daemon.
    const prevSyncedJson = lastSyncedRefsJson
    try {
      const { connection } = store.getState()
      if (connection.status !== ConnectionStatus.Connected) {
        console.log('[session] connection not yet established, skipping ref sync')
        return
      }

      const refs = currentRefs()
      const currentJson = stableStringify(refs)
      if (currentJson === lastSyncedRefsJson) return

      console.log('[session] syncing workspace refs to daemon, reason:', reason)

      // Optimistic: treat our send as the new known ref state so the echo is skipped.
      lastSyncedRefsJson = currentJson

      const expectedVersion = store.getState().sessionVersion
      const connectionId = connection.id
      const result = await deps.sessionApi.update(connectionId, refs, connectionId, expectedVersion)
      if (!result.success) {
        lastSyncedRefsJson = prevSyncedJson
        console.error('[session] failed to update session refs:', result.error)
        return
      }
      // Cross-check: an accepted response must echo exactly what we sent at the next
      // version. Anything else means the daemon rejected it (version mismatch or a
      // lock held by another connection) and returned its current state instead.

      const returnedJson = stableStringify(result.session.workspaceRefs)
      const versionOk = result.session.version === expectedVersion + 1
      const accepted = versionOk && returnedJson === currentJson

      if (accepted) {
        for (const ref of refs) unpublishedIds.delete(ref.id)
        refSyncAttempts = 0
        refSyncRetryOnNextEvent = false
        store.setState({
          sessionVersion: result.session.version,
          sessionLock: result.session.lock,
          workspaceDataDir: result.session.workspaceDataDir,
        })
      } else {
        const lockedByOther = result.session.lock !== undefined
        const reason = lockedByOther
          ? 'session locked by another window'
          : versionOk
            ? 'refs changed under us (content mismatch)'
            : `version mismatch, expected ${String(expectedVersion + 1)} got ${String(result.session.version)}`
        console.warn('[session] ref update rejected —', reason, '— reconciling')
        store.setState({
          sessionVersion: result.session.version,
          sessionLock: result.session.lock,
          workspaceDataDir: result.session.workspaceDataDir,
        })
        // Adopt the daemon's membership as the acknowledged base. Workspaces added
        // here but not yet published survive reconcileRefs and are retried below.
        lastSyncedRefsJson = stableStringify(result.session.workspaceRefs)
        reconcileRefs(result.session.workspaceRefs)
        scheduleRefSyncRetry(reason, lockedByOther)
      }
    } catch (error) {
      lastSyncedRefsJson = prevSyncedJson
      console.error('[session] failed to sync refs to daemon:', error)
    }
  }

  // After a rejected ref sync: retry now that the version is fresh, or — when
  // another window holds the lock, so an immediate retry would be rejected the same
  // way — on the next session event (the unlock is broadcast to every watcher).
  // Past MAX_REF_SYNC_ATTEMPTS the unpublished workspaces are surfaced as errors.
  function scheduleRefSyncRetry(reason: string, lockedByOther: boolean): void {
    if (unpublishedIds.size === 0 && stableStringify(currentRefs()) === lastSyncedRefsJson) return
    refSyncAttempts++
    if (refSyncAttempts >= MAX_REF_SYNC_ATTEMPTS) {
      for (const id of Array.from(unpublishedIds)) {
        setWorkspaceFileError(id, `Could not publish workspace to the session after ${String(refSyncAttempts)} attempts: ${reason}`)
      }
      return
    }
    if (lockedByOther) {
      refSyncRetryOnNextEvent = true
      return
    }
    void enqueueSync(`retry after rejection (${reason})`)
  }

  function makeHandleDeps(workspaceId: string): WorkspaceStoreDeps {
    return {
      appRegistry: deps.appRegistry,
      openTtyStream: (ptyId: string, onEvent: (event: PtyEvent) => void) => store.getState().openTtyStream(ptyId, onEvent),
      createTty: (cwd, sandbox?, startupCommand?, ptyHandle?) => store.getState().createTty(cwd, sandbox, startupCommand, ptyHandle),
      connectionId: config.connection.id,
      git: deps.git,
      filesystem: deps.filesystem,
      exec: deps.exec,
      runActions: deps.runActions,
      getSettings: deps.getSettings,
      llm: deps.llm,
      setActivityTabState: deps.setActivityTabState,
      // Workspace mutations persist that workspace's JSON body (not the ref list).
      syncToDaemon: () => { enqueueContentSync(workspaceId); },
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
      subscribeWorkspaceChanges: (callback) => store.subscribe(callback),
      github: deps.github,
      worktreeRegistry: deps.worktreeRegistry,
    }
  }

  function createHandleForWorkspace(workspace: Workspace): WorkspaceStore {
    const handle = createWorkspaceStore(workspace, makeHandleDeps(workspace.id))

    // Keep the workspaces snapshot in sync when handle state changes
    handle.subscribe((state) => {
      store.setState((s) => {
        const entry = s.workspaces.get(state.workspace.id)
        if (!entry || (entry.status !== WorkspaceEntryStatus.Loaded && entry.status !== WorkspaceEntryStatus.OperationError)) return s
        if (entry.data === state.workspace) return s
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
      displayName?: string
      initialBranch?: string | undefined
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
        let activeTabId: string | undefined = undefined
        const currentParentEntry = store.getState().workspaces.get(parentId)
        const parentSettings = currentParentEntry && (currentParentEntry.status === WorkspaceEntryStatus.Loaded || currentParentEntry.status === WorkspaceEntryStatus.OperationError)
          ? currentParentEntry.data.settings
          : undefined
        const defaultApp = getDefaultAppForWorktree(deps, options.settings, parentSettings)
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
          status: WorkspaceStatus.Active,
          isGitRepo: true,
          gitBranch: result.branch ?? '',
          gitRootPath: parent?.gitRootPath ,
          isWorktree: true,
          isDetached: options.isDetached ?? false,
          appStates,
          activeTabId,
          metadata: {
            sortOrder: nextSortOrder(parentId),
            ...(options.description ? { description: options.description } : {}),
            ...(options.displayName ? { displayName: options.displayName } : {}),
            ...(options.initialBranch ? { branchIsUserDefined: 'true' } : {}),
          },
          settings: options.settings ?? defaultWorktreeSettings,
          favouritePaths: [],
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
        await createWorkspaceFile(id, childWorkspace.path)
        await enqueueSync('addChildWorkspace')
        void handle.getState().saveRegistryEntry()
      } catch (err) {
        store.setState(s => ({
          workspaces: new Map(s.workspaces).set(id, { status: WorkspaceEntryStatus.Error, name: worktreeName, error: err instanceof Error ? err.message : String(err) })
        }))
      } finally {
        await releaseLock().catch((e: unknown) => { console.error('[session] failed to unlock session:', e) })
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
    let activeTabId: string | undefined = undefined

    const parentSettings = parentEntry && parentEntry.status === WorkspaceEntryStatus.Loaded ? parentEntry.data.settings : undefined
    const defaultApp = getDefaultAppForWorktree(deps, options.settings, parentSettings)
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
      status: WorkspaceStatus.Active,
      isGitRepo: true,
      gitBranch: branch,
      gitRootPath: parent?.gitRootPath ,
      isWorktree: options.isWorktree ?? true,
      isDetached: options.isDetached ?? false,
      appStates,
      activeTabId,
      metadata: { sortOrder: nextSortOrder(parentId), ...(options.metadata ?? {}) },
      settings: options.settings ?? defaultWorktreeSettings,
      favouritePaths: [],
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

    await createWorkspaceFile(id, childWorkspace.path)
    await enqueueSync('addChildWorkspaceFromResult')
    void handle.getState().saveRegistryEntry()
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
        try {
          await deps.worktreeRegistry.remove(workspace.path)
        } catch (err) {
          console.error('[session] failed to remove worktree registry entry:', err)
        }
      } else if (!options.keepBranch && !workspace.isDetached && workspace.gitBranch) {
        await deps.git.deleteBranch(workspace.gitRootPath, workspace.gitBranch, options.onProgress)
      }
    }

    // Renderer cleanup + remove from map (also unsubscribes the file watch)
    store.getState().onWorkspaceRemoved(id)

    // Publish the new ref list (without this workspace), then delete its JSON file.
    // Membership is the source of truth; the file delete is cleanup after.
    await enqueueSync('removeWorkspace')

    const dataDir = store.getState().workspaceDataDir
    if (dataDir) {
      const result = await deps.filesystem.deleteFile(dataDir, `${id}.json`)
      if (!result.success) {
        console.error('[session] failed to delete workspace file:', id, result.error)
      }
    }
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
      await releaseLock().catch((e: unknown) => { console.error('[session] failed to unlock session:', e) })
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
    detach: (handle: string) => { deps.terminal.detach(handle); },
  }

  const store = createStore<SessionState>()((set, get) => ({
    sessionId: config.sessionId,
    workspaces: new Map<string, WorkspaceEntry>(),
    activeWorkspaceId: undefined,
    isRestoring: false,
    sessionVersion: 0,
    sessionLock: undefined,
    workspaceDataDir: '',

    connection: config.connection,

    handleConnectionStatusChange: (info: ConnectionInfo): void => {
      const prevStatus = get().connection.status
      set({ connection: info })
      if (info.status !== ConnectionStatus.Connected && prevStatus === ConnectionStatus.Connected) {
        // An in-flight RPC can reject just before this status arrives, so the guard
        // in setWorkspaceFileError cannot catch every case. Drop the errors it let
        // through: they describe the connection, which now has its own banner.
        set((s) => {
          const workspaces = new Map(s.workspaces)
          for (const [id, entry] of Array.from(workspaces.entries())) {
            if (entry.status === WorkspaceEntryStatus.OperationError) {
              workspaces.set(id, { status: WorkspaceEntryStatus.Loaded, data: entry.data, store: entry.store })
            }
          }
          return { workspaces }
        })
      }
      if (info.status === ConnectionStatus.Connected && prevStatus !== ConnectionStatus.Connected) {
        // Flush bodies whose writes were deferred while disconnected, before the
        // stale on-disk copy can be used to rebuild state on reconnect.
        for (const [id, sync] of Array.from(wsSync.entries())) {
          if (sync.dirty) enqueueContentSync(id)
        }
      }
    },

    createTty: async (cwd: string, sandbox?: SandboxConfig, startupCommand?: string, ptyHandle?: string): Promise<string> => {
      const handle = crypto.randomUUID()
      const result = await deps.terminal.create(connectionId, handle, cwd, sandbox, startupCommand, ptyHandle)
      if (!result.success) {
        throw new Error(result.error || 'Failed to create PTY')
      }
      console.log('[Session] tty started', { ptyId: result.sessionId, connectionId, cwd, startupCommand: startupCommand ?? null })
      return result.sessionId
    },

    openTtyStream: async (ptyId: string, onEvent: (event: PtyEvent) => void): Promise<Tty> => {
      const handle = crypto.randomUUID()
      // Registered before attach so the daemon's replayed scrollback is not missed.
      const subscription = toDisposable(deps.terminal.onEvent(handle, onEvent))
      let result: IpcResult
      try {
        result = await deps.terminal.attach(connectionId, handle, ptyId)
      } catch (error) {
        subscription.dispose()
        throw error
      }
      if (!result.success) {
        subscription.dispose()
        throw new Error(result.error || 'Failed to attach to PTY')
      }
      return createTtyStore(ptyId, handle, boundTerminal, subscription)
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
          activeWorkspaceId: s.activeWorkspaceId === id ? undefined : s.activeWorkspaceId
        }
      })
    },

    onWorkspaceRemoved: (id: string): void => {
      const entry = get().workspaces.get(id)
      if (!entry) return
      unpublishedIds.delete(id)
      knownRefs.delete(id)
      // Stop watching this workspace's file and forget its sync bookkeeping.
      const sync = wsSync.get(id)
      if (sync) {
        clearWatchRetry(sync)
        if (sync.unsubscribe) sync.unsubscribe()
      }
      wsSync.delete(id)
      if (entry.status === WorkspaceEntryStatus.Loaded || entry.status === WorkspaceEntryStatus.OperationError) {
        entry.store.getState().gitController.getState().dispose()
        for (const tabId of Object.keys(entry.store.getState().appStates)) {
          const ref = entry.store.getState().getTabRef(tabId)
          if (ref) ref.dispose()
        }
        entry.store.getState().dispose()
      }
      set((s) => {
        const remaining = new Map(s.workspaces)
        remaining.delete(id)
        return {
          workspaces: remaining,
          activeWorkspaceId: s.activeWorkspaceId === id ? undefined : s.activeWorkspaceId
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
      void deps.git.getInfo(path).then(async gitInfo => {
        const appStates: Record<string, AppState> = {}
        let activeTabId: string | undefined = undefined

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
          parentId: undefined,
          status: WorkspaceStatus.Active,
          isGitRepo: gitInfo.isRepo,
          gitBranch: gitInfo.isRepo ? gitInfo.branch : undefined,
          gitRootPath: gitInfo.isRepo ? gitInfo.rootPath : undefined,
          isWorktree: false,
          isDetached: false,
          appStates,
          activeTabId,
          metadata: { sortOrder: nextSortOrder(undefined) },
          settings: options?.settings ?? defaultWorktreeSettings,
          favouritePaths: [],
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
        await createWorkspaceFile(id, workspace.path)
        void enqueueSync('addWorkspace')
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

    adoptExistingWorktree: async (parentId: string, worktreePath: string, branch: string, name: string, settings?: WorktreeSettings, description?: string, displayName?: string) => {
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
        const metadata: Record<string, string> = {
          branchIsUserDefined: 'true',
          ...(description ? { description } : {}),
          ...(displayName ? { displayName } : {}),
        }
        await addChildWorkspaceFromResult(parentId, name, worktreePath, branch, { settings, metadata })
        return { success: true }
      } finally {
        await releaseLock().catch((e: unknown) => { console.error('[session] failed to unlock session:', e) })
      }
    },

    autoOpenWorktrees: async (rootWorkspaceId: string, items: AutoOpenWorktreeItem[]) => {
      const rootEntry = get().workspaces.get(rootWorkspaceId)
      if (!rootEntry || (rootEntry.status !== WorkspaceEntryStatus.Loaded && rootEntry.status !== WorkspaceEntryStatus.OperationError)) {
        return { success: false, error: 'Root workspace not found' }
      }
      const rootPath = rootEntry.data.path

      const lockStatus = await acquireLock()
      if (!lockStatus.acquired) {
        return { success: false, error: lockStatus.error }
      }

      try {
        // Seed path → id with every currently-open workspace so children of an already-open
        // worktree attach correctly, and children of newly-created ones can resolve as we go.
        const pathToId = new Map<string, string>()
        for (const e of Array.from(get().workspaces.values())) {
          if (e.status === WorkspaceEntryStatus.Loaded || e.status === WorkspaceEntryStatus.OperationError) {
            pathToId.set(e.data.path, e.data.id)
          }
        }

        // Topologically order: an item can be created once its parent (root, an open
        // workspace, or an earlier item) is known. Items with a missing/cyclic parent are
        // appended last and fall back to the root anchor at creation time.
        const knownPaths = new Set<string>(pathToId.keys())
        knownPaths.add(rootPath)
        const ordered: AutoOpenWorktreeItem[] = []
        const remaining = [...items]
        let progressed = true
        while (remaining.length > 0 && progressed) {
          progressed = false
          for (let i = remaining.length - 1; i >= 0; i--) {
            const it = remaining[i]
            if (!it) continue
            if (it.parentPath === null || knownPaths.has(it.parentPath)) {
              ordered.push(it)
              knownPaths.add(it.path)
              remaining.splice(i, 1)
              progressed = true
            }
          }
        }
        for (const it of remaining) ordered.push(it)

        for (const it of ordered) {
          const alreadyOpen = Array.from(get().workspaces.values()).some(
            e => (e.status === WorkspaceEntryStatus.Loaded || e.status === WorkspaceEntryStatus.OperationError) && e.data.path === it.path
          )
          if (alreadyOpen) continue

          const parentId = (it.parentPath ? pathToId.get(it.parentPath) : undefined) ?? rootWorkspaceId
          const metadata: Record<string, string> = {
            branchIsUserDefined: 'true',
            ...(it.displayName ? { displayName: it.displayName } : {}),
            ...(it.description ? { description: it.description } : {}),
          }
          const newId = await addChildWorkspaceFromResult(parentId, it.name, it.path, it.branch, { metadata })
          pathToId.set(it.path, newId)
        }

        return { success: true }
      } finally {
        await releaseLock().catch((e: unknown) => { console.error('[session] failed to unlock session:', e) })
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

    createWorktreeFromPr: (parentId: string, headRefName: string, title: string, isDetached: boolean, settings?: WorktreeSettings) => {
      console.log('[session] createWorktreeFromPr called:', { parentId, headRefName, title, isDetached })
      const parentEntry = get().workspaces.get(parentId)
      const parent = parentEntry && (parentEntry.status === WorkspaceEntryStatus.Loaded || parentEntry.status === WorkspaceEntryStatus.OperationError) ? parentEntry.data : undefined

      if (!parent) {
        return { success: false, error: 'Parent workspace not found' }
      }

      if (!parent.isGitRepo || !parent.gitRootPath) {
        return { success: false, error: 'Parent workspace is not a git repository' }
      }

      const gitRootPath = parent.gitRootPath
      const remoteBranch = `origin/${headRefName}`
      const worktreeName = headRefName.split('/').pop() || headRefName
      return createChildWithLoading(parentId, worktreeName, {
        isDetached, settings,
        // The PR title becomes the workspace title (displayName); the PR body is
        // intentionally not used as the description since it can be very long.
        displayName: title,
        initialBranch: remoteBranch,
        message: 'Creating worktree from pull request...',
        // Fetch first so the PR head exists as a remote-tracking ref locally.
        preOperation: async () => { await deps.git.fetch(gitRootPath) },
        gitOperation: (onProgress) => deps.git.createWorktreeFromRemote(
          gitRootPath,
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

    setActiveWorkspace: (id: string | undefined) => {
      set({ activeWorkspaceId: id })
      if (id) {
        const entry = get().workspaces.get(id)
        if (entry && (entry.status === WorkspaceEntryStatus.Loaded || entry.status === WorkspaceEntryStatus.OperationError)) {
          void entry.store.getState().gitController.getState().refreshGit()
        }
      }
    },

    updateGitInfo: (id: string, gitInfo: GitInfo) => {
      const entry = get().workspaces.get(id)
      if (!entry || (entry.status !== WorkspaceEntryStatus.Loaded && entry.status !== WorkspaceEntryStatus.OperationError)) return
      const current = entry.store.getState().workspace
      const gitBranch = gitInfo.isRepo ? gitInfo.branch : undefined
      const gitRootPath = gitInfo.isRepo ? gitInfo.rootPath : undefined
      // Git refreshes fire on terminal activity, pane focus and review actions. An
      // unchanged result must not write: parentHash chaining makes any write a new
      // body, which every other window would then apply as an external edit.
      if (current.isGitRepo === gitInfo.isRepo && current.gitBranch === gitBranch && current.gitRootPath === gitRootPath) return
      entry.store.getState().setWorkspace({ ...current, isGitRepo: gitInfo.isRepo, gitBranch, gitRootPath })
      enqueueContentSync(id)
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
          entry.store.getState().updateStatus(WorkspaceStatus.Merged)
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

        // Refresh parent's git status after merge
        const wsData = entry && (entry.status === WorkspaceEntryStatus.Loaded || entry.status === WorkspaceEntryStatus.OperationError) ? entry.data : undefined
        if (wsData?.parentId) {
          const parentEntry = get().workspaces.get(wsData.parentId)
          if (parentEntry && (parentEntry.status === WorkspaceEntryStatus.Loaded || parentEntry.status === WorkspaceEntryStatus.OperationError)) {
            void parentEntry.store.getState().gitController.getState().refreshGit()
          }
        }

        return { success: true }
      } finally {
        await releaseLock().catch((e: unknown) => { console.error('[session] failed to unlock session:', e) })
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

        // Refresh workspace git status and git info
        const currentEntry = get().workspaces.get(id)
        if (currentEntry && currentEntry.status === WorkspaceEntryStatus.Loaded) {
          void currentEntry.store.getState().gitController.getState().refreshGit()
        }
        void get().refreshGitInfo(id)

        // Refresh parent's git status after merge
        if (currentEntry && currentEntry.status === WorkspaceEntryStatus.Loaded && currentEntry.data.parentId) {
          const parentEntry = get().workspaces.get(currentEntry.data.parentId)
          if (parentEntry && (parentEntry.status === WorkspaceEntryStatus.Loaded || parentEntry.status === WorkspaceEntryStatus.OperationError)) {
            void parentEntry.store.getState().gitController.getState().refreshGit()
          }
        }

        return { success: true }
      } finally {
        await releaseLock().catch((e: unknown) => { console.error('[session] failed to unlock session:', e) })
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

      // The fork creates a flat branch named after the candidate (see
      // createWorktree), so dedup against the candidate itself — not a
      // `parentBranch/candidate` form that is never actually created.
      let name: string | undefined = undefined
      for (let i = 0; i < 3; i++) {
        const candidate = humanId({ separator: '-', capitalize: false })
        if (!existingBranches.includes(candidate)) {
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
        const isMatch = dragParent ===undefined ? !entry.data.parentId : entry.data.parentId === dragParent
        if (isMatch) siblings.push({ id, entry })
      }
      siblings.sort((a, b) =>
        parseInt(a.entry.store.getState().metadata.sortOrder || '0') - parseInt(b.entry.store.getState().metadata.sortOrder || '0')
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
        const newMetadata = { ...entry.store.getState().metadata, sortOrder: String(i) }
        const newData = { ...entry.data, metadata: newMetadata }
        entry.store.getState().setWorkspace(newData)
        // Also update the session store snapshot
        set(s => ({
          workspaces: new Map(s.workspaces).set(id, { ...entry, data: newData })
        }))
      }

      syncAllContent()
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
        let current: string | undefined = startId
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
        const newData = { ...dragEntry.data, parentId: targetWorkspaceId, metadata: { ...dragEntry.store.getState().metadata, sortOrder: newSortOrder } }

        dragEntry.store.getState().setWorkspace(newData)
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
          const isMatch = newParentId ===undefined ? !entry.data.parentId : entry.data.parentId === newParentId
          if (isMatch && id !== workspaceId) newSiblings.push({ id, entry })
        }
        newSiblings.sort((a, b) => parseInt(a.entry.store.getState().metadata.sortOrder || '0') - parseInt(b.entry.store.getState().metadata.sortOrder || '0'))

        const targetIdx = newSiblings.findIndex(s => s.id === targetWorkspaceId)
        const insertIdx = position === 'before' ? targetIdx : targetIdx + 1
        newSiblings.splice(insertIdx, 0, { id: workspaceId, entry: dragEntry })

        // Update all new siblings' sortOrder, and parentId on the dragged item
        for (let i = 0; i < newSiblings.length; i++) {
          const item = newSiblings[i]
          if (!item) continue
          const { id, entry } = item
          const isTheDraggedItem = id === workspaceId
          const newMetadata = { ...entry.store.getState().metadata, sortOrder: String(i) }
          const parentId = isTheDraggedItem ? newParentId : entry.data.parentId
          const newData = { ...entry.data, parentId, metadata: newMetadata }

          entry.store.getState().setWorkspace(newData)
          set(s => ({
            workspaces: new Map(s.workspaces).set(id, { ...entry, data: newData })
          }))
        }

        reindexSiblings(dragParent, workspaceId)
      }

      syncAllContent()
    },

    syncToDaemon: async (reason: string) => {
      await enqueueSync(reason)
    },

    flushDeferredWrites,

    forceUnlock: async () => {
      const result = await deps.sessionApi.forceUnlock(store.getState().connection.id)
      if (!result.success) return { success: false, error: result.error }
      set({ sessionLock: undefined })
      return { success: true }
    },

    handleRestore: async (daemonSession: Session) => {
      console.log('[Session] Restoring session', daemonSession.id, 'with', daemonSession.workspaceRefs.length, 'refs, version:', daemonSession.version)
      await get().handleExternalUpdate(daemonSession)

      // Activate the first ref. Parent/child structure lives in the (not-yet-loaded)
      // file bodies, so target the first ref by position; its entry may still be
      // Loading until its file event arrives — navigation by id still works.
      const firstRef = daemonSession.workspaceRefs[0]
      if (firstRef && get().workspaces.has(firstRef.id)) {
        get().setActiveWorkspace(firstRef.id)
      }
      console.log('[Session] Session restore complete, workspace count:', get().workspaces.size)
    },

    // eslint-disable-next-line @typescript-eslint/require-await -- interface requires Promise<void> but implementation is synchronous
    handleExternalUpdate: async (daemonSession: Session) => {
      const currentVersion = get().sessionVersion
      if (daemonSession.version < currentVersion) return

      // Always carry forward version/lock/dataDir.
      set({
        sessionVersion: daemonSession.version,
        sessionLock: daemonSession.lock,
        workspaceDataDir: daemonSession.workspaceDataDir,
      })

      // A ref sync rejected under another window's lock waits here for the unlock.
      // An expired lock is only cleared lazily by the daemon, so treat it as gone too.
      if (refSyncRetryOnNextEvent && (daemonSession.lock === undefined || daemonSession.lock.expiresAt <= Date.now())) {
        refSyncRetryOnNextEvent = false
        void enqueueSync('retry after lock released')
      }

      const incomingRefsJson = stableStringify(daemonSession.workspaceRefs)
      if (incomingRefsJson === lastSyncedRefsJson) {
        // Membership unchanged (a content echo or lock-only change). Workspace
        // bodies arrive via their own file watches — nothing to reconcile here.
        return
      }
      lastSyncedRefsJson = incomingRefsJson

      console.log('[Session] External ref update received, version:', daemonSession.version, 'refs:', daemonSession.workspaceRefs.length)
      reconcileRefs(daemonSession.workspaceRefs)
    },

    dispose: (): void => {
      // Flip the guard first so any watch callback racing the unsubscribe below is a
      // no-op, then stop every file watch and forget the sync bookkeeping — this is
      // the leak that left ghost stores writing the same JSON files as their
      // replacement after a reconnect. Finally dispose each workspace's resources,
      // mirroring the per-workspace teardown in onWorkspaceRemoved.
      disposed = true
      for (const sync of Array.from(wsSync.values())) {
        clearWatchRetry(sync)
        if (sync.unsubscribe) sync.unsubscribe()
      }
      wsSync.clear()
      for (const entry of Array.from(get().workspaces.values())) {
        if (entry.status === WorkspaceEntryStatus.Loaded || entry.status === WorkspaceEntryStatus.OperationError) {
          entry.store.getState().gitController.getState().dispose()
          for (const tabId of Object.keys(entry.store.getState().appStates)) {
            const ref = entry.store.getState().getTabRef(tabId)
            if (ref) ref.dispose()
          }
          entry.store.getState().dispose()
        }
      }
    }
  }))

  return store
}

// Apply a workspace body (from a file-watch event) into the session store. Updates
// the handle in place when the workspace already exists, else reconstructs it.
function applyWorkspaceFile(
  store: StoreApi<SessionState>,
  workspace: Workspace,
  createHandleForWorkspace: (ws: Workspace) => WorkspaceStore
): void {
  const entry = store.getState().workspaces.get(workspace.id)

  if (entry && (entry.status === WorkspaceEntryStatus.Loaded || entry.status === WorkspaceEntryStatus.OperationError)) {
    const wsState = entry.store.getState()
    const oldTabIds = Object.keys(wsState.workspace.appStates)
    const newTabIds = Object.keys(workspace.appStates)
    const newTabIdSet = new Set(newTabIds)

    // Dispose resources for tabs removed externally
    for (const tabId of oldTabIds) {
      if (!newTabIdSet.has(tabId)) {
        wsState.disposeTabResources(tabId)
      }
    }

    const reconciledActiveTabId = workspace.activeTabId || newTabIds[0] || undefined
    entry.store.getState().setWorkspace({ ...workspace, activeTabId: reconciledActiveTabId })

    // Only init genuinely new tabs
    const oldTabIdSet = new Set(oldTabIds)
    for (const tabId of newTabIds) {
      if (!oldTabIdSet.has(tabId)) {
        entry.store.getState().initTab(tabId)
      }
    }
    return
  }

  reconstructWorkspace(store, workspace, createHandleForWorkspace)
}

// Helper: reconstruct a workspace from its file body, preserving its id.
function reconstructWorkspace(
  store: StoreApi<SessionState>,
  fileWorkspace: Workspace,
  createHandleForWorkspace: (ws: Workspace) => WorkspaceStore
): string {
  const id = fileWorkspace.id
  const reconstructedActiveTabId = fileWorkspace.activeTabId || (Object.keys(fileWorkspace.appStates).length > 0 ? Object.keys(fileWorkspace.appStates)[0] : undefined)
  const workspace: Workspace = { ...fileWorkspace, activeTabId: reconstructedActiveTabId }

  const handle = createHandleForWorkspace(workspace)

  store.setState((s) => ({
    workspaces: new Map(s.workspaces).set(id, { status: WorkspaceEntryStatus.Loaded, data: workspace, store: handle }),
    activeWorkspaceId: s.activeWorkspaceId ?? id
  }))

  for (const tabId of Object.keys(fileWorkspace.appStates)) {
    handle.getState().initTab(tabId)
  }

  console.log('[Session] Reconstructed workspace:', fileWorkspace.name, 'parentId:', fileWorkspace.parentId)
  return id
}
