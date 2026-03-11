import type { ReactNode } from 'react'

// Import and re-export shared types
import type {
  SandboxConfig,
  DaemonTab,
  DaemonWorkspace,
  DaemonSession,
  DaemonSessionInfo,
  WorkspaceInput,
  TerminalInstance,
  PrefixModeConfig,
  STTProvider,
  Settings
} from '../../shared/types'

export type {
  SandboxConfig,
  DaemonTab,
  DaemonWorkspace,
  DaemonSession,
  DaemonSessionInfo,
  WorkspaceInput,
  TerminalInstance,
  PrefixModeConfig,
  STTProvider,
  Settings
}

// Activity state for applications that can report their state
export type ActivityState = 'idle' | 'working' | 'waiting_for_input'

// Application - code-defined, registered at runtime
export interface Application<TState = unknown> {
  id: string
  name: string
  icon: string
  createInitialState: () => TState
  cleanup?: (tab: Tab, workspace: Workspace) => void | Promise<void>
  render: (props: ApplicationRenderProps) => ReactNode
  canClose: boolean
  canHaveMultiple: boolean
  showInNewTabMenu: boolean
  // Whether to keep tabs mounted when workspace is inactive (for PTY persistence)
  keepAlive: boolean
  // CSS display style when visible: 'block' or 'flex'
  displayStyle: 'block' | 'flex'
  // Whether this app creates tabs automatically in new workspaces
  isDefault: boolean
  // Optional: Applications can report their activity state
  getActivityState?: (tab: Tab) => ActivityState
}

export interface ApplicationRenderProps {
  tab: Tab
  workspaceId: string
  workspacePath: string
  isVisible: boolean
}

// Tab - unified tab type, references application by id
export interface Tab {
  id: string
  applicationId: string
  title: string
  state: unknown
}

// Type-specific state interfaces (for internal use within applications)
export interface TerminalState {
  ptyId: string | null
}

export interface ClaudeState extends TerminalState {
  sandbox: SandboxConfig
}

export interface FilesystemState {
  selectedPath: string | null
  expandedDirs: string[]
}

export interface ReviewState {
  parentWorkspaceId: string
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
}

