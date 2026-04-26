import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { createGitControllerStore } from './createGitControllerStore'
import type { GitControllerDeps } from './createGitControllerStore'
import type { Workspace } from '../types'
import { FileChangeStatus } from '../types'
import { makeWorkspace as makeWorkspaceBase } from '../../shared/test-fixtures/workspace'

function makeWorkspace(overrides: Partial<Workspace> = {}): Workspace {
  return makeWorkspaceBase({ isGitRepo: true, gitBranch: 'feat', gitRootPath: '/test', ...overrides })
}

function makeDeps(overrides: Partial<GitControllerDeps> = {}): GitControllerDeps {
  const ws = overrides.initialWorkspace ?? makeWorkspace()
  return {
    git: {
      hasUncommittedChanges: vi.fn().mockResolvedValue(false),
      getDiff: vi.fn().mockResolvedValue({ success: true, diff: { files: [] } }),
      checkMergeConflicts: vi.fn().mockResolvedValue({ success: true, conflicts: { hasConflicts: false } }),
      fetch: vi.fn().mockResolvedValue({ success: true }),
      getBehindCount: vi.fn().mockResolvedValue(0),
      pull: vi.fn().mockResolvedValue({ success: true }),
    } as unknown as GitControllerDeps['git'],
    github: {
      getPrInfo: vi.fn().mockResolvedValue({ noPr: true, createUrl: 'https://github.com/test/repo/compare/main...feat?expand=1' }),
    },
    lookupWorkspace: vi.fn().mockReturnValue(undefined),
    refreshGitInfo: vi.fn().mockResolvedValue(undefined),
    refreshWorkspaceGitInfo: vi.fn().mockResolvedValue(undefined),
    getWorkspace: vi.fn().mockReturnValue(ws),
    initialWorkspace: ws,
    ...overrides,
  }
}

async function flushRefresh(store: ReturnType<typeof createGitControllerStore>): Promise<void> {
  const promise = store.getState().refreshDiffStatus()
  vi.advanceTimersByTime(300)
  await promise
}

