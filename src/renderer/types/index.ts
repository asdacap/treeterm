import type { ReactNode } from 'react'
import type { WorkspaceStore, TerminalAppRef, CachedTerminal } from '../store/createWorkspaceStore'
export type { WorkspaceStore, TerminalAppRef, CachedTerminal }

// Import and re-export shared types
import {
  FileChangeStatus,
} from '../../shared/types'
import type {
  SandboxConfig,
  AppState,
  Workspace,
  Session,
  TTYSessionInfo,
  WorkspaceInput,
  TerminalInstance,
  AiHarnessInstance,
  CustomRunnerInstance,
  PrefixModeConfig,
  Settings,
  WorktreeSettings,
  RunAction,
  SSHConnectionConfig,
  ConnectionInfo,
  PortForwardSpec,
  PortForwardConfig,
  PortForwardInfo,
  ReasoningEffort
} from '../../shared/types'
export { FileChangeStatus }
import type { PtyEvent, ExecEvent, IpcResult } from '../../shared/ipc-types'
export type { PtyEvent, ExecEvent, IpcResult }

export type {
  SandboxConfig,
  AppState,
  Workspace,
  Session,
  TTYSessionInfo,
  WorkspaceInput,
  TerminalInstance,
  AiHarnessInstance,
  CustomRunnerInstance,
  PrefixModeConfig,
  Settings,
  WorktreeSettings,
  RunAction,
  SSHConnectionConfig,
  ConnectionInfo,
  PortForwardSpec,
  PortForwardConfig,
  PortForwardInfo,
  ReasoningEffort
}

/** Convenience type: AppState with its id (the map key) */
export type Tab = AppState & { id: string }

export enum ScrollPosition {
  Top = 'top',
  Bottom = 'bottom',
  Middle = 'middle',
}

// Activity state for applications that can report their state
export enum ActivityState {
  Idle = 'idle',
  Working = 'working',
  UserInputRequired = 'user_input_required',
  PermissionRequest = 'permission_request',
  SafePermissionRequested = 'safe_permission_requested',
  Completed = 'completed',
  Error = 'error',
}

/**
 * Non-serialized per-tab runtime state. Lives in the workspace store closure,
 * survives component mount/unmount. Created once per tab in onWorkspaceLoad.
 *
 * close() — Called when the user explicitly closes the tab in this window.
 *           Kills daemon-side resources (e.g. PTY process). Prefer not to
 *           clean up renderer resources here to prevent unexpected bugs.
 *
 * dispose() — Client-side resource cleanup (analyzers, activity state, etc.).
 *             The tab may be removed from another window, so this must be
 *             safe to call independently of close().
 */
export interface AppRef {
  close: () => void
  dispose: () => void
}

// Application - code-defined, registered at runtime
export interface Application<TState = unknown, TRef extends AppRef = AppRef> {
  id: string
  name: string
  icon: string
  createInitialState: () => TState
  onWorkspaceLoad: (tab: Tab, workspaceStore: WorkspaceStore) => TRef
  render: (props: ApplicationRenderProps) => ReactNode
  canClose: boolean
  showInNewTabMenu: boolean
  // CSS display style when visible: 'block' or 'flex'
  displayStyle: 'block' | 'flex'
  // Whether this app creates tabs automatically in new workspaces
  isDefault: boolean
  // Optional: Applications can report their activity state
  getActivityState?: (tab: Tab) => ActivityState
}

export interface ApplicationRenderProps {
  tab: Tab
  workspace: WorkspaceStore
  isVisible: boolean
}

// Type-specific state interfaces (for internal use within applications)
export interface TerminalState {
  ptyId: string | null       // daemon sessionId — persisted for reconnection
  ptyHandle: string | null   // ephemeral stream handle — used for write/resize/onData/onExit
  connectionId?: string      // which connection this PTY belongs to — used for routing kill
  keepOnExit: boolean
}

export type AiHarnessState = TerminalState & {
  sandbox: SandboxConfig
  autoApprove: boolean
}

export interface FilesystemState {
  selectedPath: string | null
  expandedDirs: string[]
  scrollToLine?: number
  scrollTop?: number
}

export interface ReviewState {
  // parentWorkspaceId identifies the target branch for merging.
  // If undefined/null, this is a top-level worktree - review shows only uncommitted changes
  parentWorkspaceId?: string
  viewMode: 'committed' | 'uncommitted' | 'commits'
  selectedFilePath?: string
  selectedUncommittedFilePath?: string
  scrollTop?: number
}

