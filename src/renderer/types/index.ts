export interface Application {
  id: string
  name: string
  command: string
  icon: string
  isDefault: boolean
  isBuiltIn: boolean
}

export interface TerminalTab {
  type: 'terminal'
  id: string
  title: string
  ptyId: string | null
  applicationId?: string
}

export interface FilesystemTab {
  type: 'filesystem'
  id: string
  title: string
  selectedPath: string | null
  expandedDirs: string[]
}

export type WorkspaceTab = TerminalTab | FilesystemTab

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
  // Tabs (terminals and filesystem browsers)
  tabs: WorkspaceTab[]
  activeTabId: string | null
  // Sandbox configuration
  sandbox: SandboxConfig
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
  startup: {
    childWorkspaceCommand: string
  }
  applications: Application[]
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
