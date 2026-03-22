import { app } from 'electron'
import { join } from 'path'
import { readFileSync, writeFileSync, existsSync, mkdirSync } from 'fs'
import type { Settings, TerminalInstance, AiHarnessInstance, PrefixModeConfig, STTProvider, SSHConnectionConfig, ReasoningEffort } from '../shared/types'

// Re-export for backward compatibility
export type { Settings, TerminalInstance, AiHarnessInstance, PrefixModeConfig, STTProvider, SSHConnectionConfig }

const defaultSettings: Settings = {
  terminal: {
    fontSize: 14,
    fontFamily: 'Menlo, Monaco, Consolas, monospace',
    cursorStyle: 'block',
    cursorBlink: true,
    showRawChars: false,
    instances: []
  },
  sandbox: {
    enabledByDefault: false,
    allowNetworkByDefault: true
  },
  aiHarness: {
    instances: [{
      id: 'claude',
      name: 'Claude',
      icon: '✦',
      command: process.platform === 'darwin' ? 'claude' : 'npx @anthropic-ai/claude-code',
      isDefault: false,
      enableSandbox: false,
      allowNetwork: true,
      backgroundColor: '#1a1a24'
    }]
  },
  appearance: {
    theme: 'dark'
  },
  prefixMode: {
    enabled: true,
    prefixKey: 'Control+B',
    timeout: 1500
  },
  keybindings: {
    newTab: 'c',
    closeTab: 'x',
    nextTab: 'n',
    prevTab: 'p',
    openSettings: ',',
    workspaceFocus: 'w'
  },
  stt: {
    enabled: true,
    provider: 'openaiWhisper',
    openaiApiKey: '',
    localWhisperModelPath: '',
    pushToTalkKey: 'Shift+Space',
    language: 'en'
  },
  daemon: {
    scrollbackLimit: 50000
  },
  ssh: {
    savedConnections: []
  },
  llm: {
    baseUrl: 'https://openrouter.ai/api/v1',
    apiKey: '',
    model: 'gpt-4o'
  },
  terminalAnalyzer: {
    model: 'openai/gpt-oss-safeguard-20b',
    systemPrompt: 'You are a terminal state analyzer. The current working directory is: {{cwd}}. The safe paths are: {{safe_paths}}. Given the last lines of terminal output, respond with ONLY a JSON object: {"state": "<state>", "reason": "<reason>"} where state is one of: "idle" (shell prompt visible, waiting for command), "working" (process actively running/producing output), "user_input_required" (program asking for user text input), "permission_request" (program asking for y/n or similar confirmation and the action may mutate files OUTSIDE the safe paths), "safe_permission_requested" (program asking for permission but the action only mutates files within one of the safe paths — safe to approve), "completed" (task finished, showing final result). "working" override other state and reason is the reason for the verdict, no more than 10 word. A git operation is considered safe as long as it does not mutate other branch. A plan approval is considered "user_input_required" even if it only mutate safe path.',
    titleSystemPrompt: 'Given the terminal output below, suggest a short title (max 5 words) and a brief description (max 15 words) for this workspace session. Respond with ONLY a JSON object: {"title": "<title>", "description": "<description>"}',
    reasoningEffort: 'off' as ReasoningEffort,
    safePaths: ['/tmp'],
    bufferLines: 30
  },
  globalDefaultApplicationId: 'terminal',
  recentDirectories: []
}

// Helper to add a directory to recent directories list
export function addRecentDirectory(settings: Settings, dirPath: string): Settings {
  // Remove existing entry if present
  const filtered = settings.recentDirectories.filter(d => d !== dirPath)
  // Add to front
  const updated = [dirPath, ...filtered]
  // Keep only first 10
  const limited = updated.slice(0, 10)
  return { ...settings, recentDirectories: limited }
}

function getSettingsDir(): string {
  return app.getPath('userData')
}

function getSettingsPath(): string {
  return join(getSettingsDir(), 'settings.json')
}

export function loadSettings(): Settings {
  const settingsPath = getSettingsPath()

  try {
    if (existsSync(settingsPath)) {
      const data = readFileSync(settingsPath, 'utf-8')
      const loaded = JSON.parse(data)
      // Deep merge with defaults to handle missing fields
      return mergeSettings(defaultSettings, loaded)
    }
  } catch (error) {
    console.warn('[settings] Failed to load settings, using defaults:', error)
  }

  // Return defaults and save them
  saveSettings(defaultSettings)
  return defaultSettings
}