// eslint-disable-next-line @typescript-eslint/no-empty-object-type -- intentional marker interface for tab state
export interface CommentsState {
  // empty - no persisted state needed
}

export interface ChatMessage {
  id: string
  role: 'user' | 'assistant'
  content: string
}

export interface ChatState {
  messages: ChatMessage[]
}

export enum EditorStatus {
  Loading = 'loading',
  Ready = 'ready',
  Error = 'error',
}

export enum EditorViewMode {
  Editor = 'editor',
  Preview = 'preview',
}

export enum TtyCreationStatus {
  Loading = 'loading',
  Ready = 'ready',
  Error = 'error',
}

export enum PtyViewerStatus {
  Loading = 'loading',
  Ready = 'ready',
  Error = 'error',
}

export type EditorState =
  | { status: EditorStatus.Loading; filePath: string; scrollToLine?: number }
  | { status: EditorStatus.Ready; filePath: string; originalContent: string; currentContent: string; language: string; isDirty: boolean; viewMode: EditorViewMode; scrollTop?: number; scrollToLine?: number }
  | { status: EditorStatus.Error; filePath: string; error: string }

export interface FileEntry {
  name: string
  path: string
  relativePath: string
  isDirectory: boolean
  size?: number
  modifiedTime?: number
}

export interface DirectoryContents {
  path: string
  entries: FileEntry[]
}

export interface FileContents {
  path: string
  content: string
  size: number
  language: string
}

export interface FilesystemApi {
  readDirectory: (workspacePath: string, dirPath: string) => Promise<IpcResult<{ contents: DirectoryContents }>>
  readFile: (workspacePath: string, filePath: string) => Promise<IpcResult<{ file: FileContents }>>
  writeFile: (workspacePath: string, filePath: string, content: string) => Promise<IpcResult>
  searchFiles: (workspacePath: string, query: string) => Promise<IpcResult<{ entries: FileEntry[] }>>
}

/** Workspace-scoped FilesystemApi with path pre-bound */
export interface WorkspaceFilesystemApi {
  readDirectory: (dirPath: string) => Promise<IpcResult<{ contents: DirectoryContents }>>
  readFile: (filePath: string) => Promise<IpcResult<{ file: FileContents }>>
  writeFile: (filePath: string, content: string) => Promise<IpcResult>
  searchFiles: (query: string) => Promise<IpcResult<{ entries: FileEntry[] }>>
}

export interface ExecApi {
  start: (connectionId: string, cwd: string, command: string, args: string[]) => Promise<IpcResult<{ execId: string }>>
  kill: (execId: string) => void
  onEvent: (execId: string, callback: (event: ExecEvent) => void) => () => void
}

export type GitInfo =
  | { isRepo: false }
  | { isRepo: true; branch: string; rootPath: string }

export type WorktreeResult = IpcResult<{ path: string; branch: string }>

export interface WorktreeInfo {
  path: string
  branch: string
}

export interface BranchInfo {
  name: string
  isInWorktree: boolean
  worktreePath?: string
}

export interface DiffFile {
  path: string
  status: FileChangeStatus
  additions: number
  deletions: number
}

export interface DiffResult {
  files: DiffFile[]
  totalAdditions: number
  totalDeletions: number
  baseBranch: string
  headBranch: string
}

export interface ConflictInfo {
  hasConflicts: boolean
  conflictedFiles: string[]
  messages: string[]
}

export interface UncommittedFile {
  path: string
  status: FileChangeStatus
  staged: boolean
  additions: number
  deletions: number
}

export interface UncommittedChanges {
  files: UncommittedFile[]
  totalAdditions: number
  totalDeletions: number
}

export interface FileDiffContents {
  originalContent: string
  modifiedContent: string
  language: string
}

export interface GitLogCommit {
  hash: string
  shortHash: string
  author: string
  date: string
  message: string
  parentHashes: string[]
}

export interface GitLogResult {
  commits: GitLogCommit[]
  hasMore: boolean
}

export interface ReviewComment {
  id: string
  filePath: string
  lineNumber: number
  text: string
  commitHash: string | null
  createdAt: number
  isOutdated: boolean
  addressed: boolean
  side: 'original' | 'modified'
}

export interface ReviewsData {
  version: 1
  comments: ReviewComment[]
}

export type ConflictCheckResult = IpcResult<{ conflicts: ConflictInfo }>

export interface ClipboardApi {
  writeText: (text: string) => void
  readText: () => Promise<string>
}

