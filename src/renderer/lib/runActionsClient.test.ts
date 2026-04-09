import { describe, it, expect, vi } from 'vitest'
import {
  parseMakeTargets,
  parseJustRecipes,
  parseTaskfileNames,
  stripJsoncComments,
  parseVscodeLaunch,
  parseVscodeTasks,
  createRunActionsApi,
} from './runActionsClient'
import type { FilesystemApi, TerminalApi } from '../types'

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
    expect(actions[0]!.name).toBe('build')
  })

  it('handles targets with dots and dashes', () => {
    const content = `my-target.all:\n\techo hi\n`
    const actions = parseMakeTargets(content)
    expect(actions).toHaveLength(1)
    expect(actions[0]!.name).toBe('my-target.all')
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
    expect(actions[0]!.description).toBe('')
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
    expect(actions[0]!.description).toBe('typescript')
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
    expect(actions[0]!.name).toBe('Build')
  })
})

function createMockFilesystem(files: Record<string, string>): FilesystemApi {
  return {
    readDirectory: vi.fn(),
    readFile: vi.fn().mockImplementation((_workspacePath: string, filePath: string) => {
      // filePath is absolute (workspacePath/relativePath)
      for (const [relativePath, content] of Object.entries(files)) {
        if (filePath.endsWith(relativePath)) {
          return Promise.resolve({ success: true, file: { content, path: filePath, size: content.length, language: 'text' } })
        }
      }
      return Promise.resolve({ success: false, error: 'File not found' })
    }),
    writeFile: vi.fn(),
    searchFiles: vi.fn(),
  }
}

function createMockTerminal(sessionId: string = 'pty-1'): TerminalApi {
  return {
    create: vi.fn(),
    attach: vi.fn(),
    list: vi.fn(),
    write: vi.fn(),
    resize: vi.fn(),
    kill: vi.fn(),
    onEvent: vi.fn(),
    onActiveProcessesOpen: vi.fn(),
    createSession: vi.fn().mockResolvedValue({ success: true, sessionId }),
  }
}

