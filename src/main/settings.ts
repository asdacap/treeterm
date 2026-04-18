import { app } from 'electron'
import { join } from 'path'
import { readFileSync, writeFileSync, existsSync, mkdirSync } from 'fs'
import type { Settings, TerminalInstance, AiHarnessInstance, CustomRunnerInstance, PrefixModeConfig, SSHConnectionConfig, ReasoningEffort } from '../shared/types'

// Re-export for backward compatibility
export type { Settings, TerminalInstance, AiHarnessInstance, CustomRunnerInstance, PrefixModeConfig, SSHConnectionConfig }

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
      // eslint-disable-next-line custom/no-string-literal-comparison -- Node platform is external
      command: process.platform === 'darwin' ? 'claude' : 'npx @anthropic-ai/claude-code',
      isDefault: false,
      enableSandbox: false,
      allowNetwork: true,
      backgroundColor: '#1a1a24',
      disableScrollbar: false,
      stripScrollbackClear: false
    }]
  },
  customRunner: {
    instances: []
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
  daemon: {
    mergeThreshold: 50 * 1024,
    compactedLimit: 1024 * 1024,
    scrollbackLines: 10000
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
    systemPrompt: 'You are a terminal state analyzer. The current working directory is: {{cwd}}. The safe paths are: {{safe_paths}}. Given the last lines of terminal output, respond with ONLY a JSON object: {"state": "<state>", "reason": "<reason>"} where state is one of the following that best represent the state: \n - "safe_permission_requested" program asking for permission but the action is safe. A safe action are action that either:\n   - Git operation unless it changes other worktree.\n   - Build or test or install dependencies.\n   - Anything that only mutates files within one of the safe paths.\n   - It IS allowed to read outside the safe path, just not mutate them. \n   - But it is not allowed to do a wide ranging search via find or `grep -r` on file outside the safe path.\n - "permission_request" program asking for y/n or similar confirmation but it is not considered safe as "safe_permission_requested".\n - "completed" when previous user request satisfied.\n - "user_input_required" program asking for user text input or confirmation among design choice or a plan confirmation.\n - "idle" for shell prompt visible, waiting for command or user input is incomplete.\nThe reason field should show the reason for the verdict, no more than 10 word. ',
    titleSystemPrompt: 'Given the terminal output below, suggest a short title (max 5 words), a brief description (max 15 words), and a git branch name (lowercase kebab-case, max 4 words). Respond with ONLY a JSON object: {"title": "<title>", "description": "<description>", "branchName": "<branch-name>"}',
    reasoningEffort: 'low' as ReasoningEffort,
    safePaths: ['/tmp'],
    bufferLines: 30
  },
  github: {
    pat: '',
    autodetectViaGh: true
  },
  globalDefaultApplicationId: 'terminal',
  recentDirectories: [],
  debug: {
    showBadge: false
  }
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
      const loaded = JSON.parse(data) as Partial<Settings>
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
        // eslint-disable-next-line custom/no-string-literal-comparison -- legacy migration from untyped settings
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
      // eslint-disable-next-line custom/no-string-literal-comparison -- Node platform is external
      command: oldClaude.command || (process.platform === 'darwin' ? 'claude' : 'npx @anthropic-ai/claude-code'),
      isDefault: oldClaude.startByDefault || false,
      enableSandbox: oldClaude.enableSandbox || false,
      allowNetwork: true,
      backgroundColor: '#1a1a24',
      disableScrollbar: false,
      stripScrollbackClear: false
    }]
  }

  // Backfill required boolean fields on AI Harness instances from old settings
  aiHarnessInstances = aiHarnessInstances.map(inst => ({
    ...inst,
    disableScrollbar: inst.disableScrollbar,
    stripScrollbackClear: inst.stripScrollbackClear,
  }))

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
    customRunner: {
      instances: loaded.customRunner?.instances || []
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
    daemon: {
      ...defaults.daemon,
      ...loaded.daemon
    },
    ssh: {
      ...defaults.ssh,
      ...loaded.ssh,
      savedConnections: (loaded.ssh?.savedConnections || []).map(c => ({
        ...c,
        portForwards: c.portForwards
      }))
    },
    llm: {
      ...defaults.llm,
      ...loaded.llm
    },
    terminalAnalyzer: {
      ...defaults.terminalAnalyzer,
      ...loaded.terminalAnalyzer
    },
    github: {
      ...defaults.github,
      ...loaded.github
    },
    globalDefaultApplicationId: loaded.globalDefaultApplicationId || defaults.globalDefaultApplicationId,
    recentDirectories: loaded.recentDirectories || defaults.recentDirectories,
    debug: {
      ...defaults.debug,
      ...loaded.debug
    }
  }
}

export function getDefaultSettings(): Settings {
  return { ...defaultSettings }
}