export interface TerminalApi {
  create: (connectionId: string, handle: string, cwd: string, sandbox?: SandboxConfig, startupCommand?: string) => Promise<IpcResult<{ sessionId: string }>>
  attach: (connectionId: string, handle: string, sessionId: string) => Promise<IpcResult>
  list: (connectionId: string) => Promise<TTYSessionInfo[]>
  write: (handle: string, data: string) => void
  resize: (handle: string, cols: number, rows: number) => void
  kill: (connectionId: string, sessionId: string) => void
  onEvent: (handle: string, callback: (event: PtyEvent) => void) => () => void
  onActiveProcessesOpen: (callback: () => void) => () => void
  createSession: (connectionId: string, cwd: string, startupCommand?: string) => Promise<IpcResult<{ sessionId: string }>>
}

export interface GitApi {
  getInfo: (dirPath: string) => Promise<GitInfo>
  createWorktree: (repoPath: string, name: string, baseBranch?: string, onProgress?: (data: string) => void) => Promise<WorktreeResult>
  removeWorktree: (repoPath: string, worktreePath: string, deleteBranch?: boolean, onProgress?: (data: string) => void) => Promise<IpcResult>
  listWorktrees: (repoPath: string) => Promise<WorktreeInfo[]>
  listLocalBranches: (repoPath: string) => Promise<string[]>
  listRemoteBranches: (repoPath: string) => Promise<string[]>
  getBranchesInWorktrees: (repoPath: string) => Promise<string[]>
  createWorktreeFromBranch: (repoPath: string, branch: string, worktreeName: string, onProgress?: (data: string) => void) => Promise<WorktreeResult>
  createWorktreeFromRemote: (repoPath: string, remoteBranch: string, worktreeName: string, onProgress?: (data: string) => void) => Promise<WorktreeResult>
  getDiff: (worktreePath: string, parentBranch: string) => Promise<IpcResult<{ diff: DiffResult }>>
  getFileDiff: (worktreePath: string, parentBranch: string, filePath: string) => Promise<IpcResult<{ diff: string }>>
  checkMergeConflicts: (repoPath: string, sourceBranch: string, targetBranch: string) => Promise<ConflictCheckResult>
  merge: (targetWorktreePath: string, worktreeBranch: string, squash?: boolean, onProgress?: (data: string) => void) => Promise<IpcResult>
  hasUncommittedChanges: (repoPath: string) => Promise<boolean>
  commitAll: (repoPath: string, message: string) => Promise<IpcResult>
  deleteBranch: (repoPath: string, branchName: string, onProgress?: (data: string) => void) => Promise<IpcResult>
  renameBranch: (repoPath: string, oldName: string, newName: string) => Promise<IpcResult>
  getUncommittedChanges: (repoPath: string) => Promise<IpcResult<{ changes: UncommittedChanges }>>
  getUncommittedFileDiff: (repoPath: string, filePath: string, staged: boolean) => Promise<IpcResult<{ diff: string }>>
  stageFile: (repoPath: string, filePath: string) => Promise<IpcResult>
  unstageFile: (repoPath: string, filePath: string) => Promise<IpcResult>
  stageAll: (repoPath: string) => Promise<IpcResult>
  unstageAll: (repoPath: string) => Promise<IpcResult>
  commitStaged: (repoPath: string, message: string) => Promise<IpcResult>
  getFileContentsForDiff: (worktreePath: string, parentBranch: string, filePath: string) => Promise<IpcResult<{ contents: FileDiffContents }>>
  getUncommittedFileContentsForDiff: (repoPath: string, filePath: string, staged: boolean) => Promise<IpcResult<{ contents: FileDiffContents }>>
  getHeadCommitHash: (repoPath: string) => Promise<IpcResult<{ hash: string }>>
  getLog: (repoPath: string, parentBranch: string | null, skip: number, limit: number) => Promise<IpcResult<{ result: GitLogResult }>>
  getCommitDiff: (repoPath: string, commitHash: string) => Promise<IpcResult<{ files: DiffFile[] }>>
  getCommitFileDiff: (repoPath: string, commitHash: string, filePath: string) => Promise<IpcResult<{ contents: FileDiffContents }>>
  fetch: (repoPath: string) => Promise<IpcResult>
  pull: (repoPath: string) => Promise<IpcResult>
  getBehindCount: (repoPath: string) => Promise<number>
  getRemoteUrl: (repoPath: string) => Promise<IpcResult<{ url: string }>>
}

export interface GitHubReviewThread {
  isResolved: boolean
  path: string
  body: string
  author: string
  line: number | null
}

