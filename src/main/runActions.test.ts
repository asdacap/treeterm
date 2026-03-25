import { describe, it, expect, vi } from 'vitest'
import {
  parseMakeTargets,
  parseJustRecipes,
  parseTaskfileNames,
  stripJsoncComments,
  parseVscodeLaunch,
  parseVscodeTasks,
  RunActionsClient,
  createRunActionsClient,
} from './runActions'
import type { RunActionProvider } from './runActions'

describe('parseMakeTargets', () => {
  it('parses simple targets', () => {
    const content = `
build:
\techo build

test:
\techo test

clean:
\trm -rf dist
`
    const actions = parseMakeTargets(content)
    expect(actions).toHaveLength(3)
    expect(actions[0]).toEqual({ id: 'make:build', name: 'build', source: 'make', description: '' })
    expect(actions[1]).toEqual({ id: 'make:test', name: 'test', source: 'make', description: '' })
    expect(actions[2]).toEqual({ id: 'make:clean', name: 'clean', source: 'make', description: '' })
  })

  it('skips .PHONY and dotted special targets', () => {
    const content = `.PHONY: build test
.DEFAULT:
build:
\techo build
`
    const actions = parseMakeTargets(content)
    expect(actions).toHaveLength(1)
    expect(actions[0].name).toBe('build')
  })

  it('handles targets with dots and dashes', () => {
    const content = `my-target.all:\n\techo hi\n`
    const actions = parseMakeTargets(content)
    expect(actions).toHaveLength(1)
    expect(actions[0].name).toBe('my-target.all')
  })

  it('returns empty for empty content', () => {
    expect(parseMakeTargets('')).toEqual([])
  })
})

describe('parseJustRecipes', () => {
  it('parses simple recipes', () => {
    const content = `
build:
  echo build

test arg:
  echo test

clean:
  rm -rf dist
`
    const actions = parseJustRecipes(content)
    expect(actions).toHaveLength(3)
    expect(actions.map(a => a.name)).toEqual(['build', 'test', 'clean'])
  })

  it('returns empty for empty content', () => {
    expect(parseJustRecipes('')).toEqual([])
  })
})

describe('parseTaskfileNames', () => {
  it('parses task names under tasks section', () => {
    const content = `version: '3'

tasks:
  build:
    cmds:
      - go build
  test:
    cmds:
      - go test
`
    const actions = parseTaskfileNames(content)
    expect(actions).toHaveLength(2)
    expect(actions[0]).toEqual({ id: 'task:build', name: 'build', source: 'task', description: '' })
    expect(actions[1]).toEqual({ id: 'task:test', name: 'test', source: 'task', description: '' })
  })

  it('returns empty when no tasks section', () => {
    expect(parseTaskfileNames('version: 3')).toEqual([])
  })
})

describe('stripJsoncComments', () => {
  it('strips single-line comments', () => {
    const input = `{
  // This is a comment
  "name": "test"
}`
    expect(stripJsoncComments(input)).toContain('"name": "test"')
    expect(stripJsoncComments(input)).not.toContain('// This')
  })

  it('strips multi-line comments', () => {
    const input = `{
  /* multi
     line */
  "name": "test"
}`
    expect(stripJsoncComments(input)).toContain('"name": "test"')
    expect(stripJsoncComments(input)).not.toContain('multi')
  })
})

describe('parseVscodeLaunch', () => {
  it('parses launch configurations', () => {
    const content = `{
      // launch config
      "version": "0.2.0",
      "configurations": [
        {
          "name": "Launch Program",
          "type": "node",
          "program": "index.js"
        },
        {
          "name": "Debug Tests",
          "type": "node"
        }
      ]
    }`
    const actions = parseVscodeLaunch(content)
    expect(actions).toHaveLength(2)
    expect(actions[0]).toEqual({
      id: 'vscode-launch:Launch Program',
      name: 'Launch Program',
      source: 'vscode-launch',
      description: 'node — index.js'
    })
    expect(actions[1]).toEqual({
      id: 'vscode-launch:Debug Tests',
      name: 'Debug Tests',
      source: 'vscode-launch',
      description: 'node'
    })
  })

  it('returns empty description when config has no type', () => {
    const actions = parseVscodeLaunch('{"configurations": [{"name": "Run"}]}')
    expect(actions[0].description).toBe('')
  })

  it('returns empty for no configurations', () => {
    const actions = parseVscodeLaunch('{"version": "0.2.0"}')
    expect(actions).toEqual([])
  })
})

