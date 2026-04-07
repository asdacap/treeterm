/**
 * Run Actions — discovers and executes project-defined tasks/commands.
 *
 * Each RunActionProvider knows how to detect actions from a specific config
 * format (package.json, Makefile, etc.) and run them via a PTY.
 */

import path from 'path'
import type { RunAction } from '../shared/types'
import type { GrpcDaemonClient } from './grpcClient'

type ReadFile = (path: string) => Promise<string | null>

export interface RunActionProvider {
  source: string
  detect: (workspacePath: string, readFile: ReadFile) => Promise<RunAction[]>
  run: (actionId: string, workspacePath: string) => Promise<string>
}

// === Provider Factories ===

function createNpmProvider(daemonClient: GrpcDaemonClient): RunActionProvider {
  return {
    source: 'npm',
    detect: async (_workspacePath, readFile) => {
      const content = await readFile('package.json')
      if (!content) return []
      const pkg: unknown = JSON.parse(content)
      if (typeof pkg !== 'object' || pkg === null || !('scripts' in pkg)) return []
      const scripts = (pkg as Record<string, unknown>).scripts
      if (!scripts || typeof scripts !== 'object') return []
      const scriptsRecord = scripts as Record<string, string>
      return Object.keys(scriptsRecord).map((name): RunAction => ({
        id: `npm:${name}`,
        name,
        source: 'npm',
        description: scriptsRecord[name]
      }))
    },
    run: async (_actionId, workspacePath) => {
      const name = _actionId.slice('npm:'.length)
      return daemonClient.createPtySession({ cwd: workspacePath, startupCommand: `npm run ${name}` })
    }
  }
}

function createMakeProvider(daemonClient: GrpcDaemonClient): RunActionProvider {
  return {
    source: 'make',
    detect: async (_workspacePath, readFile) => {
      const content = await readFile('Makefile')
      if (!content) return []
      return parseMakeTargets(content)
    },
    run: async (actionId, workspacePath) => {
      const name = actionId.slice('make:'.length)
      return daemonClient.createPtySession({ cwd: workspacePath, startupCommand: `make ${name}` })
    }
  }
}

function createJustProvider(daemonClient: GrpcDaemonClient): RunActionProvider {
  return {
    source: 'just',
    detect: async (_workspacePath, readFile) => {
      const content = await readFile('Justfile') ?? await readFile('justfile')
      if (!content) return []
      return parseJustRecipes(content)
    },
    run: async (actionId, workspacePath) => {
      const name = actionId.slice('just:'.length)
      return daemonClient.createPtySession({ cwd: workspacePath, startupCommand: `just ${name}` })
    }
  }
}

function createTaskProvider(daemonClient: GrpcDaemonClient): RunActionProvider {
  return {
    source: 'task',
    detect: async (_workspacePath, readFile) => {
      const content = await readFile('Taskfile.yml') ?? await readFile('Taskfile.yaml')
      if (!content) return []
      return parseTaskfileNames(content)
    },
    run: async (actionId, workspacePath) => {
      const name = actionId.slice('task:'.length)
      return daemonClient.createPtySession({ cwd: workspacePath, startupCommand: `task ${name}` })
    }
  }
}

function createVscodeLaunchProvider(daemonClient: GrpcDaemonClient): RunActionProvider {
  return {
    source: 'vscode-launch',
    detect: async (_workspacePath, readFile) => {
      const content = await readFile('.vscode/launch.json')
      if (!content) return []
      return parseVscodeLaunch(content)
    },
    run: async (actionId, workspacePath) => {
      const name = actionId.slice('vscode-launch:'.length)
      // Can't fully replicate VS Code launch — run the resolved command as best-effort
      return daemonClient.createPtySession({ cwd: workspacePath, startupCommand: `echo "Launch config: ${name} (not directly runnable)"` })
    }
  }
}

function createVscodeTaskProvider(daemonClient: GrpcDaemonClient): RunActionProvider {
  return {
    source: 'vscode-task',
    detect: async (_workspacePath, readFile) => {
      const content = await readFile('.vscode/tasks.json')
      if (!content) return []
      return parseVscodeTasks(content)
    },
    run: async (actionId, workspacePath) => {
      const name = actionId.slice('vscode-task:'.length)
      // Re-parse to get the command
      const readFileFn: ReadFile = async (p) => {
        const result = await daemonClient.readFile(workspacePath, path.join(workspacePath, p))
        if (result.success) return result.file.content
        return null
      }
      const content = await readFileFn('.vscode/tasks.json')
      if (!content) throw new Error('tasks.json not found')
      const parsed = JSON.parse(stripJsoncComments(content)) as Record<string, unknown>
      const tasksArray = (parsed.tasks ?? []) as { label?: string; command?: string; args?: string[] }[]
      const task = tasksArray.find((t) => t.label === name)
      if (!task?.command) throw new Error(`Task "${name}" has no command`)
      const cmd = task.args ? `${task.command} ${task.args.join(' ')}` : task.command
      return daemonClient.createPtySession({ cwd: workspacePath, startupCommand: cmd })
    }
  }
}