export interface GitHubReview {
  author: string
  state: 'APPROVED' | 'CHANGES_REQUESTED' | 'COMMENTED' | 'PENDING' | 'DISMISSED'
}

export interface GitHubCheckRun {
  name: string
  status: 'COMPLETED' | 'IN_PROGRESS' | 'QUEUED' | 'WAITING' | 'PENDING' | 'REQUESTED'
  conclusion: 'SUCCESS' | 'FAILURE' | 'NEUTRAL' | 'CANCELLED' | 'TIMED_OUT' | 'ACTION_REQUIRED' | 'SKIPPED' | null
}

export interface GitHubPrInfo {
  number: number
  url: string
  title: string
  state: 'OPEN' | 'CLOSED' | 'MERGED'
  reviews: GitHubReview[]
  checkRuns: GitHubCheckRun[]
  unresolvedThreads: GitHubReviewThread[]
  unresolvedCount: number
}

export type GitHubPrInfoResult = { prInfo: GitHubPrInfo } | { noPr: true; createUrl: string } | { error: string }

export interface GitHubApi {
  getPrInfo: (repoPath: string, head: string, base: string) => Promise<GitHubPrInfoResult>
}

/** Prepend connectionId to a function's parameter list */
type PrependConnectionId<F> = F extends (...args: infer A) => infer R
  ? (connectionId: string, ...args: A) => R
  : F

/** Version of an API where every method receives connectionId as the first argument */
type WithConnectionId<T> = {
  [K in keyof T]: PrependConnectionId<T[K]>
}

/** Raw filesystem API exposed by the preload — connectionId is the first parameter of every method */
export type RawFilesystemApi = WithConnectionId<FilesystemApi>

/** Generic helper: bind connectionId to all methods of a raw (WithConnectionId) API */
function bindConnectionId<T extends object>(raw: WithConnectionId<T>, connectionId: string): T {
  return Object.fromEntries(
    Object.entries(raw).map(([key, fn]) => {
      return [key, (...args: unknown[]) => (fn as (connId: string, ...rest: unknown[]) => unknown)(connectionId, ...args)]
    })
  ) as unknown as T
}

/** Bind a connectionId to a RawFilesystemApi, returning a FilesystemApi scoped to that connection */
export function createBoundFilesystem(raw: RawFilesystemApi, connectionId: string): FilesystemApi {
  return bindConnectionId<FilesystemApi>(raw, connectionId)
}

// eslint-disable-next-line @typescript-eslint/no-empty-object-type -- intentional marker interface for tab state
export interface GitHubAppState {
  // empty — reads prInfo from workspace store
}

/** Workspace-scoped GitApi with path pre-bound */
export interface WorkspaceGitApi {
  getInfo: () => Promise<GitInfo>
  createWorktree: (name: string, baseBranch?: string) => Promise<WorktreeResult>
  removeWorktree: (worktreePath: string, deleteBranch?: boolean, onProgress?: (data: string) => void) => Promise<IpcResult>
  listWorktrees: () => Promise<WorktreeInfo[]>
  listLocalBranches: () => Promise<string[]>
  listRemoteBranches: () => Promise<string[]>
  getBranchesInWorktrees: () => Promise<string[]>
  createWorktreeFromBranch: (branch: string, worktreeName: string) => Promise<WorktreeResult>
  createWorktreeFromRemote: (remoteBranch: string, worktreeName: string) => Promise<WorktreeResult>
  getDiff: (parentBranch: string) => Promise<IpcResult<{ diff: DiffResult }>>
  getFileDiff: (parentBranch: string, filePath: string) => Promise<IpcResult<{ diff: string }>>
  checkMergeConflicts: (sourceBranch: string, targetBranch: string) => Promise<ConflictCheckResult>
  merge: (worktreeBranch: string, squash?: boolean, onProgress?: (data: string) => void) => Promise<IpcResult>
  hasUncommittedChanges: () => Promise<boolean>
  commitAll: (message: string) => Promise<IpcResult>
  deleteBranch: (branchName: string, onProgress?: (data: string) => void) => Promise<IpcResult>
  getUncommittedChanges: () => Promise<IpcResult<{ changes: UncommittedChanges }>>
  getUncommittedFileDiff: (filePath: string, staged: boolean) => Promise<IpcResult<{ diff: string }>>
  stageFile: (filePath: string) => Promise<IpcResult>
  unstageFile: (filePath: string) => Promise<IpcResult>
  stageAll: () => Promise<IpcResult>
  unstageAll: () => Promise<IpcResult>
  commitStaged: (message: string) => Promise<IpcResult>
  getFileContentsForDiff: (parentBranch: string, filePath: string) => Promise<IpcResult<{ contents: FileDiffContents }>>
  getUncommittedFileContentsForDiff: (filePath: string, staged: boolean) => Promise<IpcResult<{ contents: FileDiffContents }>>
  getHeadCommitHash: () => Promise<IpcResult<{ hash: string }>>
  getLog: (parentBranch: string | null, skip: number, limit: number) => Promise<IpcResult<{ result: GitLogResult }>>
  getCommitDiff: (commitHash: string) => Promise<IpcResult<{ files: DiffFile[] }>>
  getCommitFileDiff: (commitHash: string, filePath: string) => Promise<IpcResult<{ contents: FileDiffContents }>>
  fetch: () => Promise<IpcResult>
  pull: () => Promise<IpcResult>
  getBehindCount: () => Promise<number>
}

