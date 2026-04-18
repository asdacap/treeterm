/* eslint-disable custom/no-string-literal-comparison -- TODO: migrate existing string-literal comparisons to enums */
/**
 * Run Actions client — discovers and executes project-defined tasks/commands.
 *
 * Moved from main process to renderer as part of the "main to light proxy" migration.
 * All file I/O goes through the FilesystemApi (daemon readFile via IPC).
 */

import type { RunAction } from '../../shared/types'
import type { IpcResult, IpcOk } from '../../shared/ipc-types'
import type { FilesystemApi, TerminalApi } from '../types'

type ReadFile = (relativePath: string) => Promise<string | null>

export interface RunActionsApi {
  detect: (workspacePath: string) => Promise<RunAction[]>
  run: (workspacePath: string, actionId: string) => Promise<IpcResult<{ ptyId: string }>>
}

// === Parse Helpers (exported for testing) ===

export function parseMakeTargets(content: string): RunAction[] {
  const actions: RunAction[] = []
  const regex = /^([a-zA-Z_][a-zA-Z0-9_.-]*)\s*:/gm
  let match: RegExpExecArray | null
  while ((match = regex.exec(content)) !== null) {
    const name = match[1]
    if (!name || name.startsWith('.')) continue // skip .PHONY, .DEFAULT, etc.
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
    if (!name) continue
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
      if (!name) continue
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

// === Provider system ===

interface RunActionProvider {
  source: string
  detect: (workspacePath: string, readFile: ReadFile) => Promise<RunAction[]>
  run: (actionId: string, workspacePath: string, readFile: ReadFile, createSession: (cwd: string, startupCommand: string) => Promise<IpcResult<{ sessionId: string }>>) => Promise<string>
}

function createNpmProvider(): RunActionProvider {
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
        description: scriptsRecord[name] ?? ''
      }))
    },
    run: async (actionId, workspacePath, _readFile, createSession) => {
      const name = actionId.slice('npm:'.length)
      const result = await createSession(workspacePath, `npm run ${name}`)
      if (!result.success) throw new Error(result.error)
      return result.sessionId
    }
  }
}

function createMakeProvider(): RunActionProvider {
  return {
    source: 'make',
    detect: async (_workspacePath, readFile) => {
      const content = await readFile('Makefile')
      if (!content) return []
      return parseMakeTargets(content)
    },
    run: async (actionId, workspacePath, _readFile, createSession) => {
      const name = actionId.slice('make:'.length)
      const result = await createSession(workspacePath, `make ${name}`)
      if (!result.success) throw new Error(result.error)
      return result.sessionId
    }
  }
}

function createJustProvider(): RunActionProvider {
  return {
    source: 'just',
    detect: async (_workspacePath, readFile) => {
      const content = await readFile('Justfile') ?? await readFile('justfile')
      if (!content) return []
      return parseJustRecipes(content)
    },
    run: async (actionId, workspacePath, _readFile, createSession) => {
      const name = actionId.slice('just:'.length)
      const result = await createSession(workspacePath, `just ${name}`)
      if (!result.success) throw new Error(result.error)
      return result.sessionId
    }
  }
}

function createTaskProvider(): RunActionProvider {
  return {
    source: 'task',
    detect: async (_workspacePath, readFile) => {
      const content = await readFile('Taskfile.yml') ?? await readFile('Taskfile.yaml')
      if (!content) return []
      return parseTaskfileNames(content)
    },
    run: async (actionId, workspacePath, _readFile, createSession) => {
      const name = actionId.slice('task:'.length)
      const result = await createSession(workspacePath, `task ${name}`)
      if (!result.success) throw new Error(result.error)
      return result.sessionId
    }
  }
}

function createVscodeLaunchProvider(): RunActionProvider {
  return {
    source: 'vscode-launch',
    detect: async (_workspacePath, readFile) => {
      const content = await readFile('.vscode/launch.json')
      if (!content) return []
      return parseVscodeLaunch(content)
    },
    run: async (actionId, workspacePath, _readFile, createSession) => {
      const name = actionId.slice('vscode-launch:'.length)
      // Can't fully replicate VS Code launch — run the resolved command as best-effort
      const result = await createSession(workspacePath, `echo "Launch config: ${name} (not directly runnable)"`)
      if (!result.success) throw new Error(result.error)
      return result.sessionId
    }
  }
}

function createVscodeTaskProvider(): RunActionProvider {
  return {
    source: 'vscode-task',
    detect: async (_workspacePath, readFile) => {
      const content = await readFile('.vscode/tasks.json')
      if (!content) return []
      return parseVscodeTasks(content)
    },
    run: async (actionId, _workspacePath, readFile, createSession) => {
      const name = actionId.slice('vscode-task:'.length)
      // Re-parse to get the command
      const content = await readFile('.vscode/tasks.json')
      if (!content) throw new Error('tasks.json not found')
      const parsed = JSON.parse(stripJsoncComments(content)) as Record<string, unknown>
      const tasksArray = (parsed.tasks ?? []) as { label?: string; command?: string; args?: string[] }[]
      const task = tasksArray.find((t) => t.label === name)
      if (!task?.command) throw new Error(`Task "${name}" has no command`)
      const cmd = task.args ? `${task.command} ${task.args.join(' ')}` : task.command
      const result = await createSession(_workspacePath, cmd)
      if (!result.success) throw new Error(result.error)
      return result.sessionId
    }
  }
}

// === Factory ===

export function createRunActionsApi(
  filesystem: FilesystemApi,
  terminal: TerminalApi,
  connectionId: string
): RunActionsApi {
  const providers: RunActionProvider[] = [
    createNpmProvider(),
    createMakeProvider(),
    createJustProvider(),
    createTaskProvider(),
    createVscodeLaunchProvider(),
    createVscodeTaskProvider(),
  ]

  const makeReadFile = (workspacePath: string): ReadFile => async (relativePath: string): Promise<string | null> => {
    try {
      const absolutePath = `${workspacePath}/${relativePath}`
      const result = await filesystem.readFile(workspacePath, absolutePath)
      if (result.success) return (result as IpcOk<{ file: { content: string } }>).file.content
      return null
    } catch {
      return null
    }
  }

  const createSession = (cwd: string, startupCommand: string): Promise<IpcResult<{ sessionId: string }>> => {
    return terminal.createSession(connectionId, cwd, startupCommand)
  }

  return {
    detect: async (workspacePath: string): Promise<RunAction[]> => {
      const readFile = makeReadFile(workspacePath)

      const results = await Promise.allSettled(
        providers.map(p => p.detect(workspacePath, readFile))
      )

      const actions: RunAction[] = []
      for (const result of results) {
        if (result.status === 'fulfilled') {
          actions.push(...result.value)
        }
      }
      return actions
    },

    run: async (workspacePath: string, actionId: string): Promise<IpcResult<{ ptyId: string }>> => {
      const source = actionId.split(':')[0]
      const provider = providers.find(p => p.source === source)
      if (!provider) return { success: false, error: `No provider found for action source: ${String(source)}` }
      try {
        const readFile = makeReadFile(workspacePath)
        const ptyId = await provider.run(actionId, workspacePath, readFile, createSession)
        return { success: true, ptyId }
      } catch (error) {
        return { success: false, error: error instanceof Error ? error.message : String(error) }
      }
    }
  }
}
