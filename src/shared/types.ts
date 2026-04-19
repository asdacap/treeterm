/**
 * Shared type definitions used across main, renderer, preload, and daemon processes.
 * Single source of truth for IPC-related types.
 */

// === SSH Connection Types ===

export interface PortForwardSpec {
  localPort: number
  remoteHost: string
  remotePort: number
}

export interface SSHConnectionConfig {
  id: string
  host: string
  user: string
  port: number // default 22
  identityFile?: string
  label?: string // display name
  portForwards: PortForwardSpec[]
}

export enum ConnectionTargetType {
  Local = 'local',
  Remote = 'remote',
}

export type ConnectionTarget =
  | { type: ConnectionTargetType.Local }
  | { type: ConnectionTargetType.Remote; config: SSHConnectionConfig }

export enum ConnectionStatus {
  Disconnected = 'disconnected',
  Connecting = 'connecting',
  Connected = 'connected',
  Reconnecting = 'reconnecting',
  Error = 'error',
}

export enum ConnectPhase {
  Bootstrap = 'bootstrap',
  Tunnel = 'tunnel',
  Daemon = 'daemon',
}

export type ConnectionInfo =
  | { id: string; target: ConnectionTarget; status: ConnectionStatus.Connecting; connectPhase?: ConnectPhase }
  | { id: string; target: ConnectionTarget; status: ConnectionStatus.Connected }
  | { id: string; target: ConnectionTarget; status: ConnectionStatus.Reconnecting; error: string; attempt: number }
  | { id: string; target: ConnectionTarget; status: ConnectionStatus.Disconnected; error?: string }
  | { id: string; target: ConnectionTarget; status: ConnectionStatus.Error; error: string }

// === Port Forward Types ===

export interface PortForwardConfig {
  id: string
  connectionId: string
  localPort: number
  remoteHost: string
  remotePort: number
}

export enum PortForwardStatus {
  Connecting = 'connecting',
  Active = 'active',
  Error = 'error',
  Stopped = 'stopped',
}

export type PortForwardInfo =
  | { id: string; connectionId: string; localPort: number; remoteHost: string; remotePort: number; status: PortForwardStatus.Connecting | PortForwardStatus.Active | PortForwardStatus.Stopped }
  | { id: string; connectionId: string; localPort: number; remoteHost: string; remotePort: number; status: PortForwardStatus.Error; error: string }

// === Sandbox Types ===

export interface SandboxConfig {
  enabled: boolean
  allowNetwork: boolean
  allowedPaths: string[]
}

// === Daemon Session Types ===
//
// Derived from the proto-generated types in src/generated/treeterm.ts so that
// there is exactly one source of truth for the workspace/session shape. The
// only divergences are:
//   - `metadata` bytes → `Record<string, string>` (parsed at the gRPC boundary)
//   - `appStates[].state` bytes → `unknown` (parsed at the gRPC boundary)
//   - `status` string → `WorkspaceStatus` enum (narrowed)
//
// Renderer-only per-workspace ephemeral state (e.g. `WorktreeSettings`) lives
// on WorkspaceStore, not on this type.

import type {
  Workspace as ProtoWorkspace,
  Session as ProtoSession,
  AppState as ProtoAppState,
  SessionLock as ProtoSessionLock,
} from '../generated/treeterm'

export type SessionLock = ProtoSessionLock

export type AppState = Omit<ProtoAppState, 'state'> & { state: unknown }

// Worktree-specific settings that can be inherited from parent.
// Renderer-only parameter type — never serialized to the daemon.
export interface WorktreeSettings {
  // Default application to open when creating a new worktree.
  // Empty string means inherit from parent or use global default.
  defaultApplicationId: string
}

export enum WorkspaceStatus {
  Active = 'active',
  Merged = 'merged',
  Abandoned = 'abandoned',
}

export type Workspace = Omit<ProtoWorkspace, 'metadata' | 'appStates' | 'status'> & {
  status: WorkspaceStatus
  metadata: Record<string, string>
  appStates: Record<string, AppState>
}

export type Session = Omit<ProtoSession, 'workspaces'> & {
  workspaces: Workspace[]
}

// Helper type for workspace input (without daemon-managed fields).
export type WorkspaceInput = Omit<Workspace, 'createdAt' | 'lastActivity'>

// === PTY Session Types ===

export interface TTYSessionInfo {
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
  disableScrollbar: boolean
  stripScrollbackClear: boolean
}

export interface CustomRunnerInstance {
  id: string
  name: string
  icon: string
  commandTemplate: string  // e.g., "rider {{workspace_path}}"
  isDefault: boolean
}

export interface PrefixModeConfig {
  enabled: boolean
  prefixKey: string // e.g., 'Control+B'
  timeout: number // ms (default: 1500)
}

// === Run Action Types ===

export interface RunAction {
  id: string                    // deterministic: `${source}:${name}`
  name: string                  // display name
  source: string                // provider name for UI grouping (e.g., "npm", "make")
  description: string
}

export enum FileChangeStatus {
  Added = 'added',
  Modified = 'modified',
  Deleted = 'deleted',
  Renamed = 'renamed',
  Untracked = 'untracked',
}

export enum ReasoningEffort {
  Off = 'off',
  Low = 'low',
  Medium = 'medium',
  High = 'high',
}

export interface Settings {
  terminal: {
    fontSize: number
    fontFamily: string
    cursorStyle: 'block' | 'underline' | 'bar'
    cursorBlink: boolean
    showRawChars: boolean
    instances: TerminalInstance[]
  }
  sandbox: {
    enabledByDefault: boolean
    allowNetworkByDefault: boolean
  }
  aiHarness: {
    instances: AiHarnessInstance[]
  }
  customRunner: {
    instances: CustomRunnerInstance[]
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
  daemon: {
    mergeThreshold: number
    compactedLimit: number
    scrollbackLines: number
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
    titleSystemPrompt: string // System prompt for generating workspace titles from terminal output
    reasoningEffort: ReasoningEffort
    safePaths: string[]     // Paths considered safe for permission_request classification
    bufferLines: number     // Number of lines to read from terminal buffer
  }
  // GitHub integration
  github: {
    pat: string
    autodetectViaGh: boolean
  }
  // Global default application for new worktrees
  // If not set, falls back to 'terminal' or first available app
  globalDefaultApplicationId: string
  // Recently opened directories (max 10)
  recentDirectories: string[]
  // Debug settings
  debug: {
    showBadge: boolean
  }
}