export interface SettingsApi {
  load: () => Promise<Settings>
  save: (settings: Settings) => Promise<{ success: boolean }>
  onOpen: (callback: () => void) => () => void
}

export interface AppRegistryApi {
  get: (id: string) => Application | undefined
  getDefaultApp: (appId?: string) => Application | null
}

export interface RunActionsApi {
  detect: (workspacePath: string) => Promise<RunAction[]>
  run: (workspacePath: string, actionId: string) => Promise<IpcResult<{ ptyId: string }>>
}

export interface SandboxApi {
  isAvailable: () => Promise<boolean>
}

export interface AppApi {
  onReady: (callback: () => void) => () => void
  localConnect: (windowUuid: string) => Promise<{ info: ConnectionInfo; session: Session }>
  onCloseConfirm: (callback: () => void) => () => void
  confirmClose: () => void
  cancelClose: () => void
  onCapsLockEvent: (callback: (event: { type: string; key: string; code: string }) => void) => () => void
  onSshAutoConnected: (callback: (session: Session, connection: ConnectionInfo) => void) => () => void
  onConnectionReconnected: (callback: (session: Session, connection: ConnectionInfo) => void) => () => void
}

export interface DaemonApi {
  shutdown: (connectionId: string) => Promise<IpcResult>
  onSessions: (callback: (sessions: TTYSessionInfo[]) => void) => () => void
  onDisconnected: (callback: () => void) => () => void
}

export interface LlmApi {
  send: (requestId: string, messages: { role: 'user' | 'assistant' | 'system'; content: string }[], settings: { baseUrl: string; apiKey: string; model: string; reasoning: ReasoningEffort }) => Promise<void>
  analyzeTerminal: (buffer: string, cwd: string, settings: { baseUrl: string; apiKey: string; model: string; systemPrompt: string; reasoningEffort: ReasoningEffort; safePaths: string[] }) => Promise<{ state: string; reason: string; cached?: boolean; systemPrompt?: string } | { error: string; systemPrompt?: string }>
  clearAnalyzerCache: () => Promise<void>
  generateTitle: (buffer: string, settings: { baseUrl: string; apiKey: string; model: string; titleSystemPrompt: string; reasoningEffort: ReasoningEffort }) => Promise<{ title: string; description: string; branchName: string; systemPrompt?: string } | { error: string; systemPrompt?: string }>
  cancel: (requestId: string) => void
  onDelta: (callback: (requestId: string, text: string) => void) => () => void
  onDone: (callback: (requestId: string) => void) => () => void
  onError: (callback: (requestId: string, error: string) => void) => () => void
}

