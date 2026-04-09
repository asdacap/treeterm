import { describe, it, expect, vi, beforeEach } from 'vitest'
import { parseGitHubOwnerRepo, createGitHubApi } from './githubClient'
import type { ExecApi, SettingsApi } from '../types'
import type { ExecEvent } from '../../shared/ipc-types'
import type { Settings } from '../../shared/types'

// ---------------------------------------------------------------------------
// parseGitHubOwnerRepo
// ---------------------------------------------------------------------------

describe('parseGitHubOwnerRepo', () => {
  it('parses SSH URL', () => {
    expect(parseGitHubOwnerRepo('git@github.com:owner/repo.git')).toEqual({ owner: 'owner', repo: 'repo' })
  })

  it('parses SSH URL without .git', () => {
    expect(parseGitHubOwnerRepo('git@github.com:owner/repo')).toEqual({ owner: 'owner', repo: 'repo' })
  })

  it('parses HTTPS URL', () => {
    expect(parseGitHubOwnerRepo('https://github.com/owner/repo.git')).toEqual({ owner: 'owner', repo: 'repo' })
  })

  it('parses HTTPS URL without .git', () => {
    expect(parseGitHubOwnerRepo('https://github.com/owner/repo')).toEqual({ owner: 'owner', repo: 'repo' })
  })

  it('returns null for non-GitHub URL', () => {
    expect(parseGitHubOwnerRepo('https://gitlab.com/owner/repo.git')).toBeNull()
  })

  it('returns null for invalid URL', () => {
    expect(parseGitHubOwnerRepo('not-a-url')).toBeNull()
  })
})

// ---------------------------------------------------------------------------
// createGitHubApi
// ---------------------------------------------------------------------------

interface MockExecApi extends ExecApi {
  _complete: (execId: string, stdout: string, exitCode?: number) => void
}

function createMockExec(): MockExecApi {
  const eventCallbacks = new Map<string, (event: ExecEvent) => void>()
  let execCounter = 0

  return {
    start: vi.fn().mockImplementation(() => {
      execCounter++
      return Promise.resolve({ success: true, execId: `exec-${String(execCounter)}` })
    }),
    kill: vi.fn(),
    onEvent: vi.fn().mockImplementation((execId: string, cb: (event: ExecEvent) => void) => {
      eventCallbacks.set(execId, cb)
      return () => { eventCallbacks.delete(execId) }
    }),
    _complete: (execId: string, stdout: string, exitCode = 0) => {
      const cb = eventCallbacks.get(execId)
      if (cb) {
        if (stdout) cb({ type: 'stdout', data: stdout })
        cb({ type: 'exit', exitCode })
      }
    },
  }
}

function createMockSettings(overrides?: Partial<Settings['github']>): SettingsApi {
  return {
    load: vi.fn().mockResolvedValue({
      github: { autodetectViaGh: true, pat: '', ...overrides },
    } as unknown as Settings),
    save: vi.fn().mockResolvedValue({ success: true }),
    onOpen: vi.fn().mockReturnValue(() => {}),
  }
}

describe('createGitHubApi', () => {
  let exec: MockExecApi
  let settings: SettingsApi

  beforeEach(() => {
    exec = createMockExec()
    settings = createMockSettings()
    vi.restoreAllMocks()
  })

  it('returns error when gh auth token fails', async () => {
    const api = createGitHubApi(exec, settings, 'local')
    const promise = api.getPrInfo('/repo', 'feature', 'main')

    // Wait for exec.start to be called, then complete with error
    await vi.waitFor(() => { expect(exec.start).toHaveBeenCalledTimes(1) })
    exec._complete('exec-1', '', 1)

    const result = await promise
    expect(result).toEqual({ error: 'Failed to get token from gh CLI. Is gh installed and authenticated?' })
  })

  it('returns error when no PAT configured and autodetect disabled', async () => {
    settings = createMockSettings({ autodetectViaGh: false, pat: '' })
    const api = createGitHubApi(exec, settings, 'local')
    const result = await api.getPrInfo('/repo', 'feature', 'main')
    expect(result).toEqual({ error: 'No GitHub PAT configured. Set one in Settings > GitHub.' })
  })

  it('returns error when git remote fails', async () => {
    const api = createGitHubApi(exec, settings, 'local')
    const promise = api.getPrInfo('/repo', 'feature', 'main')

    // First call: gh auth token succeeds
    await vi.waitFor(() => { expect(exec.start).toHaveBeenCalledTimes(1) })
    exec._complete('exec-1', 'ghp_token123\n')

    // Second call: git remote fails
    await vi.waitFor(() => { expect(exec.start).toHaveBeenCalledTimes(2) })
    exec._complete('exec-2', '', 1)

    const result = await promise
    expect(result).toHaveProperty('error')
    expect((result as { error: string }).error).toContain('Failed to get remote URL')
  })

  it('returns noPr result when no open PRs found', async () => {
    const api = createGitHubApi(exec, settings, 'local')
    const promise = api.getPrInfo('/repo', 'feature', 'main')

    // gh auth token
    await vi.waitFor(() => { expect(exec.start).toHaveBeenCalledTimes(1) })
    exec._complete('exec-1', 'ghp_token123\n')

    // git remote
    await vi.waitFor(() => { expect(exec.start).toHaveBeenCalledTimes(2) })
    exec._complete('exec-2', 'git@github.com:owner/repo.git\n')

    // Mock fetch: no PRs
    vi.spyOn(globalThis, 'fetch').mockResolvedValueOnce(new Response(JSON.stringify([]), { status: 200 }))

    const result = await promise
    expect(result).toEqual({ noPr: true, createUrl: 'https://github.com/owner/repo/compare/main...feature?expand=1' })
  })
})
