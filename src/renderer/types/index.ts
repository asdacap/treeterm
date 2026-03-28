import type { ReactNode } from 'react'
import type { WorkspaceStore } from '../store/createWorkspaceStore'
export type { WorkspaceStore }

// Import and re-export shared types
import type {
  SandboxConfig,
  AppState,
  Workspace,
  Session,
  SessionInfo,
  WorkspaceInput,
  TerminalInstance,
  AiHarnessInstance,
  PrefixModeConfig,
  STTProvider,
  Settings,
  WorktreeSettings,
  RunAction,
  SSHConnectionConfig,
  ConnectionInfo,
  ReasoningEffort
} from '../../shared/types'
import type { PtyEvent } from '../../shared/ipc-types'
export type { PtyEvent }

export type {
  SandboxConfig,
  AppState,
  Workspace,
  Session,
  SessionInfo,
  WorkspaceInput,
  TerminalInstance,
  AiHarnessInstance,
  PrefixModeConfig,
  STTProvider,
  Settings,
  WorktreeSettings,
  RunAction,
  SSHConnectionConfig,
  ConnectionInfo,
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
  keepOnExit?: boolean
}

export type AiHarnessState = TerminalState & {
  sandbox: SandboxConfig
  autoApprove?: boolean
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
  viewMode?: 'committed' | 'uncommitted' | 'commits'
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

export interface EditorState {
  filePath: string
  originalContent: string
  currentContent: string
  language: string
  isDirty: boolean
  viewMode: 'editor' | 'preview'
  isLoading: boolean
  error: string | null
  scrollTop?: number
}

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
  readDirectory: (workspacePath: string, dirPath: string) => Promise<{
    success: boolean
    contents?: DirectoryContents
    error?: string
  }>
  readFile: (workspacePath: string, filePath: string) => Promise<{
    success: boolean
    file?: FileContents
    error?: string
  }>
  writeFile: (workspacePath: string, filePath: string, content: string) => Promise<{
    success: boolean
    error?: string
  }>
  searchFiles: (workspacePath: string, query: string) => Promise<{
    success: boolean
    entries?: FileEntry[]
    error?: string
  }>
}

/** Workspace-scoped FilesystemApi with path pre-bound */
export interface WorkspaceFilesystemApi {
  readDirectory: (dirPath: string) => Promise<{ success: boolean; contents?: DirectoryContents; error?: string }>
  readFile: (filePath: string) => Promise<{ success: boolean; file?: FileContents; error?: string }>
  writeFile: (filePath: string, content: string) => Promise<{ success: boolean; error?: string }>
  searchFiles: (query: string) => Promise<{ success: boolean; entries?: FileEntry[]; error?: string }>
}

export interface GitInfo {
  isRepo: boolean
  branch: string | null
  rootPath: string | null
}

export interface WorktreeResult {
  success: boolean
  path?: string
  branch?: string
  error?: string
}

export interface WorktreeInfo {
  path: string
  branch: string
}

export interface BranchInfo {
  name: string
  isInWorktree: boolean
  worktreePath?: string  // Path if branch is in a worktree
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
  commitHash: string
  createdAt: number
  isOutdated: boolean
  addressed: boolean
  side: 'original' | 'modified'
}

export interface ReviewsData {
  version: 1
  comments: ReviewComment[]
}

export interface ConflictCheckResult {
  success: boolean
  conflicts?: ConflictInfo
  error?: string
}

export interface ClipboardApi {
  writeText: (text: string) => void
  readText: () => Promise<string>
}