export interface SSHApi {
  connect: (config: SSHConnectionConfig, options?: { refreshDaemon?: boolean; allowOutdatedDaemon?: boolean }) => Promise<{ info: ConnectionInfo; session: Session | null }>
  disconnect: (connectionId: string) => Promise<void>
  reconnect: (connectionId: string) => Promise<void>
  reconnectNow: (connectionId: string) => Promise<void>
  forceReconnect: (connectionId: string) => Promise<void>
  cancelReconnect: (connectionId: string) => Promise<void>
  listConnections: () => Promise<ConnectionInfo[]>
  saveConnection: (config: SSHConnectionConfig) => Promise<void>
  getSavedConnections: () => Promise<SSHConnectionConfig[]>
  removeSavedConnection: (id: string) => Promise<void>
  onConnectionStatus: (callback: (info: ConnectionInfo) => void) => () => void
  onBootstrapOutput: (callback: (connectionId: string, line: string) => void) => () => void
  onTunnelOutput: (callback: (connectionId: string, line: string) => void) => () => void
  onDaemonOutput: (callback: (connectionId: string, line: string) => void) => () => void
  watchBootstrapOutput: (connectionId: string, cb: (line: string) => void) => Promise<{ scrollback: string[]; unsubscribe: () => void }>
  watchTunnelOutput: (connectionId: string, cb: (line: string) => void) => Promise<{ scrollback: string[]; unsubscribe: () => void }>
  watchDaemonOutput: (connectionId: string, cb: (line: string) => void) => Promise<{ scrollback: string[]; unsubscribe: () => void }>
  watchConnectionStatus: (connectionId: string, cb: (info: ConnectionInfo) => void) => Promise<{ initial: ConnectionInfo; unsubscribe: () => void }>
  addPortForward: (config: PortForwardConfig) => Promise<PortForwardInfo>
  removePortForward: (portForwardId: string) => Promise<void>
  listPortForwards: (connectionId: string) => Promise<PortForwardInfo[]>
  onPortForwardStatus: (callback: (info: PortForwardInfo) => void) => () => void
  watchPortForwardOutput: (portForwardId: string, cb: (line: string) => void) => Promise<{ scrollback: string[]; unsubscribe: () => void }>
}

export interface SessionApi {
  update: (sessionId: string, workspaces: WorkspaceInput[], senderUuid?: string, expectedVersion?: number) => Promise<IpcResult<{ session: Session }>>
  lock: (sessionId: string, ttlMs?: number) => Promise<IpcResult<{ acquired: boolean; session: Session }>>
  unlock: (sessionId: string) => Promise<IpcResult<{ session: Session }>>
  forceUnlock: (sessionId: string) => Promise<IpcResult<{ session: Session }>>
  onSync: (callback: (connectionId: string, session: Session) => void) => () => void
}

export enum Platform {
  Darwin = 'darwin',
  Linux = 'linux',
  Win32 = 'win32',
  Aix = 'aix',
  Android = 'android',
  Freebsd = 'freebsd',
  Haiku = 'haiku',
  Openbsd = 'openbsd',
  Sunos = 'sunos',
  Cygwin = 'cygwin',
  Netbsd = 'netbsd',
}

export type PreloadApi = {
  platform: Platform
  terminal: TerminalApi
  selectFolder: () => Promise<string | null>
  getRecentDirectories: () => Promise<string[]>
  settings: SettingsApi
  filesystem: RawFilesystemApi
  exec: ExecApi
  sandbox: SandboxApi
  getInitialWorkspace: () => Promise<string | null>
  app: AppApi
  daemon: DaemonApi
  session: SessionApi
  getWindowUuid: () => Promise<string>
  clipboard: ClipboardApi
  ssh: SSHApi
}

declare global {
  interface Window {
    electron: PreloadApi
  }
}

// Helper to derive a Tab array from appStates Record
export function getTabs(workspace: Workspace): Tab[] {
  return Object.entries(workspace.appStates).map(([id, state]) => ({ ...state, id }))
}

// Type guard functions for application states
export function isTerminalState(state: unknown): state is TerminalState {
  return (
    state !== null &&
    typeof state === 'object' &&
    'ptyId' in state &&
    (typeof (state as TerminalState).ptyId === 'string' || (state as TerminalState).ptyId === null)
  )
}

export function isAiHarnessState(state: unknown): state is AiHarnessState {
  return (
    isTerminalState(state) &&
    'sandbox' in (state as unknown as Record<string, unknown>) &&
    typeof (state as AiHarnessState).sandbox === 'object'
  )
}

export function isReviewState(state: unknown): state is ReviewState {
  return (
    state !== null &&
    typeof state === 'object' &&
    // parentWorkspaceId is optional - if present, must be a string
    (!('parentWorkspaceId' in state) ||
      typeof (state as ReviewState).parentWorkspaceId === 'string')
  )
}

export function isEditorState(state: unknown): state is EditorState {
  return (
    state !== null &&
    typeof state === 'object' &&
    'status' in state &&
    'filePath' in state &&
    ((state as { status: unknown }).status === EditorStatus.Loading ||
      (state as { status: unknown }).status === EditorStatus.Ready ||
      (state as { status: unknown }).status === EditorStatus.Error) &&
    typeof (state as EditorState).filePath === 'string'
  )
}

export function isCommentsState(state: unknown): state is CommentsState {
  return state !== null && typeof state === 'object'
}
