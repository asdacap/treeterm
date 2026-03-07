import type { ReactNode } from 'react'

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
  // Optional: Applications can report their activity state
  getActivityState?: (tab: Tab) => ActivityState
}

export interface ApplicationRenderProps {
  tab: Tab
  workspaceId: string
  workspacePath: string
}

// Tab - unified tab type, references application by id
export interface Tab {
  id: string
  applicationId: string
  title: string
  state: unknown
  config?: Record<string, unknown>
}

// ApplicationInstance - user configuration in settings
export interface ApplicationInstance {
  id: string
  applicationId: string
  name: string
  icon: string
  config: Record<string, unknown>
  isDefault: boolean
  isBuiltIn: boolean
}

// Type-specific state interfaces (for internal use within applications)
export interface TerminalState {
  ptyId: string | null
}

export interface FilesystemState {
  selectedPath: string | null
  expandedDirs: string[]
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
}

export interface SandboxConfig {
  enabled: boolean
  allowNetwork: boolean
  allowedPaths: string[] // Additional paths besides workspace
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

export interface TerminalApi {
  create: (cwd: string, sandbox?: SandboxConfig, startupCommand?: string) => Promise<string>
  write: (id: string, data: string) => void
  resize: (id: string, cols: number, rows: number) => void
  kill: (id: string) => void
  isAlive: (id: string) => Promise<boolean>
  onData: (id: string, callback: (data: string) => void) => () => void
}

export interface GitApi {
  getInfo: (dirPath: string) => Promise<GitInfo>
  createWorktree: (repoPath: string, name: string, baseBranch?: string) => Promise<WorktreeResult>
  removeWorktree: (repoPath: string, worktreePath: string, deleteBranch?: boolean) => Promise<{ success: boolean; error?: string }>
  listWorktrees: (repoPath: string) => Promise<WorktreeInfo[]>
  getDiff: (worktreePath: string, parentBranch: string) => Promise<{ success: boolean; diff?: DiffResult; error?: string }>
  getFileDiff: (worktreePath: string, parentBranch: string, filePath: string) => Promise<{ success: boolean; diff?: string; error?: string }>
  merge: (mainRepoPath: string, worktreeBranch: string, targetBranch: string, squash?: boolean) => Promise<{ success: boolean; error?: string }>
  hasUncommittedChanges: (repoPath: string) => Promise<boolean>
  commitAll: (repoPath: string, message: string) => Promise<{ success: boolean; error?: string }>
  deleteBranch: (repoPath: string, branchName: string) => Promise<{ success: boolean; error?: string }>
}

export interface Settings {
  terminal: {
    fontSize: number
    fontFamily: string
    cursorStyle: 'block' | 'underline' | 'bar'
    cursorBlink: boolean
  }
  sandbox: {
    enabledByDefault: boolean
    allowNetworkByDefault: boolean
  }
  appearance: {
    theme: 'dark' | 'light' | 'system'
  }
  keybindings: {
    newTab: string
    closeTab: string
    nextTab: string
    prevTab: string
    openSettings: string
  }
  applications: ApplicationInstance[]
}

export interface SettingsApi {
  load: () => Promise<Settings>
  save: (settings: Settings) => Promise<{ success: boolean }>
  onOpen: (callback: () => void) => () => void
}

export interface ElectronApi {
  terminal: TerminalApi
  selectFolder: () => Promise<string | null>
  git: GitApi
  settings: SettingsApi
  filesystem: FilesystemApi
}

declare global {
  interface Window {
    electron: ElectronApi
  }
}