export interface TerminalApi {
  create: (connectionId: string, cwd: string, sandbox?: SandboxConfig, startupCommand?: string) => Promise<{ sessionId: string; handle: string } | null>
  attach: (connectionId: string, sessionId: string) => Promise<{ success: boolean; handle?: string; error?: string }>
  list: (connectionId: string) => Promise<SessionInfo[]>
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
  removeWorktree: (repoPath: string, worktreePath: string, deleteBranch?: boolean, operationId?: string) => Promise<{ success: boolean; error?: string }>
  listWorktrees: (repoPath: string) => Promise<WorktreeInfo[]>
  listLocalBranches: (repoPath: string) => Promise<string[]>
  listRemoteBranches: (repoPath: string) => Promise<string[]>
  getBranchesInWorktrees: (repoPath: string) => Promise<string[]>
  createWorktreeFromBranch: (repoPath: string, branch: string, worktreeName: string, operationId?: string) => Promise<WorktreeResult>
  createWorktreeFromRemote: (repoPath: string, remoteBranch: string, worktreeName: string, operationId?: string) => Promise<WorktreeResult>
  getDiff: (worktreePath: string, parentBranch: string) => Promise<{ success: boolean; diff?: DiffResult; error?: string }>
  getFileDiff: (worktreePath: string, parentBranch: string, filePath: string) => Promise<{ success: boolean; diff?: string; error?: string }>
  checkMergeConflicts: (repoPath: string, sourceBranch: string, targetBranch: string) => Promise<ConflictCheckResult>
  merge: (targetWorktreePath: string, worktreeBranch: string, squash?: boolean, operationId?: string) => Promise<{ success: boolean; error?: string }>
  hasUncommittedChanges: (repoPath: string) => Promise<boolean>
  commitAll: (repoPath: string, message: string) => Promise<{ success: boolean; error?: string }>
  deleteBranch: (repoPath: string, branchName: string, operationId?: string) => Promise<{ success: boolean; error?: string }>
  renameBranch: (repoPath: string, oldName: string, newName: string) => Promise<{ success: boolean; error?: string }>
  getUncommittedChanges: (repoPath: string) => Promise<{ success: boolean; changes?: UncommittedChanges; error?: string }>
  getUncommittedFileDiff: (repoPath: string, filePath: string, staged: boolean) => Promise<{ success: boolean; diff?: string; error?: string }>
  stageFile: (repoPath: string, filePath: string) => Promise<{ success: boolean; error?: string }>
  unstageFile: (repoPath: string, filePath: string) => Promise<{ success: boolean; error?: string }>
  stageAll: (repoPath: string) => Promise<{ success: boolean; error?: string }>
  unstageAll: (repoPath: string) => Promise<{ success: boolean; error?: string }>
  commitStaged: (repoPath: string, message: string) => Promise<{ success: boolean; error?: string }>
  getFileContentsForDiff: (worktreePath: string, parentBranch: string, filePath: string) => Promise<{ success: boolean; contents?: FileDiffContents; error?: string }>
  getUncommittedFileContentsForDiff: (repoPath: string, filePath: string, staged: boolean) => Promise<{ success: boolean; contents?: FileDiffContents; error?: string }>
  getHeadCommitHash: (repoPath: string) => Promise<{ success: boolean; hash?: string; error?: string }>
  getLog: (repoPath: string, parentBranch: string | null, skip: number, limit: number) => Promise<{ success: boolean; result?: GitLogResult; error?: string }>
  getCommitDiff: (repoPath: string, commitHash: string) => Promise<{ success: boolean; files?: DiffFile[]; error?: string }>
  getCommitFileDiff: (repoPath: string, commitHash: string, filePath: string) => Promise<{ success: boolean; contents?: FileDiffContents; error?: string }>
  fetch: (repoPath: string) => Promise<{ success: boolean; error?: string }>
  pull: (repoPath: string) => Promise<{ success: boolean; error?: string }>
  getBehindCount: (repoPath: string) => Promise<number>
  getRemoteUrl: (repoPath: string) => Promise<{ url?: string; error?: string }>
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

export interface GitHubAppState {
  // empty — reads prInfo from workspace store
}

/** Workspace-scoped GitApi with path pre-bound */
export interface WorkspaceGitApi {
  getInfo: () => Promise<GitInfo>
  createWorktree: (name: string, baseBranch?: string) => Promise<WorktreeResult>
  removeWorktree: (worktreePath: string, deleteBranch?: boolean, operationId?: string) => Promise<{ success: boolean; error?: string }>
  listWorktrees: () => Promise<WorktreeInfo[]>
  listLocalBranches: () => Promise<string[]>
  listRemoteBranches: () => Promise<string[]>
  getBranchesInWorktrees: () => Promise<string[]>
  createWorktreeFromBranch: (branch: string, worktreeName: string) => Promise<WorktreeResult>
  createWorktreeFromRemote: (remoteBranch: string, worktreeName: string) => Promise<WorktreeResult>
  getDiff: (parentBranch: string) => Promise<{ success: boolean; diff?: DiffResult; error?: string }>
  getFileDiff: (parentBranch: string, filePath: string) => Promise<{ success: boolean; diff?: string; error?: string }>
  checkMergeConflicts: (sourceBranch: string, targetBranch: string) => Promise<ConflictCheckResult>
  merge: (worktreeBranch: string, squash?: boolean, operationId?: string) => Promise<{ success: boolean; error?: string }>
  hasUncommittedChanges: () => Promise<boolean>
  commitAll: (message: string) => Promise<{ success: boolean; error?: string }>
  deleteBranch: (branchName: string, operationId?: string) => Promise<{ success: boolean; error?: string }>
  getUncommittedChanges: () => Promise<{ success: boolean; changes?: UncommittedChanges; error?: string }>
  getUncommittedFileDiff: (filePath: string, staged: boolean) => Promise<{ success: boolean; diff?: string; error?: string }>
  stageFile: (filePath: string) => Promise<{ success: boolean; error?: string }>
  unstageFile: (filePath: string) => Promise<{ success: boolean; error?: string }>
  stageAll: () => Promise<{ success: boolean; error?: string }>
  unstageAll: () => Promise<{ success: boolean; error?: string }>
  commitStaged: (message: string) => Promise<{ success: boolean; error?: string }>
  getFileContentsForDiff: (parentBranch: string, filePath: string) => Promise<{ success: boolean; contents?: FileDiffContents; error?: string }>
  getUncommittedFileContentsForDiff: (filePath: string, staged: boolean) => Promise<{ success: boolean; contents?: FileDiffContents; error?: string }>
  getHeadCommitHash: () => Promise<{ success: boolean; hash?: string; error?: string }>
  getLog: (parentBranch: string | null, skip: number, limit: number) => Promise<{ success: boolean; result?: GitLogResult; error?: string }>
  getCommitDiff: (commitHash: string) => Promise<{ success: boolean; files?: DiffFile[]; error?: string }>
  getCommitFileDiff: (commitHash: string, filePath: string) => Promise<{ success: boolean; contents?: FileDiffContents; error?: string }>
  fetch: () => Promise<{ success: boolean; error?: string }>
  pull: () => Promise<{ success: boolean; error?: string }>
  getBehindCount: () => Promise<number>
}

export interface SettingsApi {
  load: () => Promise<Settings>
  save: (settings: Settings) => Promise<{ success: boolean }>
  onOpen: (callback: () => void) => () => void
}

export interface AppRegistryApi {
  get: (id: string) => Application | undefined | null
  getDefaultApp: (appId?: string) => Application | null
}

export interface RunActionsApi {
  detect: (workspacePath: string) => Promise<RunAction[]>
  run: (workspacePath: string, actionId: string) => Promise<string | null>
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
  shutdown: () => Promise<{ success: boolean; error?: string }>
  onSessions: (callback: (sessions: SessionInfo[]) => void) => () => void
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
  connect: (config: SSHConnectionConfig, options?: { refreshDaemon?: boolean }) => Promise<{ info: ConnectionInfo, session?: Session }>
  disconnect: (connectionId: string) => Promise<void>
  listConnections: () => Promise<ConnectionInfo[]>
  saveConnection: (config: SSHConnectionConfig) => Promise<void>
  getSavedConnections: () => Promise<SSHConnectionConfig[]>
  removeSavedConnection: (id: string) => Promise<void>
  getOutput: (connectionId: string) => Promise<string[]>
  onConnectionStatus: (callback: (info: ConnectionInfo) => void) => () => void
  onOutput: (callback: (connectionId: string, line: string) => void) => () => void
  watchOutput: (connectionId: string, cb: (line: string) => void) => Promise<{ scrollback: string[], unsubscribe: () => void }>
  watchConnectionStatus: (connectionId: string, cb: (info: ConnectionInfo) => void) => Promise<{ initial: ConnectionInfo | undefined, unsubscribe: () => void }>
}

export interface SessionApi {
  create: (workspaces: WorkspaceInput[]) =>
    Promise<{ success: boolean; session?: Session; error?: string }>
  update: (sessionId: string, workspaces: WorkspaceInput[], senderUuid?: string) =>
    Promise<{ success: boolean; session?: Session; error?: string }>
  list: () => Promise<{ success: boolean; sessions?: Session[]; error?: string }>
  get: (sessionId: string) => Promise<{ success: boolean; session?: Session; error?: string }>
  delete: (sessionId: string) => Promise<{ success: boolean; error?: string }>
  openInNewWindow: (sessionId: string) => Promise<{ success: boolean; error?: string }>
  onShowSessions: (callback: () => void) => () => void
  onSync: (callback: (session: Session) => void) => () => void
}

export type Platform = 'darwin' | 'linux' | 'win32' | 'aix' | 'android' | 'freebsd' | 'haiku' | 'openbsd' | 'sunos' | 'cygwin' | 'netbsd'

declare global {
  interface Window {
    electron: {
      platform: Platform
      terminal: TerminalApi
      selectFolder: () => Promise<string | null>
      git: GitApi
      github: GitHubApi
      settings: SettingsApi
      filesystem: FilesystemApi
      runActions: RunActionsApi
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
    'filePath' in state &&
    'originalContent' in state &&
    'currentContent' in state &&
    'language' in state &&
    'isDirty' in state &&
    'viewMode' in state &&
    'isLoading' in state &&
    typeof (state as EditorState).filePath === 'string'
  )
}

export function isCommentsState(state: unknown): state is CommentsState {
  return state !== null && typeof state === 'object'
}
