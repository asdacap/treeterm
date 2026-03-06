export interface TerminalTab {
  id: string
  title: string
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
  // Terminal tabs
  terminals: TerminalTab[]
  activeTerminalId: string | null
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
  create: (cwd: string, sandbox?: SandboxConfig) => Promise<string>
  write: (id: string, data: string) => void
  resize: (id: string, cols: number, rows: number) => void
  kill: (id: string) => void
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
}

declare global {
  interface Window {
    electron: ElectronApi
  }
}
