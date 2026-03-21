/**
 * Shared type definitions used across main, renderer, preload, and daemon processes.
 * Single source of truth for IPC-related types.
 */

// === SSH Connection Types ===

export interface SSHConnectionConfig {
  id: string
  host: string
  user: string
  port: number // default 22
  identityFile?: string
  label?: string // display name
}

export type ConnectionTarget =
  | { type: 'local' }
  | { type: 'remote'; config: SSHConnectionConfig }

export type ConnectionStatus = 'disconnected' | 'connecting' | 'connected' | 'error'

export interface ConnectionInfo {
  id: string
  target: ConnectionTarget
  status: ConnectionStatus
  error?: string
}

// === Sandbox Types ===

export interface SandboxConfig {
  enabled: boolean
  allowNetwork: boolean
  allowedPaths: string[]
}

// === Daemon Session Types ===

export interface AppState {
  applicationId: string
  title: string
  state: unknown
}

// Worktree-specific settings that can be inherited from parent
export interface WorktreeSettings {
  // Default application to open when creating a new worktree
  // If null, inherits from parent or uses global default
  defaultApplicationId: string | null
}

export interface Workspace {
  id: string
  path: string
  name: string
  parentId: string | null
  children: string[]
  status: 'active' | 'merged' | 'abandoned'
  isGitRepo: boolean
  gitBranch: string | null
  gitRootPath: string | null
  isWorktree: boolean
  isDetached?: boolean
  appStates: Record<string, AppState>
  activeTabId: string | null
  settings?: WorktreeSettings
  metadata: Record<string, string>
  createdAt: number
  lastActivity: number
}

export interface Session {
  id: string
  workspaces: Workspace[]
  createdAt: number
  lastActivity: number
}

// Helper type for workspace input (without daemon-managed fields)
export type WorkspaceInput = Omit<Workspace, 'createdAt' | 'lastActivity' | 'attachedClients'>

// === PTY Session Types ===

export interface SessionInfo {
  id: string
  cwd: string
  cols: number
  rows: number
  createdAt: number
  lastActivity: number
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
  disableScrollbar?: boolean
  stripScrollbackClear?: boolean
}

export interface PrefixModeConfig {
  enabled: boolean
  prefixKey: string // e.g., 'Control+B'
  timeout: number // ms (default: 1500)
}

export type STTProvider = 'openaiWhisper' | 'localWhisper'

// === Run Action Types ===

export interface RunAction {
  id: string                    // deterministic: `${source}:${name}`
  name: string                  // display name
  source: string                // provider name for UI grouping (e.g., "npm", "make")
  description: string
}

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
    scrollbackLimit: number
  }
  // SSH saved connections
  ssh: {
    savedConnections: SSHConnectionConfig[]
  }
  // LLM configuration (OpenAI-compatible)
  llm: {
    baseUrl: string      // OpenAI-compatible endpoint URL
    apiKey: string       // API key
    model: string        // Model name
  }
  // Terminal analyzer LLM configuration
  terminalAnalyzer: {
    model: string           // Model name for terminal analysis
    systemPrompt: string    // System prompt (supports {{cwd}} and {{safe_paths}} templates)
    disableReasoning: boolean
    safePaths: string[]     // Paths considered safe for permission_request classification
  }
  // Global default application for new worktrees
  // If not set, falls back to 'terminal' or first available app
  globalDefaultApplicationId: string
  // Recently opened directories (max 10)
  recentDirectories: string[]
}
