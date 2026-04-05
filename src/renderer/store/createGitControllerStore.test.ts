import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { createGitControllerStore } from './createGitControllerStore'
import type { GitControllerDeps } from './createGitControllerStore'
import type { Workspace } from '../types'

function makeWorkspace(overrides: Partial<Workspace> = {}): Workspace {
  return {
    id: 'ws-1',
    name: 'test',
    path: '/test',
    parentId: null,
    status: 'active',
    isGitRepo: true,
    gitBranch: 'feat',
    gitRootPath: '/test',
    isWorktree: false,
    isDetached: false,
    appStates: {},
    activeTabId: null,
    settings: { defaultApplicationId: '' },
    metadata: {},
    createdAt: Date.now(),
    lastActivity: Date.now(),
    ...overrides,
  }
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
    } as any,
    github: {
      getPrInfo: vi.fn().mockResolvedValue({ noPr: true, createUrl: 'https://github.com/test/repo/compare/main...feat?expand=1' }),
    },
    lookupWorkspace: vi.fn().mockReturnValue(undefined),
    refreshGitInfo: vi.fn().mockResolvedValue(undefined),
    getWorkspace: vi.fn().mockReturnValue(ws),
    initialWorkspace: ws,
    ...overrides,
  }
}

const flushPromises = () => new Promise((r) => setTimeout(r, 0))

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
      store.getState().dispose()

      const promise = store.getState().refreshDiffStatus()
      expect(store.getState().gitRefreshing).toBe(true)
      await promise
      expect(store.getState().gitRefreshing).toBe(false)
    })

    it('sets hasUncommittedChanges from git API', async () => {
      const deps = makeDeps()
      vi.mocked(deps.git.hasUncommittedChanges).mockResolvedValue(true)
      const store = createGitControllerStore(deps)
      store.getState().dispose()

      await store.getState().refreshDiffStatus()
      expect(store.getState().hasUncommittedChanges).toBe(true)
    })

    it('catches hasUncommittedChanges error gracefully', async () => {
      const deps = makeDeps()
      vi.mocked(deps.git.hasUncommittedChanges).mockRejectedValue(new Error('gone'))
      const store = createGitControllerStore(deps)
      store.getState().dispose()

      await store.getState().refreshDiffStatus()
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
      vi.mocked(deps.git.getDiff).mockResolvedValue({ success: true, diff: { files: [] } })
      const store = createGitControllerStore(deps)
      store.getState().dispose()

      await store.getState().refreshDiffStatus()
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
      vi.mocked(deps.git.getDiff).mockResolvedValue({ success: true, diff: { files: [{ path: 'a.ts' }] } })
      const store = createGitControllerStore(deps)
      store.getState().dispose()

      await store.getState().refreshDiffStatus()
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
      vi.mocked(deps.git.getDiff).mockResolvedValue({ success: true, diff: { files: [] } })
      const store = createGitControllerStore(deps)
      store.getState().dispose()

      await store.getState().refreshDiffStatus()
      expect(store.getState().isDiffCleanFromParent).toBe(false)
    })

    it('skips diff check for non-worktree workspace', async () => {
      const deps = makeDeps()
      const store = createGitControllerStore(deps)
      store.getState().dispose()

      await store.getState().refreshDiffStatus()
      expect(deps.git.getDiff).not.toHaveBeenCalled()
    })

    it('skips diff check when parent has no gitBranch', async () => {
      const parentWs = makeWorkspace({ id: 'parent', gitBranch: null })
      const ws = makeWorkspace({ isWorktree: true, parentId: 'parent' })
      const deps = makeDeps({
        initialWorkspace: ws,
        getWorkspace: vi.fn().mockReturnValue(ws),
        lookupWorkspace: vi.fn().mockReturnValue(parentWs),
      })
      const store = createGitControllerStore(deps)
      store.getState().dispose()

      await store.getState().refreshDiffStatus()
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
      vi.mocked(deps.git.getDiff).mockResolvedValue({ success: false })
      const store = createGitControllerStore(deps)
      store.getState().dispose()

      await store.getState().refreshDiffStatus()
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
      vi.mocked(deps.git.checkMergeConflicts).mockResolvedValue({ success: true, conflicts: { hasConflicts: true } })
      const store = createGitControllerStore(deps)
      store.getState().dispose()

      await store.getState().refreshDiffStatus()
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
      vi.mocked(deps.git.checkMergeConflicts).mockResolvedValue({ success: false })
      const store = createGitControllerStore(deps)
      store.getState().dispose()

      await store.getState().refreshDiffStatus()
      expect(store.getState().hasConflictsWithParent).toBe(false)
    })

    it('skips merge conflict check when workspace has no gitBranch', async () => {
      const parentWs = makeWorkspace({ id: 'parent', gitBranch: 'main' })
      const ws = makeWorkspace({ isWorktree: true, parentId: 'parent', gitBranch: null })
      const deps = makeDeps({
        initialWorkspace: ws,
        getWorkspace: vi.fn().mockReturnValue(ws),
        lookupWorkspace: vi.fn().mockReturnValue(parentWs),
      })
      const store = createGitControllerStore(deps)
      store.getState().dispose()

      await store.getState().refreshDiffStatus()
      expect(deps.git.checkMergeConflicts).not.toHaveBeenCalled()
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
      const prInfo = { url: 'https://github.com/pr/1', title: 'PR', number: 1 }
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

      store.setState({ prInfo: { url: 'old' } as any })
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
      const ws = makeWorkspace({ isWorktree: true, parentId: 'parent', gitBranch: null, gitRootPath: '/root' })
      const deps = makeDeps({ initialWorkspace: ws, getWorkspace: vi.fn().mockReturnValue(ws) })
      const store = createGitControllerStore(deps)
      store.getState().dispose()

      await store.getState().refreshPrStatus()
      expect(deps.github.getPrInfo).not.toHaveBeenCalled()
    })

    it('early returns when parent has no gitBranch', async () => {
      const ws = makeWorkspace({ isWorktree: true, parentId: 'parent', gitBranch: 'feat', gitRootPath: '/root' })
      const parentWs = makeWorkspace({ id: 'parent', gitBranch: null })
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
      const ws = makeWorkspace({ parentId: null })
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
      const prInfo = { url: 'https://github.com/pr/1', title: 'PR', number: 1 }
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

    it('calls refreshDiffStatus immediately for git repo', async () => {
      const deps = makeDeps()
      const store = createGitControllerStore(deps)

      store.getState().startPolling()
      expect(deps.git.hasUncommittedChanges).toHaveBeenCalled()
      store.getState().dispose()
    })

    it('polls refreshDiffStatus every 10 seconds', async () => {
      const deps = makeDeps()
      const store = createGitControllerStore(deps)

      store.getState().startPolling()
      // Initial call
      expect(deps.git.hasUncommittedChanges).toHaveBeenCalledTimes(1)

      vi.advanceTimersByTime(10_000)
      expect(deps.git.hasUncommittedChanges).toHaveBeenCalledTimes(2)

      vi.advanceTimersByTime(10_000)
      expect(deps.git.hasUncommittedChanges).toHaveBeenCalledTimes(3)

      store.getState().dispose()
    })

    it('dispose stops polling', async () => {
      const deps = makeDeps()
      const store = createGitControllerStore(deps)

      store.getState().startPolling()
      store.getState().dispose()

      vi.mocked(deps.git.hasUncommittedChanges).mockClear()
      vi.advanceTimersByTime(30_000)
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
})
