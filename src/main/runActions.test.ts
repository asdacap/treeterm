import { describe, it, expect } from 'vitest'
import {
  parseMakeTargets,
  parseJustRecipes,
  parseTaskfileNames,
  stripJsoncComments,
  parseVscodeLaunch,
  parseVscodeTasks,
} from './runActions'

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
})
