import type { ReactNode } from 'react'
import type { WorkspaceStore } from '../store/createWorkspaceStore'
export type { WorkspaceStore }

// Import and re-export shared types
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
  STTProvider,
  Settings,
  WorktreeSettings,
  RunAction,
  SSHConnectionConfig,
  ConnectionInfo,
  PortForwardConfig,
  PortForwardInfo,
  ReasoningEffort
} from '../../shared/types'
import type { PtyEvent, IpcResult } from '../../shared/ipc-types'
export type { PtyEvent, IpcResult }

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
  STTProvider,
  Settings,
  WorktreeSettings,
  RunAction,
  SSHConnectionConfig,
  ConnectionInfo,
  PortForwardConfig,
  PortForwardInfo,
  ReasoningEffort
}

/** Convenience type: AppState with its id (the map key) */
export type Tab = AppState & { id: string }

// Activity state for applications that can report their state
export type ActivityState =
  | 'idle'
  | 'working'
  | 'user_input_required'
  | 'permission_request'
  | 'safe_permission_requested'
  | 'completed'
  | 'error'

// Base interface for application runtime refs (non-serialized per-tab state)
export interface AppRef {
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
  connectionId?: string      // which connection this PTY belongs to — used for routing kill/isAlive
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

export type EditorState =
  | { status: 'loading'; filePath: string }
  | { status: 'ready'; filePath: string; originalContent: string; currentContent: string; language: string; isDirty: boolean; viewMode: 'editor' | 'preview'; scrollTop?: number }
  | { status: 'error'; filePath: string; error: string }

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
  status: 'added' | 'modified' | 'deleted' | 'renamed'
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
  status: 'added' | 'modified' | 'deleted' | 'renamed' | 'untracked'
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
  create: (connectionId: string, cwd: string, sandbox?: SandboxConfig, startupCommand?: string) => Promise<IpcResult<{ sessionId: string; handle: string }>>
  attach: (connectionId: string, sessionId: string) => Promise<IpcResult<{ handle: string }>>
  list: (connectionId: string) => Promise<TTYSessionInfo[]>
  write: (handle: string, data: string) => void
  resize: (handle: string, cols: number, rows: number) => void
  kill: (connectionId: string, sessionId: string) => void
  isAlive: (connectionId: string, id: string) => Promise<boolean>
  onEvent: (handle: string, callback: (event: PtyEvent) => void) => () => void
  onActiveProcessesOpen: (callback: () => void) => () => void
}