describe('createRunActionsApi', () => {
  it('detect aggregates actions from all providers', async () => {
    const filesystem = createMockFilesystem({
      'package.json': '{"scripts":{"build":"echo build"}}',
      'Makefile': 'test:\n\techo test\n',
    })
    const terminal = createMockTerminal()
    const api = createRunActionsApi(filesystem, terminal, 'local')
    const actions = await api.detect('/workspace')
    const npmActions = actions.filter(a => a.source === 'npm')
    const makeActions = actions.filter(a => a.source === 'make')
    expect(npmActions.length).toBeGreaterThanOrEqual(1)
    expect(makeActions.length).toBeGreaterThanOrEqual(1)
  })

  it('detect returns empty when no files found', async () => {
    const filesystem = createMockFilesystem({})
    const terminal = createMockTerminal()
    const api = createRunActionsApi(filesystem, terminal, 'local')
    const actions = await api.detect('/workspace')
    expect(actions).toEqual([])
  })

  it('detect handles readFile errors gracefully', async () => {
    const filesystem: FilesystemApi = {
      readDirectory: vi.fn(),
      readFile: vi.fn().mockRejectedValue(new Error('read error')),
      writeFile: vi.fn(),
      searchFiles: vi.fn(),
    }
    const terminal = createMockTerminal()
    const api = createRunActionsApi(filesystem, terminal, 'local')
    const actions = await api.detect('/workspace')
    expect(actions).toEqual([])
  })

  it('detect returns empty when package.json has no scripts', async () => {
    const filesystem = createMockFilesystem({
      'package.json': '{"name": "test"}',
    })
    const terminal = createMockTerminal()
    const api = createRunActionsApi(filesystem, terminal, 'local')
    const actions = await api.detect('/workspace')
    const npmActions = actions.filter(a => a.source === 'npm')
    expect(npmActions).toHaveLength(0)
  })

  it('detect returns empty when package.json scripts is not an object', async () => {
    const filesystem = createMockFilesystem({
      'package.json': '{"scripts": "invalid"}',
    })
    const terminal = createMockTerminal()
    const api = createRunActionsApi(filesystem, terminal, 'local')
    const actions = await api.detect('/workspace')
    const npmActions = actions.filter(a => a.source === 'npm')
    expect(npmActions).toHaveLength(0)
  })

  it('detect finds Taskfile.yaml when Taskfile.yml missing', async () => {
    const filesystem = createMockFilesystem({
      'Taskfile.yaml': "version: '3'\n\ntasks:\n  build:\n    cmds:\n      - echo build\n",
    })
    const terminal = createMockTerminal()
    const api = createRunActionsApi(filesystem, terminal, 'local')
    const actions = await api.detect('/workspace')
    const taskActions = actions.filter(a => a.source === 'task')
    expect(taskActions.length).toBeGreaterThanOrEqual(1)
  })

  it('detect finds justfile with lowercase fallback', async () => {
    const filesystem = createMockFilesystem({
      'justfile': 'build:\n  echo build\n',
    })
    const terminal = createMockTerminal()
    const api = createRunActionsApi(filesystem, terminal, 'local')
    const actions = await api.detect('/workspace')
    const justActions = actions.filter(a => a.source === 'just')
    expect(justActions.length).toBeGreaterThanOrEqual(1)
  })

  it('run npm action creates session with correct command', async () => {
    const filesystem = createMockFilesystem({})
    const terminal = createMockTerminal('pty-npm')
    const api = createRunActionsApi(filesystem, terminal, 'conn-1')
    const result = await api.run('/workspace', 'npm:build')
    expect(result).toMatchObject({ success: true, ptyId: 'pty-npm' })
    expect(terminal.createSession).toHaveBeenCalledWith('conn-1', '/workspace', 'npm run build')
  })

  it('run make action creates session with correct command', async () => {
    const filesystem = createMockFilesystem({})
    const terminal = createMockTerminal('pty-make')
    const api = createRunActionsApi(filesystem, terminal, 'conn-1')
    await api.run('/workspace', 'make:test')
    expect(terminal.createSession).toHaveBeenCalledWith('conn-1', '/workspace', 'make test')
  })

  it('run just action creates session with correct command', async () => {
    const filesystem = createMockFilesystem({})
    const terminal = createMockTerminal('pty-just')
    const api = createRunActionsApi(filesystem, terminal, 'conn-1')
    await api.run('/workspace', 'just:deploy')
    expect(terminal.createSession).toHaveBeenCalledWith('conn-1', '/workspace', 'just deploy')
  })

  it('run task action creates session with correct command', async () => {
    const filesystem = createMockFilesystem({})
    const terminal = createMockTerminal('pty-task')
    const api = createRunActionsApi(filesystem, terminal, 'conn-1')
    await api.run('/workspace', 'task:lint')
    expect(terminal.createSession).toHaveBeenCalledWith('conn-1', '/workspace', 'task lint')
  })

  it('run vscode-launch action creates session with echo message', async () => {
    const filesystem = createMockFilesystem({})
    const terminal = createMockTerminal('pty-launch')
    const api = createRunActionsApi(filesystem, terminal, 'conn-1')
    await api.run('/workspace', 'vscode-launch:Debug App')
    expect(terminal.createSession).toHaveBeenCalledWith('conn-1', '/workspace', 'echo "Launch config: Debug App (not directly runnable)"')
  })

  it('run vscode-task re-reads tasks.json and runs command', async () => {
    const tasksJson = JSON.stringify({
      version: '2.0.0',
      tasks: [{ label: 'Build', command: 'npm', args: ['run', 'build'] }]
    })
    const filesystem = createMockFilesystem({
      '.vscode/tasks.json': tasksJson,
    })
    const terminal = createMockTerminal('pty-vscode')
    const api = createRunActionsApi(filesystem, terminal, 'conn-1')
    await api.run('/workspace', 'vscode-task:Build')
    expect(terminal.createSession).toHaveBeenCalledWith('conn-1', '/workspace', 'npm run build')
  })

  it('run vscode-task returns error when tasks.json not found', async () => {
    const filesystem = createMockFilesystem({})
    const terminal = createMockTerminal()
    const api = createRunActionsApi(filesystem, terminal, 'conn-1')
    const result = await api.run('/workspace', 'vscode-task:Build')
    expect(result).toMatchObject({ success: false, error: 'tasks.json not found' })
  })

  it('run vscode-task returns error when task has no command', async () => {
    const tasksJson = JSON.stringify({
      tasks: [{ label: 'NoCmd' }]
    })
    const filesystem = createMockFilesystem({
      '.vscode/tasks.json': tasksJson,
    })
    const terminal = createMockTerminal()
    const api = createRunActionsApi(filesystem, terminal, 'conn-1')
    const result = await api.run('/workspace', 'vscode-task:NoCmd')
    expect(result).toMatchObject({ success: false, error: 'Task "NoCmd" has no command' })
  })

  it('run vscode-task runs command without args when no args specified', async () => {
    const tasksJson = JSON.stringify({
      tasks: [{ label: 'Lint', command: 'eslint .' }]
    })
    const filesystem = createMockFilesystem({
      '.vscode/tasks.json': tasksJson,
    })
    const terminal = createMockTerminal('pty-lint')
    const api = createRunActionsApi(filesystem, terminal, 'conn-1')
    await api.run('/workspace', 'vscode-task:Lint')
    expect(terminal.createSession).toHaveBeenCalledWith('conn-1', '/workspace', 'eslint .')
  })

  it('run returns error for unknown provider', async () => {
    const filesystem = createMockFilesystem({})
    const terminal = createMockTerminal()
    const api = createRunActionsApi(filesystem, terminal, 'local')
    const result = await api.run('/workspace', 'unknown:action')
    // eslint-disable-next-line @typescript-eslint/no-unsafe-assignment
    expect(result).toMatchObject({ success: false, error: expect.stringContaining('No provider found') })
  })

  it('run returns error when createSession fails', async () => {
    const filesystem = createMockFilesystem({})
    const terminal = createMockTerminal()
    ;(terminal.createSession as ReturnType<typeof vi.fn>).mockResolvedValue({ success: false, error: 'daemon down' })
    const api = createRunActionsApi(filesystem, terminal, 'local')
    const result = await api.run('/workspace', 'npm:build')
    expect(result).toMatchObject({ success: false, error: 'daemon down' })
  })
})