// === Parse Helpers (exported for testing) ===

export function parseMakeTargets(content: string): RunAction[] {
  const actions: RunAction[] = []
  const regex = /^([a-zA-Z_][a-zA-Z0-9_.-]*)\s*:/gm
  let match: RegExpExecArray | null
  while ((match = regex.exec(content)) !== null) {
    const name = match[1]
    if (name.startsWith('.')) continue // skip .PHONY, .DEFAULT, etc.
    actions.push({ id: `make:${name}`, name, source: 'make', description: '' })
  }
  return actions
}

export function parseJustRecipes(content: string): RunAction[] {
  const actions: RunAction[] = []
  // Just recipes: lines starting with a name followed by optional params and ':'
  const regex = /^([a-zA-Z_][a-zA-Z0-9_-]*)\s*(?:[^:=]*):/gm
  let match: RegExpExecArray | null
  while ((match = regex.exec(content)) !== null) {
    const name = match[1]
    actions.push({ id: `just:${name}`, name, source: 'just', description: '' })
  }
  return actions
}

export function parseTaskfileNames(content: string): RunAction[] {
  const actions: RunAction[] = []
  // Match task names under "tasks:" section
  // Tasks are top-level keys under tasks: with 2-space indent
  const tasksMatch = content.match(/^tasks:\s*$/m)
  if (!tasksMatch) return actions
  const afterTasks = content.slice((tasksMatch.index ?? 0) + tasksMatch[0].length)
  const lines = afterTasks.split('\n')
  for (const line of lines) {
    // Stop at next top-level key
    if (/^[a-zA-Z]/.test(line) && !line.startsWith(' ')) break
    const taskMatch = line.match(/^  ([a-zA-Z_][a-zA-Z0-9_-]*):\s*$/)
    if (taskMatch) {
      const name = taskMatch[1]
      actions.push({ id: `task:${name}`, name, source: 'task', description: '' })
    }
  }
  return actions
}

export function stripJsoncComments(content: string): string {
  // Strip single-line comments (// ...) and multi-line comments (/* ... */)
  // Naive but works for typical .vscode JSON files
  return content
    .replace(/\/\/.*$/gm, '')
    .replace(/\/\*[\s\S]*?\*\//g, '')
}

export function parseVscodeLaunch(content: string): RunAction[] {
  const parsed = JSON.parse(stripJsoncComments(content)) as Record<string, unknown>
  const configs = (parsed.configurations ?? []) as { name: string; type?: string; program?: string }[]
  return configs.map((config): RunAction => ({
    id: `vscode-launch:${config.name}`,
    name: config.name,
    source: 'vscode-launch',
    description: config.type ? `${config.type}${config.program ? ` — ${config.program}` : ''}` : ''
  }))
}

export function parseVscodeTasks(content: string): RunAction[] {
  const parsed = JSON.parse(stripJsoncComments(content)) as Record<string, unknown>
  const tasks = (parsed.tasks ?? []) as { label?: string; command?: string; type?: string }[]
  return tasks
    .filter((t): t is { label: string; command?: string; type?: string } => Boolean(t.label))
    .map((t): RunAction => ({
      id: `vscode-task:${t.label}`,
      name: t.label,
      source: 'vscode-task',
      description: t.command ?? t.type ?? ''
    }))
}

// === RunActionsClient ===

export class RunActionsClient {
  private providers: RunActionProvider[]

  constructor(private daemonClient: GrpcDaemonClient, providers: RunActionProvider[]) {
    this.providers = providers
  }

  async detect(workspacePath: string): Promise<RunAction[]> {
    const readFile: ReadFile = async (filePath) => {
      try {
        const absolutePath = path.join(workspacePath, filePath)
        const result = await this.daemonClient.readFile(workspacePath, absolutePath)
        if (result.success) return result.file.content
        return null
      } catch {
        return null
      }
    }

    const results = await Promise.allSettled(
      this.providers.map(p => p.detect(workspacePath, readFile))
    )

    const actions: RunAction[] = []
    for (const result of results) {
      if (result.status === 'fulfilled') {
        actions.push(...result.value)
      }
    }
    return actions
  }

  async run(workspacePath: string, actionId: string): Promise<{ success: true; ptyId: string } | { success: false; error: string }> {
    const source = actionId.split(':')[0]
    const provider = this.providers.find(p => p.source === source)
    if (!provider) return { success: false, error: `No provider found for action source: ${source}` }
    const ptyId = await provider.run(actionId, workspacePath)
    return { success: true, ptyId }
  }
}

// === Factory ===

export function createRunActionsClient(daemonClient: GrpcDaemonClient): RunActionsClient {
  const providers: RunActionProvider[] = [
    createNpmProvider(daemonClient),
    createMakeProvider(daemonClient),
    createJustProvider(daemonClient),
    createTaskProvider(daemonClient),
    createVscodeLaunchProvider(daemonClient),
    createVscodeTaskProvider(daemonClient),
  ]
  return new RunActionsClient(daemonClient, providers)
}