export interface Workspace {
  id: string
  name: string
  path: string
  parentId: string | null
  children: string[]
  status: 'active' | 'merged' | 'abandoned'
  // Git-related fields
  isGitRepo: boolean
  gitBranch: string | null
  gitRootPath: string | null
  isWorktree: boolean
  isDetached?: boolean  // If true, no merge - just "Close and Clean"
  // Tabs
  tabs: Tab[]
  activeTabId: string | null
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

export interface ChildWorktreeInfo extends WorktreeInfo {
  displayName: string
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

export interface ConflictCheckResult {
  success: boolean
  conflicts?: ConflictInfo
  error?: string
}

export interface TerminalApi {
  create: (cwd: string, sandbox?: SandboxConfig, startupCommand?: string) => Promise<string>
  attach: (sessionId: string) => Promise<{ success: boolean; scrollback?: string[]; error?: string }>
  detach: (sessionId: string) => Promise<void>
  list: () => Promise<DaemonSessionInfo[]>
  write: (id: string, data: string) => void
  resize: (id: string, cols: number, rows: number) => void
  kill: (id: string) => void
  isAlive: (id: string) => Promise<boolean>
  onData: (id: string, callback: (data: string) => void) => () => void
  onExit: (id: string, callback: (exitCode: number) => void) => () => void
  onNewTerminal: (callback: () => void) => () => void
  onShowSessions: (callback: () => void) => () => void
}

export interface GitApi {
  getInfo: (dirPath: string) => Promise<GitInfo>
  createWorktree: (repoPath: string, name: string, baseBranch?: string) => Promise<WorktreeResult>
  removeWorktree: (repoPath: string, worktreePath: string, deleteBranch?: boolean) => Promise<{ success: boolean; error?: string }>
  listWorktrees: (repoPath: string) => Promise<WorktreeInfo[]>
  getChildWorktrees: (repoPath: string, parentBranch: string | null) => Promise<ChildWorktreeInfo[]>
  listLocalBranches: (repoPath: string) => Promise<string[]>
  listRemoteBranches: (repoPath: string) => Promise<string[]>
  getBranchesInWorktrees: (repoPath: string) => Promise<string[]>
  createWorktreeFromBranch: (repoPath: string, branch: string, worktreeName: string) => Promise<WorktreeResult>
  createWorktreeFromRemote: (repoPath: string, remoteBranch: string, worktreeName: string) => Promise<WorktreeResult>
  getDiff: (worktreePath: string, parentBranch: string) => Promise<{ success: boolean; diff?: DiffResult; error?: string }>
  getFileDiff: (worktreePath: string, parentBranch: string, filePath: string) => Promise<{ success: boolean; diff?: string; error?: string }>
  getDiffAgainstHead: (worktreePath: string, parentBranch: string) => Promise<{ success: boolean; diff?: DiffResult; error?: string }>
  getFileDiffAgainstHead: (worktreePath: string, parentBranch: string, filePath: string) => Promise<{ success: boolean; diff?: string; error?: string }>
  checkMergeConflicts: (repoPath: string, sourceBranch: string, targetBranch: string) => Promise<ConflictCheckResult>
  merge: (mainRepoPath: string, worktreeBranch: string, targetBranch: string, squash?: boolean) => Promise<{ success: boolean; error?: string }>
  hasUncommittedChanges: (repoPath: string) => Promise<boolean>
  commitAll: (repoPath: string, message: string) => Promise<{ success: boolean; error?: string }>
  deleteBranch: (repoPath: string, branchName: string) => Promise<{ success: boolean; error?: string }>
  getUncommittedChanges: (repoPath: string) => Promise<{ success: boolean; changes?: UncommittedChanges; error?: string }>
  getUncommittedFileDiff: (repoPath: string, filePath: string, staged: boolean) => Promise<{ success: boolean; diff?: string; error?: string }>
  stageFile: (repoPath: string, filePath: string) => Promise<{ success: boolean; error?: string }>
  unstageFile: (repoPath: string, filePath: string) => Promise<{ success: boolean; error?: string }>
  stageAll: (repoPath: string) => Promise<{ success: boolean; error?: string }>
  unstageAll: (repoPath: string) => Promise<{ success: boolean; error?: string }>
  commitStaged: (repoPath: string, message: string) => Promise<{ success: boolean; error?: string }>
  getFileContentsForDiff: (worktreePath: string, parentBranch: string, filePath: string) => Promise<{ success: boolean; contents?: FileDiffContents; error?: string }>
  getFileContentsForDiffAgainstHead: (worktreePath: string, parentBranch: string, filePath: string) => Promise<{ success: boolean; contents?: FileDiffContents; error?: string }>
  getUncommittedFileContentsForDiff: (repoPath: string, filePath: string, staged: boolean) => Promise<{ success: boolean; contents?: FileDiffContents; error?: string }>
}

export interface SettingsApi {
  load: () => Promise<Settings>
  save: (settings: Settings) => Promise<{ success: boolean }>
  onOpen: (callback: () => void) => () => void
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
  onReady: (callback: () => void) => () => void
  onCloseConfirm: (callback: () => void) => () => void
  confirmClose: () => void
  cancelClose: () => void
  onCapsLockEvent: (callback: (event: { type: string; key: string; code: string }) => void) => () => void
}

export interface DaemonApi {
  shutdown: () => Promise<{ success: boolean; error?: string }>
  onSessions: (callback: (sessions: DaemonSessionInfo[]) => void) => () => void
}

export interface SessionApi {
  create: (workspaces: WorkspaceInput[]) =>
    Promise<{ success: boolean; session?: DaemonSession; error?: string }>
  update: (sessionId: string, workspaces: WorkspaceInput[]) =>
    Promise<{ success: boolean; session?: DaemonSession; error?: string }>
  list: () => Promise<{ success: boolean; sessions?: DaemonSession[]; error?: string }>
  get: (sessionId: string) => Promise<{ success: boolean; session?: DaemonSession; error?: string }>
  delete: (sessionId: string) => Promise<{ success: boolean; error?: string }>
  onShowSessions: (callback: () => void) => () => void
}

export interface ElectronApi {
  platform: 'darwin' | 'linux' | 'win32' | 'aix' | 'android' | 'freebsd' | 'haiku' | 'openbsd' | 'sunos' | 'cygwin' | 'netbsd'
  terminal: TerminalApi
  selectFolder: () => Promise<string | null>
  git: GitApi
  settings: SettingsApi
  filesystem: FilesystemApi
  sandbox: SandboxApi
  stt: STTApi
  getInitialWorkspace: () => Promise<string | null>
  app: AppApi
  daemon: DaemonApi
  session: SessionApi
}

declare global {
  interface Window {
    electron: ElectronApi
  }
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

export function isClaudeState(state: unknown): state is ClaudeState {
  return (
    isTerminalState(state) &&
    'sandbox' in state &&
    typeof (state as ClaudeState).sandbox === 'object' &&
    (state as ClaudeState).sandbox !== null
  )
}

export function isFilesystemState(state: unknown): state is FilesystemState {
  return (
    state !== null &&
    typeof state === 'object' &&
    'selectedPath' in state &&
    'expandedDirs' in state &&
    Array.isArray((state as FilesystemState).expandedDirs)
  )
}

export function isReviewState(state: unknown): state is ReviewState {
  return (
    state !== null &&
    typeof state === 'object' &&
    'parentWorkspaceId' in state &&
    typeof (state as ReviewState).parentWorkspaceId === 'string'
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