describe('parseVscodeTasks', () => {
  it('parses shell tasks', () => {
    const content = `{
      "version": "2.0.0",
      "tasks": [
        {
          "label": "Build",
          "type": "shell",
          "command": "npm run build"
        },
        {
          "label": "Test",
          "command": "npm test"
        }
      ]
    }`
    const actions = parseVscodeTasks(content)
    expect(actions).toHaveLength(2)
    expect(actions[0]).toEqual({
      id: 'vscode-task:Build',
      name: 'Build',
      source: 'vscode-task',
      description: 'npm run build'
    })
  })

  it('returns empty for no tasks', () => {
    expect(parseVscodeTasks('{"version": "2.0.0"}')).toEqual([])
  })

  it('uses type as description when no command', () => {
    const content = '{"tasks": [{"label": "Watch", "type": "typescript"}]}'
    const actions = parseVscodeTasks(content)
    expect(actions[0].description).toBe('typescript')
  })

  it('filters tasks without labels', () => {
    const content = `{
      "tasks": [
        { "command": "echo hi" },
        { "label": "Build", "command": "npm run build" }
      ]
    }`
    const actions = parseVscodeTasks(content)
    expect(actions).toHaveLength(1)
    expect(actions[0].name).toBe('Build')
  })
})

describe('RunActionsClient', () => {
  function makeProvider(overrides?: Partial<RunActionProvider>): RunActionProvider {
    return {
      source: 'test',
      detect: vi.fn().mockResolvedValue([]),
      run: vi.fn().mockResolvedValue('pty-1'),
      ...overrides,
    }
  }

  it('detect aggregates actions from all providers', async () => {
    const p1 = makeProvider({
      source: 'npm',
      detect: vi.fn().mockResolvedValue([{ id: 'npm:build', name: 'build', source: 'npm', description: '' }]),
    })
    const p2 = makeProvider({
      source: 'make',
      detect: vi.fn().mockResolvedValue([{ id: 'make:test', name: 'test', source: 'make', description: '' }]),
    })
    const client = new RunActionsClient({} as any, [p1, p2])
    const actions = await client.detect('/workspace')
    expect(actions).toHaveLength(2)
    expect(actions[0].id).toBe('npm:build')
    expect(actions[1].id).toBe('make:test')
  })

  it('detect handles provider failures gracefully', async () => {
    const p1 = makeProvider({
      source: 'npm',
      detect: vi.fn().mockRejectedValue(new Error('parse error')),
    })
    const p2 = makeProvider({
      source: 'make',
      detect: vi.fn().mockResolvedValue([{ id: 'make:all', name: 'all', source: 'make', description: '' }]),
    })
    const client = new RunActionsClient({} as any, [p1, p2])
    const actions = await client.detect('/workspace')
    expect(actions).toHaveLength(1)
    expect(actions[0].id).toBe('make:all')
  })

  it('detect returns empty when all providers fail', async () => {
    const p1 = makeProvider({ detect: vi.fn().mockRejectedValue(new Error('fail')) })
    const client = new RunActionsClient({} as any, [p1])
    const actions = await client.detect('/workspace')
    expect(actions).toEqual([])
  })

  it('run dispatches to correct provider', async () => {
    const p1 = makeProvider({ source: 'npm', run: vi.fn().mockResolvedValue('pty-npm') })
    const p2 = makeProvider({ source: 'make', run: vi.fn().mockResolvedValue('pty-make') })
    const client = new RunActionsClient({} as any, [p1, p2])

    const result = await client.run('/workspace', 'make:build')
    expect(result).toBe('pty-make')
    expect(p2.run).toHaveBeenCalledWith('make:build', '/workspace')
    expect(p1.run).not.toHaveBeenCalled()
  })

  it('run throws for unknown provider', async () => {
    const client = new RunActionsClient({} as any, [])
    await expect(client.run('/workspace', 'unknown:action')).rejects.toThrow('No provider found')
  })

  it('detect returns empty when package.json has no scripts', async () => {
    const mockDaemonClient = {
      readFile: vi.fn().mockResolvedValue({
        success: true,
        file: { content: '{"name": "test"}' },
      }),
      createPtySession: vi.fn(),
    }

    const client = createRunActionsClient(mockDaemonClient as any)
    const actions = await client.detect('/workspace')
    const npmActions = actions.filter(a => a.source === 'npm')
    expect(npmActions).toHaveLength(0)
  })

  it('detect returns empty when package.json scripts is not an object', async () => {
    const mockDaemonClient = {
      readFile: vi.fn().mockResolvedValue({
        success: true,
        file: { content: '{"scripts": "invalid"}' },
      }),
      createPtySession: vi.fn(),
    }

    const client = createRunActionsClient(mockDaemonClient as any)
    const actions = await client.detect('/workspace')
    const npmActions = actions.filter(a => a.source === 'npm')
    expect(npmActions).toHaveLength(0)
  })

  it('detect finds Taskfile.yaml when Taskfile.yml missing', async () => {
    const taskfileContent = 'version: \'3\'\n\ntasks:\n  build:\n    cmds:\n      - echo build\n'
    const mockDaemonClient = {
      readFile: vi.fn().mockImplementation((_ws: string, absPath: string) => {
        if (absPath.endsWith('Taskfile.yml')) return Promise.resolve({ success: false })
        if (absPath.endsWith('Taskfile.yaml')) return Promise.resolve({ success: true, file: { content: taskfileContent } })
        return Promise.resolve({ success: false })
      }),
      createPtySession: vi.fn(),
    }

    const client = createRunActionsClient(mockDaemonClient as any)
    const actions = await client.detect('/workspace')
    const taskActions = actions.filter(a => a.source === 'task')
    expect(taskActions.length).toBeGreaterThanOrEqual(1)
  })

  it('detect finds Justfile with lowercase fallback', async () => {
    let callCount = 0
    const mockDaemonClient = {
      readFile: vi.fn().mockImplementation((_ws: string, absPath: string) => {
        // Return null for Justfile (uppercase), content for justfile (lowercase)
        if (absPath.endsWith('Justfile')) return Promise.resolve({ success: false })
        if (absPath.endsWith('justfile')) return Promise.resolve({ success: true, file: { content: 'build:\n  echo build\n' } })
        return Promise.resolve({ success: false })
      }),
      createPtySession: vi.fn(),
    }

    const client = createRunActionsClient(mockDaemonClient as any)
    const actions = await client.detect('/workspace')
    const justActions = actions.filter(a => a.source === 'just')
    expect(justActions.length).toBeGreaterThanOrEqual(1)
  })

  it('detect reads files via daemonClient', async () => {
    const mockDaemonClient = {
      readFile: vi.fn().mockResolvedValue({
        success: true,
        file: { content: '{"scripts":{"build":"echo build"}}' },
      }),
      createPtySession: vi.fn().mockResolvedValue('pty-1'),
    }

    const client = createRunActionsClient(mockDaemonClient as any)
    const actions = await client.detect('/workspace')

    // Should have found at least the npm build script
    const npmActions = actions.filter(a => a.source === 'npm')
    expect(npmActions.length).toBeGreaterThanOrEqual(1)
    expect(npmActions[0].name).toBe('build')
  })

  it('detect handles readFile returning null content', async () => {
    const mockDaemonClient = {
      readFile: vi.fn().mockResolvedValue({ success: false }),
      createPtySession: vi.fn(),
    }

    const client = createRunActionsClient(mockDaemonClient as any)
    const actions = await client.detect('/workspace')
    expect(actions).toEqual([])
  })

  it('detect handles readFile throwing', async () => {
    const mockDaemonClient = {
      readFile: vi.fn().mockRejectedValue(new Error('read error')),
      createPtySession: vi.fn(),
    }

    const client = createRunActionsClient(mockDaemonClient as any)
    const actions = await client.detect('/workspace')
    expect(actions).toEqual([])
  })

  it('run npm action creates pty with correct command', async () => {
    const mockDaemonClient = {
      readFile: vi.fn(),
      createPtySession: vi.fn().mockResolvedValue('pty-npm'),
    }

    const client = createRunActionsClient(mockDaemonClient as any)
    const result = await client.run('/workspace', 'npm:build')
    expect(result).toBe('pty-npm')
    expect(mockDaemonClient.createPtySession).toHaveBeenCalledWith({
      cwd: '/workspace',
      startupCommand: 'npm run build',
    })
  })

  it('run make action creates pty with correct command', async () => {
    const mockDaemonClient = {
      readFile: vi.fn(),
      createPtySession: vi.fn().mockResolvedValue('pty-make'),
    }

    const client = createRunActionsClient(mockDaemonClient as any)
    const result = await client.run('/workspace', 'make:test')
    expect(mockDaemonClient.createPtySession).toHaveBeenCalledWith({
      cwd: '/workspace',
      startupCommand: 'make test',
    })
  })

  it('run just action creates pty with correct command', async () => {
    const mockDaemonClient = {
      readFile: vi.fn(),
      createPtySession: vi.fn().mockResolvedValue('pty-just'),
    }

    const client = createRunActionsClient(mockDaemonClient as any)
    await client.run('/workspace', 'just:deploy')
    expect(mockDaemonClient.createPtySession).toHaveBeenCalledWith({
      cwd: '/workspace',
      startupCommand: 'just deploy',
    })
  })

  it('run task action creates pty with correct command', async () => {
    const mockDaemonClient = {
      readFile: vi.fn(),
      createPtySession: vi.fn().mockResolvedValue('pty-task'),
    }

    const client = createRunActionsClient(mockDaemonClient as any)
    await client.run('/workspace', 'task:lint')
    expect(mockDaemonClient.createPtySession).toHaveBeenCalledWith({
      cwd: '/workspace',
      startupCommand: 'task lint',
    })
  })

  it('run vscode-launch action creates pty with echo message', async () => {
    const mockDaemonClient = {
      readFile: vi.fn(),
      createPtySession: vi.fn().mockResolvedValue('pty-launch'),
    }

    const client = createRunActionsClient(mockDaemonClient as any)
    await client.run('/workspace', 'vscode-launch:Debug App')
    expect(mockDaemonClient.createPtySession).toHaveBeenCalledWith({
      cwd: '/workspace',
      startupCommand: 'echo "Launch config: Debug App (not directly runnable)"',
    })
  })

  it('run vscode-task action re-reads tasks.json and runs command', async () => {
    const tasksJson = JSON.stringify({
      version: '2.0.0',
      tasks: [{ label: 'Build', command: 'npm', args: ['run', 'build'] }]
    })
    const mockDaemonClient = {
      readFile: vi.fn().mockResolvedValue({ success: true, file: { content: tasksJson } }),
      createPtySession: vi.fn().mockResolvedValue('pty-vscode'),
    }

    const client = createRunActionsClient(mockDaemonClient as any)
    await client.run('/workspace', 'vscode-task:Build')
    expect(mockDaemonClient.createPtySession).toHaveBeenCalledWith({
      cwd: '/workspace',
      startupCommand: 'npm run build',
    })
  })

  it('run vscode-task throws when tasks.json not found', async () => {
    const mockDaemonClient = {
      readFile: vi.fn().mockResolvedValue({ success: false }),
      createPtySession: vi.fn(),
    }

    const client = createRunActionsClient(mockDaemonClient as any)
    await expect(client.run('/workspace', 'vscode-task:Build')).rejects.toThrow('tasks.json not found')
  })

  it('run vscode-task throws when task has no command', async () => {
    const tasksJson = JSON.stringify({
      tasks: [{ label: 'NoCmd' }]
    })
    const mockDaemonClient = {
      readFile: vi.fn().mockResolvedValue({ success: true, file: { content: tasksJson } }),
      createPtySession: vi.fn(),
    }

    const client = createRunActionsClient(mockDaemonClient as any)
    await expect(client.run('/workspace', 'vscode-task:NoCmd')).rejects.toThrow('Task "NoCmd" has no command')
  })

  it('run vscode-task runs command without args when no args specified', async () => {
    const tasksJson = JSON.stringify({
      tasks: [{ label: 'Lint', command: 'eslint .' }]
    })
    const mockDaemonClient = {
      readFile: vi.fn().mockResolvedValue({ success: true, file: { content: tasksJson } }),
      createPtySession: vi.fn().mockResolvedValue('pty-lint'),
    }

    const client = createRunActionsClient(mockDaemonClient as any)
    await client.run('/workspace', 'vscode-task:Lint')
    expect(mockDaemonClient.createPtySession).toHaveBeenCalledWith({
      cwd: '/workspace',
      startupCommand: 'eslint .',
    })
  })
})