describe('createGitControllerStore', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    vi.clearAllMocks()
  })
  afterEach(() => {
    vi.useRealTimers()
  })

  describe('refreshDiffStatus', () => {
    it('sets gitRefreshing true then false', async () => {
      const deps = makeDeps()
      const store = createGitControllerStore(deps)

      const promise = store.getState().refreshDiffStatus()
      vi.advanceTimersByTime(300)
      expect(store.getState().gitRefreshing).toBe(true)
      await promise
      expect(store.getState().gitRefreshing).toBe(false)
    })

    it('sets hasUncommittedChanges from git API', async () => {
      const deps = makeDeps()
      vi.mocked(deps.git.hasUncommittedChanges).mockResolvedValue(true)
      const store = createGitControllerStore(deps)

      await flushRefresh(store)
      expect(store.getState().hasUncommittedChanges).toBe(true)
    })

    it('catches hasUncommittedChanges error gracefully', async () => {
      const deps = makeDeps()
      vi.mocked(deps.git.hasUncommittedChanges).mockRejectedValue(new Error('gone'))
      const store = createGitControllerStore(deps)

      await flushRefresh(store)
      expect(store.getState().hasUncommittedChanges).toBe(false)
      expect(store.getState().gitRefreshing).toBe(false)
    })

    it('sets isDiffCleanFromParent true when diff is empty and no uncommitted', async () => {
      const parentWs = makeWorkspace({ id: 'parent', gitBranch: 'main' })
      const ws = makeWorkspace({ isWorktree: true, parentId: 'parent' })
      const deps = makeDeps({
        initialWorkspace: ws,
        getWorkspace: vi.fn().mockReturnValue(ws),
        lookupWorkspace: vi.fn().mockReturnValue(parentWs),
      })
      vi.mocked(deps.git.getDiff).mockResolvedValue({ success: true, diff: { files: [], totalAdditions: 0, totalDeletions: 0, baseBranch: 'main', headBranch: 'feat' } })
      const store = createGitControllerStore(deps)

      await flushRefresh(store)
      expect(store.getState().isDiffCleanFromParent).toBe(true)
    })

    it('sets isDiffCleanFromParent false when diff has files', async () => {
      const parentWs = makeWorkspace({ id: 'parent', gitBranch: 'main' })
      const ws = makeWorkspace({ isWorktree: true, parentId: 'parent' })
      const deps = makeDeps({
        initialWorkspace: ws,
        getWorkspace: vi.fn().mockReturnValue(ws),
        lookupWorkspace: vi.fn().mockReturnValue(parentWs),
      })
      vi.mocked(deps.git.getDiff).mockResolvedValue({ success: true, diff: { files: [{ path: 'a.ts', status: FileChangeStatus.Modified, additions: 1, deletions: 0 }], totalAdditions: 1, totalDeletions: 0, baseBranch: 'main', headBranch: 'feat' } })
      const store = createGitControllerStore(deps)

      await flushRefresh(store)
      expect(store.getState().isDiffCleanFromParent).toBe(false)
    })

    it('sets isDiffCleanFromParent false when uncommitted changes exist despite clean diff', async () => {
      const parentWs = makeWorkspace({ id: 'parent', gitBranch: 'main' })
      const ws = makeWorkspace({ isWorktree: true, parentId: 'parent' })
      const deps = makeDeps({
        initialWorkspace: ws,
        getWorkspace: vi.fn().mockReturnValue(ws),
        lookupWorkspace: vi.fn().mockReturnValue(parentWs),
      })
      vi.mocked(deps.git.hasUncommittedChanges).mockResolvedValue(true)
      vi.mocked(deps.git.getDiff).mockResolvedValue({ success: true, diff: { files: [], totalAdditions: 0, totalDeletions: 0, baseBranch: 'main', headBranch: 'feat' } })
      const store = createGitControllerStore(deps)

      await flushRefresh(store)
      expect(store.getState().isDiffCleanFromParent).toBe(false)
    })

    it('refreshes parent git info before reading parent branch', async () => {
      const parentWs = makeWorkspace({ id: 'parent', gitBranch: 'main' })
      const ws = makeWorkspace({ isWorktree: true, parentId: 'parent' })
      const deps = makeDeps({
        initialWorkspace: ws,
        getWorkspace: vi.fn().mockReturnValue(ws),
        lookupWorkspace: vi.fn().mockReturnValue(parentWs),
      })
      const store = createGitControllerStore(deps)

      await flushRefresh(store)
      expect(deps.refreshWorkspaceGitInfo).toHaveBeenCalledWith('parent')
    })

    it('skips diff check for non-worktree workspace', async () => {
      const deps = makeDeps()
      const store = createGitControllerStore(deps)

      await flushRefresh(store)
      expect(deps.git.getDiff).not.toHaveBeenCalled()
    })

    it('skips diff check when parent has no gitBranch', async () => {
      const parentWs = makeWorkspace({ id: 'parent', gitBranch: undefined })
      const ws = makeWorkspace({ isWorktree: true, parentId: 'parent' })
      const deps = makeDeps({
        initialWorkspace: ws,
        getWorkspace: vi.fn().mockReturnValue(ws),
        lookupWorkspace: vi.fn().mockReturnValue(parentWs),
      })
      const store = createGitControllerStore(deps)

      await flushRefresh(store)
      expect(deps.git.getDiff).not.toHaveBeenCalled()
    })

    it('sets isDiffCleanFromParent false when getDiff returns success: false', async () => {
      const parentWs = makeWorkspace({ id: 'parent', gitBranch: 'main' })
      const ws = makeWorkspace({ isWorktree: true, parentId: 'parent' })
      const deps = makeDeps({
        initialWorkspace: ws,
        getWorkspace: vi.fn().mockReturnValue(ws),
        lookupWorkspace: vi.fn().mockReturnValue(parentWs),
      })
      vi.mocked(deps.git.getDiff).mockResolvedValue({ success: false, error: 'diff failed' })
      const store = createGitControllerStore(deps)

      await flushRefresh(store)
      expect(store.getState().isDiffCleanFromParent).toBe(false)
    })

    it('sets hasConflictsWithParent true when conflicts detected', async () => {
      const parentWs = makeWorkspace({ id: 'parent', gitBranch: 'main' })
      const ws = makeWorkspace({ isWorktree: true, parentId: 'parent', gitBranch: 'feat' })
      const deps = makeDeps({
        initialWorkspace: ws,
        getWorkspace: vi.fn().mockReturnValue(ws),
        lookupWorkspace: vi.fn().mockReturnValue(parentWs),
      })
      vi.mocked(deps.git.checkMergeConflicts).mockResolvedValue({ success: true, conflicts: { hasConflicts: true, conflictedFiles: ['a.ts'], messages: ['conflict'] } })
      const store = createGitControllerStore(deps)

      await flushRefresh(store)
      expect(store.getState().hasConflictsWithParent).toBe(true)
    })

    it('sets hasConflictsWithParent false when checkMergeConflicts fails', async () => {
      const parentWs = makeWorkspace({ id: 'parent', gitBranch: 'main' })
      const ws = makeWorkspace({ isWorktree: true, parentId: 'parent', gitBranch: 'feat' })
      const deps = makeDeps({
        initialWorkspace: ws,
        getWorkspace: vi.fn().mockReturnValue(ws),
        lookupWorkspace: vi.fn().mockReturnValue(parentWs),
      })
      vi.mocked(deps.git.checkMergeConflicts).mockResolvedValue({ success: false, error: 'check failed' })
      const store = createGitControllerStore(deps)

      await flushRefresh(store)
      expect(store.getState().hasConflictsWithParent).toBe(false)
    })

    it('skips merge conflict check when workspace has no gitBranch', async () => {
      const parentWs = makeWorkspace({ id: 'parent', gitBranch: 'main' })
      const ws = makeWorkspace({ isWorktree: true, parentId: 'parent', gitBranch: undefined })
      const deps = makeDeps({
        initialWorkspace: ws,
        getWorkspace: vi.fn().mockReturnValue(ws),
        lookupWorkspace: vi.fn().mockReturnValue(parentWs),
      })
      const store = createGitControllerStore(deps)

      await flushRefresh(store)
      expect(deps.git.checkMergeConflicts).not.toHaveBeenCalled()
    })
  })

  describe('debounce', () => {
    it('rapid calls within debounce window collapse to one daemon call', async () => {
      const deps = makeDeps()
      const store = createGitControllerStore(deps)

      const p1 = store.getState().refreshDiffStatus()
      const p2 = store.getState().refreshDiffStatus()
      const p3 = store.getState().refreshDiffStatus()
      vi.advanceTimersByTime(300)
      await Promise.all([p1, p2, p3])
      expect(deps.git.hasUncommittedChanges).toHaveBeenCalledTimes(1)

      store.getState().dispose()
    })

    it('calls separated by more than 300ms each fire once', async () => {
      const deps = makeDeps()
      const store = createGitControllerStore(deps)

      const p1 = store.getState().refreshDiffStatus()
      vi.advanceTimersByTime(300)
      await p1
      expect(deps.git.hasUncommittedChanges).toHaveBeenCalledTimes(1)

      const p2 = store.getState().refreshDiffStatus()
      vi.advanceTimersByTime(300)
      await p2
      expect(deps.git.hasUncommittedChanges).toHaveBeenCalledTimes(2)

      store.getState().dispose()
    })

    it('dispose cancels a pending debounced call', () => {
      const deps = makeDeps()
      const store = createGitControllerStore(deps)

      void store.getState().refreshDiffStatus()
      store.getState().dispose()

      vi.advanceTimersByTime(300)
      expect(deps.git.hasUncommittedChanges).not.toHaveBeenCalled()
    })
  })

  describe('refreshRemoteStatus', () => {
    it('calls fetch then getBehindCount and sets behindCount', async () => {
      const deps = makeDeps()
      vi.mocked(deps.git.getBehindCount).mockResolvedValue(3)
      const store = createGitControllerStore(deps)
      store.getState().dispose()

      await store.getState().refreshRemoteStatus()
      expect(deps.git.fetch).toHaveBeenCalledWith('/test')
      expect(deps.git.getBehindCount).toHaveBeenCalledWith('/test')
      expect(store.getState().behindCount).toBe(3)
    })

    it('does nothing for non-git repo', async () => {
      const ws = makeWorkspace({ isGitRepo: false })
      const deps = makeDeps({ initialWorkspace: ws, getWorkspace: vi.fn().mockReturnValue(ws) })
      const store = createGitControllerStore(deps)
      store.getState().dispose()

      await store.getState().refreshRemoteStatus()
      expect(deps.git.fetch).not.toHaveBeenCalled()
    })

    it('catches fetch errors gracefully', async () => {
      const deps = makeDeps()
      vi.mocked(deps.git.fetch).mockRejectedValue(new Error('network'))
      const store = createGitControllerStore(deps)
      store.getState().dispose()

      await store.getState().refreshRemoteStatus()
      expect(store.getState().behindCount).toBe(0)
    })
  })

  describe('pullFromRemote', () => {
    it('success: resets behindCount, calls refreshGitInfo', async () => {
      const deps = makeDeps()
      vi.mocked(deps.git.pull).mockResolvedValue({ success: true })
      const store = createGitControllerStore(deps)
      store.getState().dispose()

      const result = await store.getState().pullFromRemote()
      expect(result).toEqual({ success: true })
      expect(store.getState().behindCount).toBe(0)
      expect(store.getState().pullLoading).toBe(false)
      expect(deps.refreshGitInfo).toHaveBeenCalled()
    })

    it('returns error for non-git repo', async () => {
      const ws = makeWorkspace({ isGitRepo: false })
      const deps = makeDeps({ initialWorkspace: ws, getWorkspace: vi.fn().mockReturnValue(ws) })
      const store = createGitControllerStore(deps)
      store.getState().dispose()

      const result = await store.getState().pullFromRemote()
      expect(result).toEqual({ success: false, error: 'Not a git repo' })
    })

    it('returns pull failure result without resetting behindCount', async () => {
      const deps = makeDeps()
      vi.mocked(deps.git.pull).mockResolvedValue({ success: false, error: 'conflicts' })
      const store = createGitControllerStore(deps)
      store.getState().dispose()
      store.setState({ behindCount: 5 })

      const result = await store.getState().pullFromRemote()
      expect(result).toEqual({ success: false, error: 'conflicts' })
      expect(store.getState().behindCount).toBe(5)
    })

    it('catches thrown Error', async () => {
      const deps = makeDeps()
      vi.mocked(deps.git.pull).mockRejectedValue(new Error('fatal'))
      const store = createGitControllerStore(deps)
      store.getState().dispose()

      const result = await store.getState().pullFromRemote()
      expect(result).toEqual({ success: false, error: 'fatal' })
      expect(store.getState().pullLoading).toBe(false)
    })

    it('catches thrown non-Error', async () => {
      const deps = makeDeps()
      vi.mocked(deps.git.pull).mockRejectedValue('something')
      const store = createGitControllerStore(deps)
      store.getState().dispose()

      const result = await store.getState().pullFromRemote()
      expect(result).toEqual({ success: false, error: 'Unknown error' })
    })
  })

  describe('refreshPrStatus', () => {
    it('sets prInfo when PR exists', async () => {
      const ws = makeWorkspace({ isWorktree: true, parentId: 'parent', gitBranch: 'feat', gitRootPath: '/root' })
      const parentWs = makeWorkspace({ id: 'parent', gitBranch: 'main' })
      const deps = makeDeps({
        initialWorkspace: ws,
        getWorkspace: vi.fn().mockReturnValue(ws),
        lookupWorkspace: vi.fn().mockReturnValue(parentWs),
      })
      const prInfo = { url: 'https://github.com/pr/1', title: 'PR', number: 1, state: 'OPEN' as const, reviews: [], checkRuns: [], unresolvedThreads: [], unresolvedCount: 0 }
      vi.mocked(deps.github.getPrInfo).mockResolvedValue({ prInfo })
      const store = createGitControllerStore(deps)
      store.getState().dispose()

      await store.getState().refreshPrStatus()
      expect(store.getState().prInfo).toEqual(prInfo)
    })

    it('sets prInfo to null when noPr', async () => {
      const ws = makeWorkspace({ isWorktree: true, parentId: 'parent', gitBranch: 'feat', gitRootPath: '/root' })
      const parentWs = makeWorkspace({ id: 'parent', gitBranch: 'main' })
      const deps = makeDeps({
        initialWorkspace: ws,
        getWorkspace: vi.fn().mockReturnValue(ws),
        lookupWorkspace: vi.fn().mockReturnValue(parentWs),
      })
      vi.mocked(deps.github.getPrInfo).mockResolvedValue({ noPr: true, createUrl: 'url' })
      const store = createGitControllerStore(deps)
      store.getState().dispose()

      store.setState({ prInfo: { url: 'old' } as unknown as import('../types').GitHubPrInfo })
      await store.getState().refreshPrStatus()
      expect(store.getState().prInfo).toBeNull()
    })

    it('early returns for non-worktree', async () => {
      const ws = makeWorkspace({ isWorktree: false })
      const deps = makeDeps({ initialWorkspace: ws, getWorkspace: vi.fn().mockReturnValue(ws) })
      const store = createGitControllerStore(deps)
      store.getState().dispose()

      await store.getState().refreshPrStatus()
      expect(deps.github.getPrInfo).not.toHaveBeenCalled()
    })

    it('early returns when missing gitBranch', async () => {
      const ws = makeWorkspace({ isWorktree: true, parentId: 'parent',  gitRootPath: '/root' })
      const deps = makeDeps({ initialWorkspace: ws, getWorkspace: vi.fn().mockReturnValue(ws) })
      const store = createGitControllerStore(deps)
      store.getState().dispose()

      await store.getState().refreshPrStatus()
      expect(deps.github.getPrInfo).not.toHaveBeenCalled()
    })

    it('early returns when parent has no gitBranch', async () => {
      const ws = makeWorkspace({ isWorktree: true, parentId: 'parent', gitBranch: 'feat', gitRootPath: '/root' })
      const parentWs = makeWorkspace({ id: 'parent', gitBranch: undefined })
      const deps = makeDeps({
        initialWorkspace: ws,
        getWorkspace: vi.fn().mockReturnValue(ws),
        lookupWorkspace: vi.fn().mockReturnValue(parentWs),
      })
      const store = createGitControllerStore(deps)
      store.getState().dispose()

      await store.getState().refreshPrStatus()
      expect(deps.github.getPrInfo).not.toHaveBeenCalled()
    })

    it('catches errors gracefully', async () => {
      const ws = makeWorkspace({ isWorktree: true, parentId: 'parent', gitBranch: 'feat', gitRootPath: '/root' })
      const parentWs = makeWorkspace({ id: 'parent', gitBranch: 'main' })
      const deps = makeDeps({
        initialWorkspace: ws,
        getWorkspace: vi.fn().mockReturnValue(ws),
        lookupWorkspace: vi.fn().mockReturnValue(parentWs),
      })
      vi.mocked(deps.github.getPrInfo).mockRejectedValue(new Error('auth'))
      const store = createGitControllerStore(deps)
      store.getState().dispose()

      await store.getState().refreshPrStatus()
      expect(store.getState().prInfo).toBeNull()
    })
  })

  describe('openGitHub', () => {
    it('returns error when missing parentId', async () => {
      const ws = makeWorkspace({ parentId: undefined })
      const deps = makeDeps({ initialWorkspace: ws, getWorkspace: vi.fn().mockReturnValue(ws) })
      const store = createGitControllerStore(deps)
      store.getState().dispose()

      const result = await store.getState().openGitHub()
      expect(result).toEqual({ error: 'Missing workspace info' })
    })

    it('returns error when parent not found', async () => {
      const ws = makeWorkspace({ parentId: 'parent', gitBranch: 'feat', gitRootPath: '/root' })
      const deps = makeDeps({
        initialWorkspace: ws,
        getWorkspace: vi.fn().mockReturnValue(ws),
        lookupWorkspace: vi.fn().mockReturnValue(undefined),
      })
      const store = createGitControllerStore(deps)
      store.getState().dispose()

      const result = await store.getState().openGitHub()
      expect(result).toEqual({ error: 'Parent branch not found' })
    })

    it('returns url with hasPr: true when PR exists', async () => {
      const ws = makeWorkspace({ parentId: 'parent', gitBranch: 'feat', gitRootPath: '/root' })
      const parentWs = makeWorkspace({ id: 'parent', gitBranch: 'main' })
      const deps = makeDeps({
        initialWorkspace: ws,
        getWorkspace: vi.fn().mockReturnValue(ws),
        lookupWorkspace: vi.fn().mockReturnValue(parentWs),
      })
      const prInfo = { url: 'https://github.com/pr/1', title: 'PR', number: 1, state: 'OPEN' as const, reviews: [], checkRuns: [], unresolvedThreads: [], unresolvedCount: 0 }
      vi.mocked(deps.github.getPrInfo).mockResolvedValue({ prInfo })
      const store = createGitControllerStore(deps)
      store.getState().dispose()

      const result = await store.getState().openGitHub()
      expect(result).toEqual({ url: 'https://github.com/pr/1', hasPr: true })
      expect(store.getState().prInfo).toEqual(prInfo)
    })

    it('returns createUrl with hasPr: false when no PR', async () => {
      const ws = makeWorkspace({ parentId: 'parent', gitBranch: 'feat', gitRootPath: '/root' })
      const parentWs = makeWorkspace({ id: 'parent', gitBranch: 'main' })
      const deps = makeDeps({
        initialWorkspace: ws,
        getWorkspace: vi.fn().mockReturnValue(ws),
        lookupWorkspace: vi.fn().mockReturnValue(parentWs),
      })
      vi.mocked(deps.github.getPrInfo).mockResolvedValue({ noPr: true, createUrl: 'https://github.com/create' })
      const store = createGitControllerStore(deps)
      store.getState().dispose()

      const result = await store.getState().openGitHub()
      expect(result).toEqual({ url: 'https://github.com/create', hasPr: false })
      expect(store.getState().prInfo).toBeNull()
    })
  })

  describe('startPolling / dispose', () => {
    it('does not start polling for non-git repo', () => {
      const ws = makeWorkspace({ isGitRepo: false })
      const deps = makeDeps({ initialWorkspace: ws, getWorkspace: vi.fn().mockReturnValue(ws) })
      const store = createGitControllerStore(deps)

      store.getState().startPolling()
      expect(deps.git.hasUncommittedChanges).not.toHaveBeenCalled()
    })

    it('calls refreshDiffStatus immediately on startPolling without debounce', () => {
      const deps = makeDeps()
      const store = createGitControllerStore(deps)

      store.getState().startPolling()
      expect(deps.git.hasUncommittedChanges).toHaveBeenCalled()
      store.getState().dispose()
    })

    it('dispose cancels a pending debounced refreshDiffStatus', () => {
      const deps = makeDeps()
      const store = createGitControllerStore(deps)
      store.getState().startPolling()
      vi.mocked(deps.git.hasUncommittedChanges).mockClear()

      void store.getState().refreshDiffStatus()
      store.getState().dispose()

      vi.advanceTimersByTime(300)
      expect(deps.git.hasUncommittedChanges).not.toHaveBeenCalled()
    })

    it('startPolling calls refreshRemoteStatus for git repos', () => {
      const deps = makeDeps()
      const store = createGitControllerStore(deps)

      store.getState().startPolling()
      expect(deps.git.fetch).toHaveBeenCalled()
      store.getState().dispose()
    })

    it('startPolling calls refreshPrStatus for worktree with parent', () => {
      const ws = makeWorkspace({ isWorktree: true, parentId: 'parent', gitBranch: 'feat', gitRootPath: '/root' })
      const parentWs = makeWorkspace({ id: 'parent', gitBranch: 'main' })
      const deps = makeDeps({
        initialWorkspace: ws,
        getWorkspace: vi.fn().mockReturnValue(ws),
        lookupWorkspace: vi.fn().mockReturnValue(parentWs),
      })
      const store = createGitControllerStore(deps)

      store.getState().startPolling()
      expect(deps.github.getPrInfo).toHaveBeenCalled()
      store.getState().dispose()
    })
  })

  describe('triggerRefresh', () => {
    it('calls refreshDiffStatus after debounce window', () => {
      const deps = makeDeps()
      const store = createGitControllerStore(deps)
      store.getState().dispose()

      store.getState().triggerRefresh()
      expect(deps.git.hasUncommittedChanges).not.toHaveBeenCalled()
      vi.advanceTimersByTime(300)
      expect(deps.git.hasUncommittedChanges).toHaveBeenCalled()
    })
  })
})