export function saveSettings(settings: Settings): void {
  const settingsDir = getSettingsDir()
  const settingsPath = getSettingsPath()

  try {
    if (!existsSync(settingsDir)) {
      mkdirSync(settingsDir, { recursive: true })
    }
    // Exception to "all file mutations through daemon gRPC" rule:
    // Settings live in the app data dir (outside any workspace), so direct fs access is correct here.
    writeFileSync(settingsPath, JSON.stringify(settings, null, 2), 'utf-8')
  } catch (error) {
    console.error('Failed to save settings:', error)
    throw error
  }
}

function migrateKeybindings(
  defaults: Settings['keybindings'],
  loaded: Record<string, string | { direct?: string; prefixMode?: string }> | undefined
): Settings['keybindings'] {
  if (!loaded) return defaults

  const result = { ...defaults }
  const keys = ['newTab', 'closeTab', 'nextTab', 'prevTab', 'openSettings'] as const

  for (const key of keys) {
    const value = loaded[key]
    if (value) {
      if (typeof value === 'string') {
        // Already a string, keep it
        result[key] = value
      } else if (value.prefixMode) {
        // Migrate from old object format - use prefixMode field
        result[key] = value.prefixMode
      }
    }
  }

  return result
}

function mergeSettings(defaults: Settings, loaded: Partial<Settings>): Settings {
  // Migrate terminal instances from various old formats
  let terminalInstances: TerminalInstance[] = loaded.terminal?.instances || []

  // Migrate from old applications array format
  const oldApplications = (loaded as { applications?: Array<{
    id: string
    applicationId?: string
    name: string
    icon: string
    config?: { command?: string }
    command?: string
    isDefault: boolean
    isBuiltIn: boolean
  }> }).applications

  if (oldApplications && terminalInstances.length === 0) {
    // Extract custom terminal instances from old applications format
    terminalInstances = oldApplications
      .filter(a => {
        // Only migrate non-built-in terminals with custom commands
        const hasCommand = a.config?.command || a.command
        const isTerminal = a.applicationId === 'terminal' || !a.applicationId
        return !a.isBuiltIn && isTerminal && hasCommand
      })
      .map(a => ({
        id: a.id,
        name: a.name,
        icon: a.icon,
        startupCommand: a.config?.command || a.command || '',
        isDefault: a.isDefault
      }))
  }

  // Migrate AI Harness instances from old claude settings
  let aiHarnessInstances: AiHarnessInstance[] = loaded.aiHarness?.instances || []
  const oldClaude = (loaded as { claude?: { command?: string; startByDefault?: boolean; enableSandbox?: boolean } }).claude
  if (oldClaude && aiHarnessInstances.length === 0) {
    aiHarnessInstances = [{
      id: 'claude',
      name: 'Claude',
      icon: '✦',
      command: oldClaude.command || (process.platform === 'darwin' ? 'claude' : 'npx @anthropic-ai/claude-code'),
      isDefault: oldClaude.startByDefault || false,
      enableSandbox: oldClaude.enableSandbox || false,
      allowNetwork: true,
      backgroundColor: '#1a1a24'
    }]
  }

  return {
    terminal: {
      ...defaults.terminal,
      ...loaded.terminal,
      instances: terminalInstances
    },
    sandbox: {
      ...defaults.sandbox,
      ...loaded.sandbox
    },
    aiHarness: {
      instances: aiHarnessInstances
    },
    appearance: {
      ...defaults.appearance,
      ...loaded.appearance
    },
    prefixMode: {
      ...defaults.prefixMode,
      ...loaded.prefixMode
    },
    keybindings: migrateKeybindings(
      defaults.keybindings,
      loaded.keybindings as Record<string, string | { direct?: string; prefixMode?: string }> | undefined
    ),
    stt: {
      ...defaults.stt,
      ...loaded.stt
    },
    daemon: {
      ...defaults.daemon,
      ...loaded.daemon
    },
    ssh: {
      ...defaults.ssh,
      ...loaded.ssh
    },
    llm: {
      ...defaults.llm,
      ...loaded.llm
    },
    terminalAnalyzer: {
      ...defaults.terminalAnalyzer,
      ...loaded.terminalAnalyzer
    },
    globalDefaultApplicationId: loaded.globalDefaultApplicationId || defaults.globalDefaultApplicationId,
    recentDirectories: loaded.recentDirectories || defaults.recentDirectories
  }
}

export function getDefaultSettings(): Settings {
  return { ...defaultSettings }
}
