/**
 * Shared type definitions used across main, renderer, preload, and daemon processes.
 * Single source of truth for IPC-related types.
 */

// === Sandbox Types ===

export interface SandboxConfig {
  enabled: boolean
  allowNetwork: boolean
  allowedPaths: string[]
}

// === Daemon Session Types ===

export interface DaemonTab {
  id: string
  applicationId: string
  title: string
  state: unknown
}

export interface DaemonWorkspace {
  path: string              // Primary key - same folder = same workspace session
  name: string
  parentPath: string | null // For tree reconstruction
  status: 'active' | 'merged' | 'abandoned'
  isGitRepo: boolean
  gitBranch: string | null
  gitRootPath: string | null
  isWorktree: boolean
  isDetached?: boolean
  tabs: DaemonTab[]
  activeTabId: string | null
  createdAt: number
  lastActivity: number
  attachedClients: number
}

export interface DaemonSession {
  id: string
  workspaces: DaemonWorkspace[]
  createdAt: number
  lastActivity: number
  attachedClients: number
}

// Helper type for workspace input (without daemon-managed fields)
export type WorkspaceInput = Omit<DaemonWorkspace, 'createdAt' | 'lastActivity' | 'attachedClients'>

// === PTY Session Types ===

export interface DaemonSessionInfo {
  id: string
  cwd: string
  cols: number
  rows: number
  createdAt: number
  lastActivity: number
  attachedClients: number
}

// === Settings Types ===

export interface TerminalInstance {
  id: string
  name: string
  icon: string
  startupCommand: string
  isDefault: boolean
}

export interface AiHarnessInstance {
  id: string
  name: string
  icon: string
  command: string
  isDefault: boolean
  enableSandbox: boolean
  allowNetwork: boolean
  backgroundColor: string
}

export interface PrefixModeConfig {
  enabled: boolean
  prefixKey: string // e.g., 'Control+B'
  timeout: number // ms (default: 1500)
}

export type STTProvider = 'openaiWhisper' | 'localWhisper'

export interface Settings {
  terminal: {
    fontSize: number
    fontFamily: string
    cursorStyle: 'block' | 'underline' | 'bar'
    cursorBlink: boolean
    showRawChars: boolean
    startByDefault: boolean
    instances: TerminalInstance[]
  }
  sandbox: {
    enabledByDefault: boolean
    allowNetworkByDefault: boolean
  }
  aiHarness: {
    instances: AiHarnessInstance[]
  }
  appearance: {
    theme: 'dark' | 'light' | 'system'
  }
  prefixMode: PrefixModeConfig
  keybindings: {
    newTab: string // Key after prefix (e.g., 'c')
    closeTab: string
    nextTab: string
    prevTab: string
    openSettings: string
    workspaceFocus: string
  }
  stt: {
    enabled: boolean
    provider: STTProvider
    openaiApiKey: string
    localWhisperModelPath: string
    pushToTalkKey: string
    language: string // ISO-639-1 code (e.g., 'en', 'ms', 'zh')
  }
  daemon: {
    enabled: boolean
    orphanTimeout: number
    scrollbackLimit: number
    killOnQuit: boolean
  }
}