export interface GitApi {
  getInfo: (dirPath: string) => Promise<GitInfo>
  createWorktree: (repoPath: string, name: string, baseBranch?: string, operationId?: string) => Promise<WorktreeResult>
  removeWorktree: (repoPath: string, worktreePath: string, deleteBranch?: boolean, operationId?: string) => Promise<IpcResult>
  listWorktrees: (repoPath: string) => Promise<WorktreeInfo[]>
  listLocalBranches: (repoPath: string) => Promise<string[]>
  listRemoteBranches: (repoPath: string) => Promise<string[]>
  getBranchesInWorktrees: (repoPath: string) => Promise<string[]>
  createWorktreeFromBranch: (repoPath: string, branch: string, worktreeName: string, operationId?: string) => Promise<WorktreeResult>
  createWorktreeFromRemote: (repoPath: string, remoteBranch: string, worktreeName: string, operationId?: string) => Promise<WorktreeResult>
  getDiff: (worktreePath: string, parentBranch: string) => Promise<IpcResult<{ diff: DiffResult }>>
  getFileDiff: (worktreePath: string, parentBranch: string, filePath: string) => Promise<IpcResult<{ diff: string }>>
  checkMergeConflicts: (repoPath: string, sourceBranch: string, targetBranch: string) => Promise<ConflictCheckResult>
  merge: (targetWorktreePath: string, worktreeBranch: string, squash?: boolean, operationId?: string) => Promise<IpcResult>
  hasUncommittedChanges: (repoPath: string) => Promise<boolean>
  commitAll: (repoPath: string, message: string) => Promise<IpcResult>
  deleteBranch: (repoPath: string, branchName: string, operationId?: string) => Promise<IpcResult>
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
  onOutput: (callback: (operationId: string, data: string) => void) => () => void
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

/** Version of an API where every method (except onOutput) receives connectionId as the first argument */
type WithConnectionId<T> = {
  [K in keyof T]: K extends 'onOutput' ? T[K] : PrependConnectionId<T[K]>
}

/** Raw git API exposed by the preload — connectionId is the first parameter of every method */
export type RawGitApi = WithConnectionId<GitApi>

/** Raw GitHub API exposed by the preload — connectionId is the first parameter */
export type RawGitHubApi = WithConnectionId<GitHubApi>

/** Raw filesystem API exposed by the preload — connectionId is the first parameter of every method */
export type RawFilesystemApi = WithConnectionId<FilesystemApi>

/** Raw run actions API exposed by the preload — connectionId is the first parameter of every method */
export type RawRunActionsApi = WithConnectionId<RunActionsApi>

/** Generic helper: bind connectionId to all methods of a raw (WithConnectionId) API */
function bindConnectionId<T extends object>(raw: WithConnectionId<T>, connectionId: string): T {
  return Object.fromEntries(
    Object.entries(raw).map(([key, fn]) => {
      if (key === 'onOutput') return [key, fn]
      return [key, (...args: unknown[]) => (fn as (connId: string, ...rest: unknown[]) => unknown)(connectionId, ...args)]
    })
  ) as unknown as T
}

/** Bind a connectionId to a RawGitApi, returning a GitApi scoped to that connection */
export function createBoundGit(raw: RawGitApi, connectionId: string): GitApi {
  return bindConnectionId<GitApi>(raw, connectionId)
}

/** Bind a connectionId to a RawGitHubApi, returning a GitHubApi scoped to that connection */
export function createBoundGitHub(raw: RawGitHubApi, connectionId: string): GitHubApi {
  return bindConnectionId<GitHubApi>(raw, connectionId)
}

/** Bind a connectionId to a RawFilesystemApi, returning a FilesystemApi scoped to that connection */
export function createBoundFilesystem(raw: RawFilesystemApi, connectionId: string): FilesystemApi {
  return bindConnectionId<FilesystemApi>(raw, connectionId)
}

/** Bind a connectionId to a RawRunActionsApi, returning a RunActionsApi scoped to that connection */
export function createBoundRunActions(raw: RawRunActionsApi, connectionId: string): RunActionsApi {
  return bindConnectionId<RunActionsApi>(raw, connectionId)
}

export interface GitHubAppState {
  // empty — reads prInfo from workspace store
}

/** Workspace-scoped GitApi with path pre-bound */
export interface WorkspaceGitApi {
  getInfo: () => Promise<GitInfo>
  createWorktree: (name: string, baseBranch?: string) => Promise<WorktreeResult>
  removeWorktree: (worktreePath: string, deleteBranch?: boolean, operationId?: string) => Promise<IpcResult>
  listWorktrees: () => Promise<WorktreeInfo[]>
  listLocalBranches: () => Promise<string[]>
  listRemoteBranches: () => Promise<string[]>
  getBranchesInWorktrees: () => Promise<string[]>
  createWorktreeFromBranch: (branch: string, worktreeName: string) => Promise<WorktreeResult>
  createWorktreeFromRemote: (remoteBranch: string, worktreeName: string) => Promise<WorktreeResult>
  getDiff: (parentBranch: string) => Promise<IpcResult<{ diff: DiffResult }>>
  getFileDiff: (parentBranch: string, filePath: string) => Promise<IpcResult<{ diff: string }>>
  checkMergeConflicts: (sourceBranch: string, targetBranch: string) => Promise<ConflictCheckResult>
  merge: (worktreeBranch: string, squash?: boolean, operationId?: string) => Promise<IpcResult>
  hasUncommittedChanges: () => Promise<boolean>
  commitAll: (message: string) => Promise<IpcResult>
  deleteBranch: (branchName: string, operationId?: string) => Promise<IpcResult>
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

export interface STTApi {
  transcribeOpenAI: (audioBuffer: ArrayBuffer, apiKey: string, language?: string) => Promise<{ text: string }>
  transcribeLocal: (audioBuffer: ArrayBuffer, modelPath: string, language?: string) => Promise<{ text: string }>
  checkMicPermission: () => Promise<boolean>
}

export interface AppApi {
  onReady: (callback: (session: Session | null) => void) => () => void
  onCloseConfirm: (callback: () => void) => () => void
  confirmClose: () => void
  cancelClose: () => void
  onCapsLockEvent: (callback: (event: { type: string; key: string; code: string }) => void) => () => void
}

export interface DaemonApi {
  shutdown: () => Promise<IpcResult>
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
  connect: (config: SSHConnectionConfig, options?: { refreshDaemon?: boolean }) => Promise<{ info: ConnectionInfo; session: Session | null }>
  disconnect: (connectionId: string) => Promise<void>
  listConnections: () => Promise<ConnectionInfo[]>
  saveConnection: (config: SSHConnectionConfig) => Promise<void>
  getSavedConnections: () => Promise<SSHConnectionConfig[]>
  removeSavedConnection: (id: string) => Promise<void>
  getOutput: (connectionId: string) => Promise<string[]>
  onConnectionStatus: (callback: (info: ConnectionInfo) => void) => () => void
  onOutput: (callback: (connectionId: string, line: string) => void) => () => void
  watchOutput: (connectionId: string, cb: (line: string) => void) => Promise<{ scrollback: string[]; unsubscribe: () => void }>
  watchConnectionStatus: (connectionId: string, cb: (info: ConnectionInfo) => void) => Promise<{ initial: ConnectionInfo; unsubscribe: () => void }>
  addPortForward: (config: PortForwardConfig) => Promise<PortForwardInfo>
  removePortForward: (portForwardId: string) => Promise<void>
  listPortForwards: (connectionId: string) => Promise<PortForwardInfo[]>
  onPortForwardStatus: (callback: (info: PortForwardInfo) => void) => () => void
  watchPortForwardOutput: (portForwardId: string, cb: (line: string) => void) => Promise<{ scrollback: string[]; unsubscribe: () => void }>
}

export interface SessionApi {
  create: (workspaces: WorkspaceInput[]) => Promise<IpcResult<{ session: Session }>>
  update: (sessionId: string, workspaces: WorkspaceInput[], senderUuid?: string, expectedVersion?: number) => Promise<IpcResult<{ session: Session }>>
  list: () => Promise<IpcResult<{ sessions: Session[] }>>
  get: (sessionId: string) => Promise<IpcResult<{ session: Session }>>
  delete: (sessionId: string) => Promise<IpcResult>
  openInNewWindow: (sessionId: string) => Promise<IpcResult>
  onShowSessions: (callback: () => void) => () => void
  onSync: (callback: (session: Session) => void) => () => void
}

export type Platform = 'darwin' | 'linux' | 'win32' | 'aix' | 'android' | 'freebsd' | 'haiku' | 'openbsd' | 'sunos' | 'cygwin' | 'netbsd'

export type PreloadApi = {
  platform: Platform
  terminal: TerminalApi
  selectFolder: () => Promise<string | null>
  getRecentDirectories: () => Promise<string[]>
  git: RawGitApi
  github: RawGitHubApi
  settings: SettingsApi
  filesystem: RawFilesystemApi
  runActions: RawRunActionsApi
  sandbox: SandboxApi
  stt: STTApi
  getInitialWorkspace: () => Promise<string | null>
  app: AppApi
  daemon: DaemonApi
  session: SessionApi
  getWindowUuid: () => Promise<string>
  clipboard: ClipboardApi
  llm: LlmApi
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
    'sandbox' in state &&
    typeof (state as AiHarnessState).sandbox === 'object' &&
    (state as AiHarnessState).sandbox !== null
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
    ((state as { status: unknown }).status === 'loading' ||
      (state as { status: unknown }).status === 'ready' ||
      (state as { status: unknown }).status === 'error') &&
    typeof (state as EditorState).filePath === 'string'
  )
}

export function isCommentsState(state: unknown): state is CommentsState {
  return state !== null && typeof state === 'object'
}
