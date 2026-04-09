import { describe, it, expect, vi, beforeEach } from 'vitest'
import { parseGitHubOwnerRepo, createGitHubApi } from './githubClient'
import type { ExecApi, SettingsApi, GitHubPrInfoResult } from '../types'
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

  it('returns full prInfo on happy path with GraphQL', async () => {
    const api = createGitHubApi(exec, settings, 'local')
    const promise = api.getPrInfo('/repo', 'feature', 'main')

    await vi.waitFor(() => { expect(exec.start).toHaveBeenCalledTimes(1) })
    exec._complete('exec-1', 'ghp_token123\n')
    await vi.waitFor(() => { expect(exec.start).toHaveBeenCalledTimes(2) })
    exec._complete('exec-2', 'git@github.com:owner/repo.git\n')

    const graphqlData = {
      data: {
        repository: {
          pullRequest: {
            state: 'OPEN',
            reviewThreads: {
              nodes: [
                { isResolved: false, comments: { nodes: [{ body: 'fix this', path: 'src/a.ts', line: 10, author: { login: 'reviewer' } }] } },
                { isResolved: true, comments: { nodes: [{ body: 'resolved', path: 'src/b.ts', line: 5, author: { login: 'other' } }] } },
              ]
            },
            latestReviews: { nodes: [{ author: { login: 'reviewer' }, state: 'CHANGES_REQUESTED' }] },
            commits: { nodes: [{ commit: { statusCheckRollup: { contexts: { nodes: [
              { __typename: 'CheckRun', name: 'CI', status: 'COMPLETED', conclusion: 'SUCCESS' },
              { __typename: 'StatusContext', context: 'deploy', state: 'success' },
            ] } } } }] }
          }
        }
      }
    }

    vi.spyOn(globalThis, 'fetch')
      .mockResolvedValueOnce(new Response(JSON.stringify([{ number: 42, title: 'My PR' }]), { status: 200 }))
      .mockResolvedValueOnce(new Response(JSON.stringify(graphqlData), { status: 200 }))

    const result = await promise
    expect(result).toHaveProperty('prInfo')
    const { prInfo } = result as Extract<GitHubPrInfoResult, { prInfo: unknown }>
    expect(prInfo.number).toBe(42)
    expect(prInfo.state).toBe('OPEN')
    expect(prInfo.reviews).toHaveLength(1)
    expect(prInfo.checkRuns).toHaveLength(1)
    expect(prInfo.checkRuns[0].name).toBe('CI')
    expect(prInfo.unresolvedThreads).toHaveLength(1)
    expect(prInfo.unresolvedCount).toBe(1)
  })

  it('returns basic prInfo when GraphQL returns non-ok', async () => {
    const api = createGitHubApi(exec, settings, 'local')
    const promise = api.getPrInfo('/repo', 'feature', 'main')

    await vi.waitFor(() => { expect(exec.start).toHaveBeenCalledTimes(1) })
    exec._complete('exec-1', 'ghp_token123\n')
    await vi.waitFor(() => { expect(exec.start).toHaveBeenCalledTimes(2) })
    exec._complete('exec-2', 'git@github.com:owner/repo.git\n')

    vi.spyOn(globalThis, 'fetch')
      .mockResolvedValueOnce(new Response(JSON.stringify([{ number: 1, title: 'PR' }]), { status: 200 }))
      .mockResolvedValueOnce(new Response('error', { status: 500 }))

    const result = await promise
    expect(result).toHaveProperty('prInfo')
    const { prInfo } = result as Extract<GitHubPrInfoResult, { prInfo: unknown }>
    expect(prInfo.number).toBe(1)
    expect(prInfo.reviews).toEqual([])
    expect(prInfo.checkRuns).toEqual([])
  })

  it('returns basic prInfo when GraphQL fetch throws', async () => {
    const api = createGitHubApi(exec, settings, 'local')
    const promise = api.getPrInfo('/repo', 'feature', 'main')

    await vi.waitFor(() => { expect(exec.start).toHaveBeenCalledTimes(1) })
    exec._complete('exec-1', 'ghp_token123\n')
    await vi.waitFor(() => { expect(exec.start).toHaveBeenCalledTimes(2) })
    exec._complete('exec-2', 'git@github.com:owner/repo.git\n')

    vi.spyOn(globalThis, 'fetch')
      .mockResolvedValueOnce(new Response(JSON.stringify([{ number: 1, title: 'PR' }]), { status: 200 }))
      .mockRejectedValueOnce(new Error('network error'))

    const result = await promise
    expect(result).toHaveProperty('prInfo')
    const { prInfo } = result as Extract<GitHubPrInfoResult, { prInfo: unknown }>
    expect(prInfo.number).toBe(1)
    expect(prInfo.unresolvedCount).toBe(0)
  })

  it('returns error when REST API returns non-ok', async () => {
    const api = createGitHubApi(exec, settings, 'local')
    const promise = api.getPrInfo('/repo', 'feature', 'main')

    await vi.waitFor(() => { expect(exec.start).toHaveBeenCalledTimes(1) })
    exec._complete('exec-1', 'ghp_token123\n')
    await vi.waitFor(() => { expect(exec.start).toHaveBeenCalledTimes(2) })
    exec._complete('exec-2', 'git@github.com:owner/repo.git\n')

    vi.spyOn(globalThis, 'fetch').mockResolvedValueOnce(new Response('', { status: 403, statusText: 'Forbidden' }))

    const result = await promise
    expect(result).toEqual({ error: 'GitHub API error: 403 Forbidden' })
  })

  it('uses PAT when autodetect is disabled', async () => {
    settings = createMockSettings({ autodetectViaGh: false, pat: 'ghp_pat123' })
    const api = createGitHubApi(exec, settings, 'local')
    const promise = api.getPrInfo('/repo', 'feature', 'main')

    // Only git remote should be called (no gh auth token)
    await vi.waitFor(() => { expect(exec.start).toHaveBeenCalledTimes(1) })
    exec._complete('exec-1', 'git@github.com:owner/repo.git\n')

    vi.spyOn(globalThis, 'fetch').mockResolvedValueOnce(new Response(JSON.stringify([]), { status: 200 }))

    const result = await promise
    expect(result).toHaveProperty('noPr')
    // Verify gh auth token was NOT called
    expect(exec.start).toHaveBeenCalledTimes(1)
    expect((exec.start as ReturnType<typeof vi.fn>).mock.calls[0]![3]).toEqual(['remote', 'get-url', 'origin'])
  })

  it('returns error when remote URL is not from GitHub', async () => {
    const api = createGitHubApi(exec, settings, 'local')
    const promise = api.getPrInfo('/repo', 'feature', 'main')

    await vi.waitFor(() => { expect(exec.start).toHaveBeenCalledTimes(1) })
    exec._complete('exec-1', 'ghp_token123\n')
    await vi.waitFor(() => { expect(exec.start).toHaveBeenCalledTimes(2) })
    exec._complete('exec-2', 'https://gitlab.com/owner/repo.git\n')

    const result = await promise
    expect(result).toHaveProperty('error')
    expect((result as { error: string }).error).toContain('Could not parse GitHub owner/repo')
  })

  it('returns error when exec.start fails', async () => {
    ;(exec.start as ReturnType<typeof vi.fn>).mockResolvedValueOnce({ success: false, error: 'exec failed' })
    const api = createGitHubApi(exec, settings, 'local')
    const result = await api.getPrInfo('/repo', 'feature', 'main')
    expect(result).toHaveProperty('error')
    expect((result as { error: string }).error).toBe('exec failed')
  })
})
