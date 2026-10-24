import { describe, it, expect, vi } from 'vitest'
import {
  MAX_REPO_HITS,
  REPO_SEARCH_TIMEOUT_MS,
  RepoSearchEventType,
  RepoSearchTool,
  parseVimgrepLine,
  startRepoSearch,
  toWorkspaceAbsolute,
  toWorkspaceRelative,
  type RepoSearchEvent,
} from './repoSearch'
import type { ExecApi } from '../types'
import { ExecEventType, type ExecEvent } from '../../shared/ipc-types'

// An ExecApi whose execs are driven by the test: each start() gets a fresh id, and
// emit(id, event) delivers an event to that exec's subscriber.
function createControlledExecApi(startResult?: { success: false; error: string }): {
  execApi: ExecApi
  emit: (execId: string, event: ExecEvent) => void
  resolveStart: () => Promise<void>
  unsubscribed: string[]
} {
  const callbacks = new Map<string, (event: ExecEvent) => void>()
  const unsubscribed: string[] = []
  let count = 0
  let pending: (() => void) | null = null
  const execApi: ExecApi = {
    start: vi.fn().mockImplementation(() => new Promise((resolve) => {
      count++
      const execId = `exec-${String(count)}`
      pending = () => { resolve(startResult ?? { success: true, execId }) }
    })),
    kill: vi.fn(),
    onEvent: vi.fn().mockImplementation((execId: string, cb: (event: ExecEvent) => void) => {
      callbacks.set(execId, cb)
      return () => {
        unsubscribed.push(execId)
        callbacks.delete(execId)
      }
    }),
  }
  return {
    execApi,
    emit: (execId, event) => { callbacks.get(execId)?.(event) },
    resolveStart: async () => {
      pending?.()
      pending = null
      await Promise.resolve()
      await Promise.resolve()
    },
    unsubscribed,
  }
}

function collect(): { events: RepoSearchEvent[]; onEvent: (event: RepoSearchEvent) => void } {
  const events: RepoSearchEvent[] = []
  return { events, onEvent: (event) => { events.push(event) } }
}

describe('parseVimgrepLine', () => {
  it('parses path, line, column and text, keeping colons in the text', () => {
    expect(parseVimgrepLine('./src/a.ts:12:5:const x = { a: 1 }')).toEqual({
      filePath: 'src/a.ts', lineNumber: 12, column: 5, preview: 'const x = { a: 1 }',
    })
  })

  it('keeps paths without a ./ prefix as they are', () => {
    expect(parseVimgrepLine('src/a.ts:1:1:x')).toMatchObject({ filePath: 'src/a.ts' })
  })

  it('returns an error for unexpected output', () => {
    expect(parseVimgrepLine('garbage')).toBeInstanceOf(Error)
  })
})

describe('startRepoSearch', () => {
  it('runs rg with smart-case fixed-string search', async () => {
    const { execApi, resolveStart } = createControlledExecApi()
    startRepoSearch(execApi, 'conn', '/repo', 'foo', vi.fn())
    await resolveStart()
    expect(execApi.start).toHaveBeenCalledWith(
      'conn', '/repo', 'rg',
      ['--vimgrep', '--fixed-strings', '--smart-case', '--max-columns', '300', '--max-columns-preview', '--', 'foo', '.'],
      REPO_SEARCH_TIMEOUT_MS,
    )
  })

  it('streams hits as complete lines arrive, across chunk boundaries', async () => {
    const { execApi, emit, resolveStart } = createControlledExecApi()
    const { events, onEvent } = collect()
    startRepoSearch(execApi, 'conn', '/repo', 'foo', onEvent)
    await resolveStart()

    emit('exec-1', { type: ExecEventType.Stdout, data: './a.ts:1:1:foo\n./b.t' })
    expect(events).toEqual([
      { type: RepoSearchEventType.Hits, hits: [{ filePath: 'a.ts', lineNumber: 1, column: 1, preview: 'foo' }] },
    ])
    emit('exec-1', { type: ExecEventType.Stdout, data: 's:2:3:foo' })
    emit('exec-1', { type: ExecEventType.Exit, exitCode: 0 })
    expect(events.slice(1)).toEqual([
      { type: RepoSearchEventType.Hits, hits: [{ filePath: 'b.ts', lineNumber: 2, column: 3, preview: 'foo' }] },
      { type: RepoSearchEventType.Done, tool: RepoSearchTool.Ripgrep, truncated: false },
    ])
  })

  it('treats exit code 1 as no matches', async () => {
    const { execApi, emit, resolveStart } = createControlledExecApi()
    const { events, onEvent } = collect()
    startRepoSearch(execApi, 'conn', '/repo', 'foo', onEvent)
    await resolveStart()
    emit('exec-1', { type: ExecEventType.Exit, exitCode: 1 })
    expect(events).toEqual([{ type: RepoSearchEventType.Done, tool: RepoSearchTool.Ripgrep, truncated: false }])
  })

  it('reports other exit codes as errors with stderr', async () => {
    const { execApi, emit, resolveStart } = createControlledExecApi()
    const { events, onEvent } = collect()
    startRepoSearch(execApi, 'conn', '/repo', 'foo', onEvent)
    await resolveStart()
    emit('exec-1', { type: ExecEventType.Stderr, data: 'bad regex\n' })
    emit('exec-1', { type: ExecEventType.Exit, exitCode: 2 })
    expect(events).toEqual([{ type: RepoSearchEventType.Error, message: 'rg exited with code 2: bad regex' }])
  })

  it('reports unparseable output as an error and stops', async () => {
    const { execApi, emit, resolveStart } = createControlledExecApi()
    const { events, onEvent } = collect()
    startRepoSearch(execApi, 'conn', '/repo', 'foo', onEvent)
    await resolveStart()
    emit('exec-1', { type: ExecEventType.Stdout, data: 'garbage\n' })
    expect(events).toEqual([{ type: RepoSearchEventType.Error, message: 'Unexpected search output: garbage' }])
    expect(execApi.kill).toHaveBeenCalledWith('exec-1')
  })

  it.each([
    ['foo', ['grep', '-n', '--column', '-I', '-F', '-i', '-e', 'foo']],
    ['Foo', ['grep', '-n', '--column', '-I', '-F', '-e', 'Foo']],
  ])('falls back to git grep when rg cannot be spawned (%s)', async (query, gitArgs) => {
    const { execApi, emit, resolveStart } = createControlledExecApi()
    const { events, onEvent } = collect()
    startRepoSearch(execApi, 'conn', '/repo', query, onEvent)
    await resolveStart()
    emit('exec-1', { type: ExecEventType.Error, message: 'spawn error: No such file or directory (os error 2)' })
    await resolveStart()

    expect(execApi.start).toHaveBeenLastCalledWith('conn', '/repo', 'git', gitArgs, REPO_SEARCH_TIMEOUT_MS)
    emit('exec-2', { type: ExecEventType.Stdout, data: `a.ts:4:2:${query}\n` })
    emit('exec-2', { type: ExecEventType.Exit, exitCode: 0 })
    expect(events).toEqual([
      { type: RepoSearchEventType.Hits, hits: [{ filePath: 'a.ts', lineNumber: 4, column: 2, preview: query }] },
      { type: RepoSearchEventType.Done, tool: RepoSearchTool.GitGrep, truncated: false },
    ])
  })

  it('reports other exec errors, including from git grep', async () => {
    const { execApi, emit, resolveStart } = createControlledExecApi()
    const { events, onEvent } = collect()
    startRepoSearch(execApi, 'conn', '/repo', 'foo', onEvent)
    await resolveStart()
    emit('exec-1', { type: ExecEventType.Error, message: 'spawn error: missing' })
    await resolveStart()
    emit('exec-2', { type: ExecEventType.Error, message: 'spawn error: missing' })
    expect(events).toEqual([{ type: RepoSearchEventType.Error, message: 'spawn error: missing' }])
  })

  it('reports a failed start', async () => {
    const { execApi, resolveStart } = createControlledExecApi({ success: false, error: 'no connection' })
    const { events, onEvent } = collect()
    startRepoSearch(execApi, 'conn', '/repo', 'foo', onEvent)
    await resolveStart()
    expect(events).toEqual([{ type: RepoSearchEventType.Error, message: 'no connection' }])
  })

  it('reports a rejected start', async () => {
    const execApi: ExecApi = {
      start: vi.fn().mockRejectedValue(new Error('ipc down')),
      kill: vi.fn(),
      onEvent: vi.fn(),
    }
    const { events, onEvent } = collect()
    startRepoSearch(execApi, 'conn', '/repo', 'foo', onEvent)
    await new Promise((r) => setTimeout(r, 0))
    expect(events).toEqual([{ type: RepoSearchEventType.Error, message: 'Error: ipc down' }])
  })

  it('stops at the hit cap, kills the exec and reports truncation', async () => {
    const { execApi, emit, resolveStart, unsubscribed } = createControlledExecApi()
    const { events, onEvent } = collect()
    startRepoSearch(execApi, 'conn', '/repo', 'foo', onEvent)
    await resolveStart()
    const lines = Array.from({ length: MAX_REPO_HITS + 5 }, (_, i) => `a.ts:${String(i + 1)}:1:foo`).join('\n')
    emit('exec-1', { type: ExecEventType.Stdout, data: `${lines}\n` })

    const hits = events.flatMap(e => e.type === RepoSearchEventType.Hits ? e.hits : [])
    expect(hits).toHaveLength(MAX_REPO_HITS)
    expect(events.at(-1)).toEqual({ type: RepoSearchEventType.Done, tool: RepoSearchTool.Ripgrep, truncated: true })
    expect(execApi.kill).toHaveBeenCalledWith('exec-1')
    expect(unsubscribed).toContain('exec-1')
  })

  it('cancel kills the running exec and delivers nothing more', async () => {
    const { execApi, emit, resolveStart } = createControlledExecApi()
    const { events, onEvent } = collect()
    const cancel = startRepoSearch(execApi, 'conn', '/repo', 'foo', onEvent)
    await resolveStart()
    cancel()
    emit('exec-1', { type: ExecEventType.Stdout, data: 'a.ts:1:1:foo\n' })
    expect(execApi.kill).toHaveBeenCalledWith('exec-1')
    expect(events).toEqual([])
  })

  it('cancel before the exec has started kills it once it starts', async () => {
    const { execApi, resolveStart } = createControlledExecApi()
    const { events, onEvent } = collect()
    const cancel = startRepoSearch(execApi, 'conn', '/repo', 'foo', onEvent)
    cancel()
    await resolveStart()
    expect(execApi.kill).toHaveBeenCalledWith('exec-1')
    expect(execApi.onEvent).not.toHaveBeenCalled()
    expect(events).toEqual([])
  })

  it('cancel after the search finished does not kill anything', async () => {
    const { execApi, emit, resolveStart } = createControlledExecApi()
    const cancel = startRepoSearch(execApi, 'conn', '/repo', 'foo', vi.fn())
    await resolveStart()
    emit('exec-1', { type: ExecEventType.Exit, exitCode: 0 })
    cancel()
    expect(execApi.kill).not.toHaveBeenCalled()
  })

  it('cancel before a failed start reports nothing', async () => {
    const { execApi, resolveStart } = createControlledExecApi({ success: false, error: 'no connection' })
    const { events, onEvent } = collect()
    const cancel = startRepoSearch(execApi, 'conn', '/repo', 'foo', onEvent)
    cancel()
    await resolveStart()
    expect(events).toEqual([])
  })
})

describe('workspace path helpers', () => {
  it('joins a relative hit path onto the workspace', () => {
    expect(toWorkspaceAbsolute('/repo', 'src/a.ts')).toBe('/repo/src/a.ts')
  })

  it('strips the workspace prefix, leaving relative paths alone', () => {
    expect(toWorkspaceRelative('/repo', '/repo/src/a.ts')).toBe('src/a.ts')
    expect(toWorkspaceRelative('/repo', 'src/a.ts')).toBe('src/a.ts')
  })
})
