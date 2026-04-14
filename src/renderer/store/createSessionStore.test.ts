import { describe, it, expect, vi, beforeEach } from 'vitest'
import { createSessionStore, WorkspaceEntryStatus } from './createSessionStore'
import type { SessionDeps, SessionState } from './createSessionStore'
import type { LlmApi, Workspace, Application, GitInfo } from '../types'
import { ConnectionStatus } from '../../shared/types'
import type { StoreApi } from 'zustand'
import { createMockExecApi } from '../../shared/mockApis'

const flushPromises = () => new Promise(r => setTimeout(r, 0))

function makeDeps(overrides?: Partial<SessionDeps>): SessionDeps {
  return {
    git: {
      getInfo: vi.fn().mockResolvedValue({ isRepo: true, branch: 'main', rootPath: '/repo' } satisfies GitInfo),
      createWorktree: vi.fn().mockResolvedValue({ success: true, path: '/repo/.worktrees/test', branch: 'test' }),
      removeWorktree: vi.fn().mockResolvedValue({ success: true }),
      listWorktrees: vi.fn().mockResolvedValue([]),
      listLocalBranches: vi.fn().mockResolvedValue(['main']),
      listRemoteBranches: vi.fn().mockResolvedValue([]),
      getBranchesInWorktrees: vi.fn().mockResolvedValue([]),
      createWorktreeFromBranch: vi.fn().mockResolvedValue({ success: true, path: '/repo/.worktrees/feat', branch: 'feat' }),
      createWorktreeFromRemote: vi.fn().mockResolvedValue({ success: true, path: '/repo/.worktrees/remote', branch: 'remote-branch' }),
      getDiff: vi.fn().mockResolvedValue({ success: true }),
      getFileDiff: vi.fn().mockResolvedValue({ success: true }),
      checkMergeConflicts: vi.fn().mockResolvedValue({ hasConflicts: false }),
      merge: vi.fn().mockResolvedValue({ success: true }),
      hasUncommittedChanges: vi.fn().mockResolvedValue(false),
      commitAll: vi.fn().mockResolvedValue({ success: true }),
      deleteBranch: vi.fn().mockResolvedValue({ success: true }),
      renameBranch: vi.fn().mockResolvedValue({ success: true }),
      getUncommittedChanges: vi.fn().mockResolvedValue({ success: true }),
      getUncommittedFileDiff: vi.fn().mockResolvedValue({ success: true }),
      stageFile: vi.fn().mockResolvedValue({ success: true }),
      unstageFile: vi.fn().mockResolvedValue({ success: true }),
      stageAll: vi.fn().mockResolvedValue({ success: true }),
      unstageAll: vi.fn().mockResolvedValue({ success: true }),
      commitStaged: vi.fn().mockResolvedValue({ success: true }),
      getFileContentsForDiff: vi.fn().mockResolvedValue({ success: true }),
      getUncommittedFileContentsForDiff: vi.fn().mockResolvedValue({ success: true }),
      getHeadCommitHash: vi.fn().mockResolvedValue({ success: true, hash: 'abc123' }),
      getLog: vi.fn().mockResolvedValue({ success: true, result: { commits: [], hasMore: false } }),
      getCommitDiff: vi.fn().mockResolvedValue({ success: true, files: [] }),
      getCommitFileDiff: vi.fn().mockResolvedValue({ success: true, contents: null }),
      getRemoteUrl: vi.fn().mockResolvedValue({ url: 'https://github.com/test/repo.git' }),
      fetch: vi.fn(),
      pull: vi.fn(),
      getBehindCount: vi.fn(),
    },
    filesystem: {
      readDirectory: vi.fn().mockResolvedValue({ success: true }),
      readFile: vi.fn().mockResolvedValue({ success: true }),
      writeFile: vi.fn().mockResolvedValue({ success: true }),
      searchFiles: vi.fn().mockResolvedValue({ success: true }),
    },
    runActions: {
      detect: vi.fn().mockResolvedValue([]),
      run: vi.fn().mockResolvedValue(null),
    },
    exec: createMockExecApi(),
    sessionApi: {
      update: vi.fn().mockResolvedValue({ success: true }),
      lock: vi.fn().mockResolvedValue({ success: true, acquired: true, session: { id: 'session-1', workspaces: [], createdAt: 0, lastActivity: 0, version: 1, lock: null } }),
      unlock: vi.fn().mockResolvedValue({ success: true, session: { id: 'session-1', workspaces: [], createdAt: 0, lastActivity: 0, version: 1, lock: null } }),
      forceUnlock: vi.fn().mockResolvedValue({ success: true, session: { id: 'session-1', workspaces: [], createdAt: 0, lastActivity: 0, version: 1, lock: null } }),
      // Note: lock/unlock signatures no longer take holderId (daemon-generated identity via per-session gRPC connection)
      onSync: vi.fn().mockReturnValue(() => {}),
    },
    terminal: {
      create: vi.fn().mockResolvedValue({ success: true, sessionId: 'pty-1' }),
      attach: vi.fn().mockResolvedValue({ success: true }),
      list: vi.fn().mockResolvedValue([]),
      write: vi.fn(),
      resize: vi.fn(),
      kill: vi.fn(),
      onEvent: vi.fn().mockReturnValue(() => {}),
      onActiveProcessesOpen: vi.fn().mockReturnValue(() => {}),
      createSession: vi.fn().mockResolvedValue({ success: true, sessionId: 'pty-1' }),
    },
    getSettings: vi.fn().mockReturnValue({
      terminal: { fontSize: 14, fontFamily: 'monospace', cursorStyle: 'block', cursorBlink: true, showRawChars: false, instances: [] },
      sandbox: { enabledByDefault: false, allowNetworkByDefault: true },
      aiHarness: { instances: [] },
    }),
    appRegistry: {
      get: vi.fn().mockReturnValue(null),
      getDefaultApp: vi.fn().mockReturnValue(null),
    },
    llm: {
      analyzeTerminal: vi.fn().mockResolvedValue({ state: 'idle', reason: '' }),
      generateTitle: vi.fn().mockResolvedValue({ title: '', description: '', branchName: '' }),
    } as unknown as LlmApi,
    setActivityTabState: vi.fn(),
    github: {
      getPrInfo: vi.fn().mockResolvedValue({ noPr: true, createUrl: 'https://github.com/test/repo/compare/main...feat?expand=1' }),
    },
    ...overrides,
  }
}

function makeWorkspace(overrides: Partial<Workspace> = {}): Workspace {
  return {
    id: 'ws-1',
    name: 'test',
    path: '/test',
    parentId: null,
    status: 'active',
    isGitRepo: false,
    gitBranch: null,
    gitRootPath: null,
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

function makeFakeApp(overrides: Partial<Application> = {}): Application {
  return {
    id: 'terminal',
    name: 'Terminal',
    icon: 'terminal',
    createInitialState: () => ({ ptyId: null }),
    onWorkspaceLoad: () => ({ close: () => {}, dispose: () => {} }),
    canClose: true,
    showInNewTabMenu: true,
    displayStyle: 'block',
    isDefault: false,
    render: () => null,
    ...overrides,
  }
}

describe('createSessionStore', () => {
  let store: StoreApi<SessionState>
  let deps: SessionDeps

  beforeEach(() => {
    vi.clearAllMocks()
    deps = makeDeps()
    const localConn = { id: 'local', target: { type: 'local' as const }, status: ConnectionStatus.Connected as const }
    store = createSessionStore({ sessionId: 'session-1', connection: localConn }, deps)
  })

  describe('initial state', () => {
    it('has correct session id', () => {
      expect(store.getState().sessionId).toBe('session-1')
    })

    it('has empty workspace collection', () => {
      expect(store.getState().workspaces).toEqual(new Map())
    })

    it('has null active workspace', () => {
      expect(store.getState().activeWorkspaceId).toBeNull()
    })

    it('is not restoring', () => {
      expect(store.getState().isRestoring).toBe(false)
    })

    it('preserves connection from config', () => {
      const conn = { id: 'conn-1', target: { type: 'local' as const }, host: 'example.com', status: ConnectionStatus.Connected as const }
      const s = createSessionStore({ sessionId: 's', connection: conn }, deps)
      expect(s.getState().connection).toEqual(conn)
    })
  })

  describe('workspace management', () => {
    it('addWorkspace creates workspace and sets active', async () => {
      const id = store.getState().addWorkspace('/my/project')
      expect(id).toBeDefined()
      expect(store.getState().workspaces.get(id)).toBeDefined()
      // Immediately after addWorkspace, status is 'loading' with name available
      const loadingEntry = store.getState().workspaces.get(id)!
      expect(loadingEntry.status).toBe(WorkspaceEntryStatus.Loading)
      expect((loadingEntry as { name: string }).name).toBe('project')
      expect(store.getState().activeWorkspaceId).toBe(id)
      // After flush, workspace transitions to 'loaded' with full data
      await flushPromises()
      const entry = store.getState().workspaces.get(id)!
      expect(entry.status).toBe(WorkspaceEntryStatus.Loaded)
      expect((entry as Extract<typeof entry, { status: WorkspaceEntryStatus.Loaded }>).data.name).toBe('project')
      expect((entry as Extract<typeof entry, { status: WorkspaceEntryStatus.Loaded }>).data.path).toBe('/my/project')
    })

    it('addWorkspace queries git info', async () => {
      store.getState().addWorkspace('/my/repo')
      await flushPromises()
      expect(deps.git.getInfo).toHaveBeenCalledWith('/my/repo')
      const entry = Array.from(store.getState().workspaces.values())[0]!
      expect(entry.status).toBe(WorkspaceEntryStatus.Loaded)
      const ws = (entry as Extract<typeof entry, { status: WorkspaceEntryStatus.Loaded }>).data
      expect(ws.isGitRepo).toBe(true)
      expect(ws.gitBranch).toBe('main')
    })

    it('addWorkspace creates default tab when app registry returns app', async () => {
      const app = makeFakeApp()
      vi.mocked(deps.appRegistry.getDefaultApp).mockReturnValue(app)
      const id = store.getState().addWorkspace('/test')
      await flushPromises()
      const entry = store.getState().workspaces.get(id)!
      expect(entry.status).toBe(WorkspaceEntryStatus.Loaded)
      const ws = (entry as Extract<typeof entry, { status: WorkspaceEntryStatus.Loaded }>).data
      expect(Object.keys(ws.appStates)).toHaveLength(1)
      expect(ws.activeTabId).toBeDefined()
    })

    it('addWorkspace skips default tabs when option set', async () => {
      const app = makeFakeApp()
      vi.mocked(deps.appRegistry.getDefaultApp).mockReturnValue(app)
      const id = store.getState().addWorkspace('/test', { skipDefaultTabs: true })
      await flushPromises()
      const entry = store.getState().workspaces.get(id)!
      expect(entry.status).toBe(WorkspaceEntryStatus.Loaded)
      const ws = (entry as Extract<typeof entry, { status: WorkspaceEntryStatus.Loaded }>).data
      expect(Object.keys(ws.appStates)).toHaveLength(0)
    })

    it('setActiveWorkspace updates active workspace id', () => {
      const id = store.getState().addWorkspace('/test')
      store.getState().setActiveWorkspace(null)
      expect(store.getState().activeWorkspaceId).toBeNull()
      store.getState().setActiveWorkspace(id)
      expect(store.getState().activeWorkspaceId).toBe(id)
    })

    it('setActiveWorkspace triggers git refresh on new active workspace', async () => {
      const id = store.getState().addWorkspace('/test')
      await flushPromises()

      const entry = store.getState().workspaces.get(id)
      expect(entry?.status).toBe(WorkspaceEntryStatus.Loaded)
      if (entry?.status !== WorkspaceEntryStatus.Loaded) return

      const triggerRefresh = vi.spyOn(entry.store.getState().gitController.getState(), 'triggerRefresh')

      store.getState().setActiveWorkspace(null)
      store.getState().setActiveWorkspace(id)
      expect(triggerRefresh).toHaveBeenCalled()
    })
  })

  describe('child workspace operations', () => {
    let parentId: string

    beforeEach(async () => {
      parentId = store.getState().addWorkspace('/repo')
      await flushPromises()
    })

    it('addChildWorkspace creates worktree child', async () => {
      const result = store.getState().addChildWorkspace(parentId, 'feature')
      expect(result).toEqual({ success: true })
      await flushPromises()
      expect(deps.git.createWorktree).toHaveBeenCalled()

      const workspaces = store.getState().workspaces
      const children = Array.from(workspaces.values())
        .filter((e): e is Extract<typeof e, { status: WorkspaceEntryStatus.Loaded }> => e.status === WorkspaceEntryStatus.Loaded && e.data.parentId === parentId)
        .map(e => e.data)
      expect(children).toHaveLength(1)
      expect(children[0]!.name).toBe('feature')
      expect(children[0]!.isWorktree).toBe(true)
    })

    it('addChildWorkspace fails when parent not found', () => {
      const result = store.getState().addChildWorkspace('nonexistent', 'feat')
      expect(result).toEqual({ success: false, error: 'Parent workspace not found' })
    })

    it('addChildWorkspace fails when parent is not a git repo', async () => {
      vi.mocked(deps.git.getInfo).mockResolvedValue({ isRepo: false })
      const id = store.getState().addWorkspace('/no-git')
      await flushPromises()
      const result = store.getState().addChildWorkspace(id, 'feat')
      expect(result).toEqual({ success: false, error: 'Parent workspace is not a git repository' })
    })

    it('addChildWorkspace sets error state when git operation fails', async () => {
      vi.mocked(deps.git.createWorktree).mockResolvedValue({ success: false, error: 'git error' })
      const result = store.getState().addChildWorkspace(parentId, 'feat')
      expect(result).toEqual({ success: true })
      await flushPromises()
      const workspaces = store.getState().workspaces
      const errorEntry = Array.from(workspaces.values()).find(e => e.status === WorkspaceEntryStatus.Error)
      expect(errorEntry).toBeDefined()
      if (errorEntry) {
        expect(errorEntry.status).toBe(WorkspaceEntryStatus.Error)
        expect(errorEntry.error).toBe('git error')
      }
    })

    it('adoptExistingWorktree adds existing worktree', async () => {
      const result = await store.getState().adoptExistingWorktree(parentId, '/repo/.worktrees/existing', 'existing-branch', 'existing')
      expect(result).toEqual({ success: true })
    })

    it('adoptExistingWorktree fails when parent not found', async () => {
      const result = await store.getState().adoptExistingWorktree('bad', '/path', 'branch', 'name')
      expect(result).toEqual({ success: false, error: 'Parent workspace not found' })
    })

    it('adoptExistingWorktree fails when worktree already open', async () => {
      // First adopt
      await store.getState().adoptExistingWorktree(parentId, '/repo/.worktrees/dup', 'branch', 'dup')
      // Second adopt of same path
      const result = await store.getState().adoptExistingWorktree(parentId, '/repo/.worktrees/dup', 'branch', 'dup')
      expect(result).toEqual({ success: false, error: 'This worktree is already open' })
    })

    it('createWorktreeFromBranch creates child from branch', async () => {
      const result = store.getState().createWorktreeFromBranch(parentId, 'feature/my-feat', false)
      expect(result).toEqual({ success: true })
      await flushPromises()
      expect(deps.git.createWorktreeFromBranch).toHaveBeenCalledWith('/repo', 'feature/my-feat', 'my-feat', expect.any(Function))
    })

    it('createWorktreeFromBranch fails for non-git parent', async () => {
      vi.mocked(deps.git.getInfo).mockResolvedValue({ isRepo: false })
      const id = store.getState().addWorkspace('/no-git')
      await flushPromises()
      const result = store.getState().createWorktreeFromBranch(id, 'feat', false)
      expect(result).toEqual({ success: false, error: 'Parent workspace is not a git repository' })
    })

    it('createWorktreeFromRemote creates child from remote branch', async () => {
      const result = store.getState().createWorktreeFromRemote(parentId, 'origin/feature', false)
      expect(result).toEqual({ success: true })
      await flushPromises()
      expect(deps.git.createWorktreeFromRemote).toHaveBeenCalledWith('/repo', 'origin/feature', 'feature', expect.any(Function))
    })

    it('createWorktreeFromRemote fails for non-existent parent', () => {
      const result = store.getState().createWorktreeFromRemote('bad', 'origin/feat', false)
      expect(result).toEqual({ success: false, error: 'Parent workspace not found' })
    })
  })

  describe('workspace removal', () => {
    let parentId: string
    let childId: string

    beforeEach(async () => {
      parentId = store.getState().addWorkspace('/repo')
      await flushPromises()
      store.getState().addChildWorkspace(parentId, 'child')
      await flushPromises()
      const childEntry = Array.from(store.getState().workspaces.values())
        .find((e): e is Extract<typeof e, { status: WorkspaceEntryStatus.Loaded }> => e.status === WorkspaceEntryStatus.Loaded && e.data.name === 'child')
      expect(childEntry).toBeDefined()
      childId = childEntry?.data.id ?? ''
    })

    it('removeWorkspace removes child and cleans up git', async () => {
      await store.getState().removeWorkspace(childId)
      expect(store.getState().workspaces.get(childId)).toBeUndefined()
      expect(deps.git.removeWorktree).toHaveBeenCalled()
    })

    it('removeWorkspaceKeepBranch keeps branch', async () => {
      await store.getState().removeWorkspaceKeepBranch(childId)
      expect(store.getState().workspaces.get(childId)).toBeUndefined()
      // removeWorktree called with deleteBranch=false
      expect(deps.git.removeWorktree).toHaveBeenCalledWith('/repo', expect.any(String), false, undefined)
    })

    it('removeWorkspaceKeepBoth skips both worktree and branch removal', async () => {
      await store.getState().removeWorkspaceKeepBoth(childId)
      expect(store.getState().workspaces.get(childId)).toBeUndefined()
      expect(deps.git.removeWorktree).not.toHaveBeenCalled()
      expect(deps.git.deleteBranch).not.toHaveBeenCalled()
    })

    it('removeWorkspace resets active workspace when removing active', async () => {
      store.getState().setActiveWorkspace(childId)
      await store.getState().removeWorkspace(childId)
      expect(store.getState().activeWorkspaceId).toBeNull()
    })

    it('onWorkspaceRemoved removes without git cleanup', () => {
      store.getState().onWorkspaceRemoved(childId)
      expect(store.getState().workspaces.get(childId)).toBeUndefined()
      expect(deps.git.removeWorktree).not.toHaveBeenCalled()
    })

    it('onWorkspaceRemoved does nothing for non-existent workspace', () => {
      store.getState().onWorkspaceRemoved('bad-id')
      // Should not throw
    })

    it('onWorkspaceRemoved removes workspace from state', () => {
      store.getState().onWorkspaceRemoved(childId)
      expect(store.getState().workspaces.get(childId)).toBeUndefined()
    })
  })

  describe('git info', () => {
    it('updateGitInfo updates workspace git fields', async () => {
      const id = store.getState().addWorkspace('/test')
      await flushPromises()
      store.getState().updateGitInfo(id, { isRepo: true, branch: 'develop', rootPath: '/test' })
      const entry = store.getState().workspaces.get(id)!
      expect(entry.status).toBe(WorkspaceEntryStatus.Loaded)
      expect((entry as Extract<typeof entry, { status: WorkspaceEntryStatus.Loaded }>).data.gitBranch).toBe('develop')
    })

    it('updateGitInfo does nothing for non-existent workspace', () => {
      store.getState().updateGitInfo('bad', { isRepo: false })
      // Should not throw
    })

    it('refreshGitInfo re-queries git info', async () => {
      const id = store.getState().addWorkspace('/test')
      await flushPromises()
      vi.mocked(deps.git.getInfo).mockResolvedValue({ isRepo: true, branch: 'feature', rootPath: '/test' })
      await store.getState().refreshGitInfo(id)
      const entry = store.getState().workspaces.get(id)!
      expect(entry.status).toBe(WorkspaceEntryStatus.Loaded)
      expect((entry as Extract<typeof entry, { status: WorkspaceEntryStatus.Loaded }>).data.gitBranch).toBe('feature')
    })

    it('refreshGitInfo does nothing for non-existent workspace', async () => {
      await store.getState().refreshGitInfo('bad')
      // Should not throw
    })
  })

  describe('merge and clean', () => {
    let parentId: string
    let childId: string

    beforeEach(async () => {
      parentId = store.getState().addWorkspace('/repo')
      await flushPromises()
      store.getState().addChildWorkspace(parentId, 'child')
      await flushPromises()
      const childEntry = Array.from(store.getState().workspaces.values())
        .find((e): e is Extract<typeof e, { status: WorkspaceEntryStatus.Loaded }> => e.status === WorkspaceEntryStatus.Loaded && e.data.name === 'child')
      expect(childEntry).toBeDefined()
      childId = childEntry?.data.id ?? ''
    })

    it('mergeAndRemoveWorkspace merges, removes, and cleans up', async () => {
      const result = await store.getState().mergeAndRemoveWorkspace(childId, false)
      expect(result).toEqual({ success: true })
      expect(deps.git.merge).toHaveBeenCalled()
      expect(store.getState().workspaces.get(childId)).toBeUndefined()
    })

    it('mergeAndRemoveWorkspace auto-commits uncommitted changes', async () => {
      // First call is parent (clean), second call is child (dirty)
      vi.mocked(deps.git.hasUncommittedChanges)
        .mockResolvedValueOnce(false)
        .mockResolvedValueOnce(true)
      await store.getState().mergeAndRemoveWorkspace(childId, false)
      expect(deps.git.commitAll).toHaveBeenCalled()
    })

    it('mergeAndRemoveWorkspace fails when parent has uncommitted changes', async () => {
      vi.mocked(deps.git.hasUncommittedChanges).mockResolvedValueOnce(true)
      const result = await store.getState().mergeAndRemoveWorkspace(childId, false)
      expect(result.success).toBe(false)
      expect(result.error).toContain('Parent workspace has uncommitted changes')
      expect(deps.git.merge).not.toHaveBeenCalled()
    })

    it('mergeAndRemoveWorkspace fails when workspace not found', async () => {
      const result = await store.getState().mergeAndRemoveWorkspace('bad', false)
      expect(result).toEqual({ success: false, error: 'Workspace not found' })
    })

    it('mergeAndRemoveWorkspace fails for non-worktree', async () => {
      const result = await store.getState().mergeAndRemoveWorkspace(parentId, false)
      expect(result).toEqual({ success: false, error: 'Not a worktree workspace' })
    })

    it('mergeAndRemoveWorkspace fails when merge fails', async () => {
      vi.mocked(deps.git.merge).mockResolvedValue({ success: false, error: 'conflict' })
      const result = await store.getState().mergeAndRemoveWorkspace(childId, false)
      expect(result.success).toBe(false)
      expect(result.error).toContain('conflict')
    })

    it('mergeAndKeepWorkspace merges but keeps workspace alive', async () => {
      const result = await store.getState().mergeAndKeepWorkspace(childId, false)
      expect(result).toEqual({ success: true })
      expect(deps.git.merge).toHaveBeenCalled()
      expect(store.getState().workspaces.get(childId)).toBeDefined()
    })

    it('mergeAndKeepWorkspace auto-commits uncommitted changes', async () => {
      // First call is parent (clean), second call is child (dirty)
      vi.mocked(deps.git.hasUncommittedChanges)
        .mockResolvedValueOnce(false)
        .mockResolvedValueOnce(true)
      await store.getState().mergeAndKeepWorkspace(childId, false)
      expect(deps.git.commitAll).toHaveBeenCalled()
    })

    it('mergeAndKeepWorkspace fails when parent has uncommitted changes', async () => {
      vi.mocked(deps.git.hasUncommittedChanges).mockResolvedValueOnce(true)
      const result = await store.getState().mergeAndKeepWorkspace(childId, false)
      expect(result.success).toBe(false)
      expect(result.error).toContain('Parent workspace has uncommitted changes')
      expect(deps.git.merge).not.toHaveBeenCalled()
    })

    it('mergeAndKeepWorkspace fails when workspace not found', async () => {
      const result = await store.getState().mergeAndKeepWorkspace('bad', false)
      expect(result).toEqual({ success: false, error: 'Workspace not found' })
    })

    it('mergeAndKeepWorkspace fails for non-worktree', async () => {
      const result = await store.getState().mergeAndKeepWorkspace(parentId, false)
      expect(result).toEqual({ success: false, error: 'Not a worktree workspace' })
    })

    it('mergeAndKeepWorkspace fails when merge fails', async () => {
      vi.mocked(deps.git.merge).mockResolvedValue({ success: false, error: 'conflict' })
      const result = await store.getState().mergeAndKeepWorkspace(childId, false)
      expect(result.success).toBe(false)
      expect(result.error).toContain('conflict')
    })

    it('closeAndCleanWorkspace removes worktree', async () => {
      const result = await store.getState().closeAndCleanWorkspace(childId)
      expect(result).toEqual({ success: true })
      expect(store.getState().workspaces.get(childId)).toBeUndefined()
    })

    it('closeAndCleanWorkspace fails for non-existent workspace', async () => {
      const result = await store.getState().closeAndCleanWorkspace('bad')
      expect(result).toEqual({ success: false, error: 'Workspace not found' })
    })

    it('closeAndCleanWorkspace fails for non-worktree', async () => {
      const result = await store.getState().closeAndCleanWorkspace(parentId)
      expect(result).toEqual({ success: false, error: 'Not a worktree workspace' })
    })
  })

  describe('quickForkWorkspace', () => {
    it('creates a new child workspace with generated name', async () => {
      const parentId = store.getState().addWorkspace('/repo')
      await flushPromises()
      const result = await store.getState().quickForkWorkspace(parentId)
      expect(result.success).toBe(true)
      expect(deps.git.listLocalBranches).toHaveBeenCalled()
      await flushPromises()
      expect(deps.git.createWorktree).toHaveBeenCalled()
    })

    it('fails when workspace not found', async () => {
      const result = await store.getState().quickForkWorkspace('bad')
      expect(result).toEqual({ success: false, error: 'Workspace not found' })
    })

    it('fails when workspace has no git root', async () => {
      vi.mocked(deps.git.getInfo).mockResolvedValue({ isRepo: false })
      const id = store.getState().addWorkspace('/no-git')
      await flushPromises()
      const result = await store.getState().quickForkWorkspace(id)
      expect(result).toEqual({ success: false, error: 'Workspace has no git root path' })
    })
  })

  describe('syncToDaemon', () => {
    it('syncs session to daemon', async () => {
      store.getState().addWorkspace('/test')
      await flushPromises()
      await store.getState().syncToDaemon()
      expect(deps.sessionApi.update).toHaveBeenCalled()
    })

    it('skips sync when workspaces have not changed', async () => {
      // Mock returns incrementing versions so each accepted sync bumps sessionVersion correctly
      let mockVersion = 0
      vi.mocked(deps.sessionApi.update).mockImplementation(() => {
        mockVersion++
        // eslint-disable-next-line @typescript-eslint/no-unsafe-return
        return Promise.resolve({ success: true, session: { version: mockVersion, lock: null } } as any)
      })

      store.getState().addWorkspace('/test')
      await flushPromises()
      await store.getState().syncToDaemon()

      // Clear mock to measure subsequent calls only
      vi.mocked(deps.sessionApi.update).mockClear()

      // Second sync with same state should be skipped
      await store.getState().syncToDaemon()
      expect(deps.sessionApi.update).not.toHaveBeenCalled()
    })

    it('syncs again after workspace state changes', async () => {
      let mockVersion = 0
      vi.mocked(deps.sessionApi.update).mockImplementation(() => {
        mockVersion++
        // eslint-disable-next-line @typescript-eslint/no-unsafe-return
        return Promise.resolve({ success: true, session: { version: mockVersion, lock: null } } as any)
      })

      const id = store.getState().addWorkspace('/test')
      await flushPromises()
      await store.getState().syncToDaemon()

      vi.mocked(deps.sessionApi.update).mockClear()

      // Change workspace metadata
      const entry = store.getState().workspaces.get(id)!
      if (entry.status === WorkspaceEntryStatus.Loaded) {
        entry.store.getState().updateMetadata('displayName', 'changed')
      }
      await store.getState().syncToDaemon()
      expect(deps.sessionApi.update).toHaveBeenCalledTimes(1)
    })

    it('syncs again after external update invalidates cache', async () => {
      let mockVersion = 0
      vi.mocked(deps.sessionApi.update).mockImplementation(() => {
        mockVersion++
        // eslint-disable-next-line @typescript-eslint/no-unsafe-return
        return Promise.resolve({ success: true, session: { version: mockVersion, lock: null } } as any)
      })

      store.getState().addWorkspace('/test')
      await flushPromises()
      await store.getState().syncToDaemon()

      vi.mocked(deps.sessionApi.update).mockClear()

      // Simulate external update — must use version > current to be accepted
      const currentVersion = store.getState().sessionVersion
      await store.getState().handleExternalUpdate({
        id: 'session-ext',
        workspaces: [{
          id: 'ws-ext',
          path: '/test',
          name: 'test',
          parentId: null,
          status: 'active',
          isGitRepo: false,
          gitBranch: null,
          gitRootPath: null,
          isWorktree: false,
          isDetached: false,
          appStates: {},
          activeTabId: null,
          createdAt: 0,
          lastActivity: 0,
          metadata: {},
          settings: { defaultApplicationId: '' },
        }],
        createdAt: 0,
        lastActivity: 0,
        version: currentVersion + 1,
        lock: null,
      })

      // Should sync again since external update invalidated cache
      mockVersion = currentVersion + 1
      await store.getState().syncToDaemon()
      expect(deps.sessionApi.update).toHaveBeenCalledTimes(1)
    })
  })

  describe('getDefaultAppForWorktree', () => {
    it('uses worktree settings defaultApplicationId when set', async () => {
      const app = makeFakeApp({ id: 'custom-app' })
      vi.mocked(deps.appRegistry.get).mockReturnValue(app)

      const id = store.getState().addWorkspace('/test', {
        settings: { defaultApplicationId: 'custom-app' },
      })
      await flushPromises()
      const entry = store.getState().workspaces.get(id)!
      expect(entry.status).toBe(WorkspaceEntryStatus.Loaded)
      const ws = (entry as Extract<typeof entry, { status: WorkspaceEntryStatus.Loaded }>).data
      const tab = Object.values(ws.appStates)[0]!
      expect(tab.applicationId).toBe('custom-app')
    })

    it('falls back to global default app', async () => {
      const app = makeFakeApp({ id: 'global-default' })
      vi.mocked(deps.appRegistry.get).mockReturnValue(undefined)
      vi.mocked(deps.appRegistry.getDefaultApp).mockReturnValue(app)

      const id = store.getState().addWorkspace('/test')
      await flushPromises()
      const entry = store.getState().workspaces.get(id)!
      expect(entry.status).toBe(WorkspaceEntryStatus.Loaded)
      const ws = (entry as Extract<typeof entry, { status: WorkspaceEntryStatus.Loaded }>).data
      const tab = Object.values(ws.appStates)[0]!
      expect(tab.applicationId).toBe('global-default')
    })

    it('falls back to parent settings when worktree settings app not found', async () => {
      const parentApp = makeFakeApp({ id: 'parent-app' })
      vi.mocked(deps.appRegistry.get).mockImplementation((id: string) => {
        if (id === 'parent-app') return parentApp
        return undefined
      })

      // Create parent workspace with settings
      const parentId = store.getState().addWorkspace('/parent', {
        settings: { defaultApplicationId: 'parent-app' },
      })
      await flushPromises()

      // Create child — worktree settings has unknown app, should fall back to parent
      const result = store.getState().addChildWorkspace(parentId, 'child', false, { defaultApplicationId: 'unknown-app' })
      expect(result.success).toBe(true)
      await flushPromises()
    })

    it('falls back to globalDefaultApplicationId from settings', async () => {
      const globalApp = makeFakeApp({ id: 'global-setting-app' })
      vi.mocked(deps.appRegistry.get).mockImplementation((id: string) => {
        if (id === 'global-setting-app') return globalApp
        return undefined
      })
      vi.mocked(deps.getSettings).mockReturnValue({
        ...deps.getSettings(),
        globalDefaultApplicationId: 'global-setting-app',
      })

      const id = store.getState().addWorkspace('/test')
      await flushPromises()
      const entry = store.getState().workspaces.get(id)!
      expect(entry.status).toBe(WorkspaceEntryStatus.Loaded)
      const ws = (entry as Extract<typeof entry, { status: WorkspaceEntryStatus.Loaded }>).data
      const tab = Object.values(ws.appStates)[0]!
      expect(tab.applicationId).toBe('global-setting-app')
    })

    it('falls back to globalDefaultApplicationId even when app not found in registry', async () => {
      vi.mocked(deps.appRegistry.get).mockReturnValue(undefined)
      const fallbackApp = makeFakeApp({ id: 'fallback' })
      vi.mocked(deps.appRegistry.getDefaultApp).mockReturnValue(fallbackApp)
      vi.mocked(deps.getSettings).mockReturnValue({
        ...deps.getSettings(),
        globalDefaultApplicationId: 'nonexistent-app',
      })

      const id = store.getState().addWorkspace('/test')
      await flushPromises()
      const entry = store.getState().workspaces.get(id)!
      expect(entry.status).toBe(WorkspaceEntryStatus.Loaded)
      const ws = (entry as Extract<typeof entry, { status: WorkspaceEntryStatus.Loaded }>).data
      const tab = Object.values(ws.appStates)[0]!
      expect(tab.applicationId).toBe('fallback')
    })
  })

  describe('syncToDaemon error handling', () => {
    it('handles sync failure gracefully', async () => {
      vi.mocked(deps.sessionApi.update).mockResolvedValue({ success: false, error: 'sync failed' })
      store.getState().addWorkspace('/test')
      await flushPromises()
      await store.getState().syncToDaemon()
      // Should not throw
    })

    it('handles sync exception gracefully', async () => {
      vi.mocked(deps.sessionApi.update).mockRejectedValue(new Error('network error'))
      store.getState().addWorkspace('/test')
      await flushPromises()
      await store.getState().syncToDaemon()
      // Should not throw
    })
  })

  describe('createTty', () => {
    it('returns pty session id on success', async () => {
      const ptyId = await store.getState().createTty('/test')
      expect(ptyId).toBe('pty-1')
    })

    it('throws when terminal.create fails with error message', async () => {
      vi.mocked(deps.terminal.create).mockResolvedValue({ success: false, error: 'No PTY available' })
      await expect(store.getState().createTty('/test')).rejects.toThrow('No PTY available')
    })

    it('throws default message when terminal.create fails without error', async () => {
      vi.mocked(deps.terminal.create).mockResolvedValue({ success: false, error: '' } as never)
      await expect(store.getState().createTty('/test')).rejects.toThrow('Failed to create PTY')
    })
  })

  describe('openTtyStream', () => {
    it('throws when terminal.attach fails with error message', async () => {
      vi.mocked(deps.terminal.attach).mockResolvedValue({ success: false, error: 'PTY not found' })
      await expect(store.getState().openTtyStream('pty-1', vi.fn())).rejects.toThrow('PTY not found')
    })

    it('throws default message when terminal.attach fails without error', async () => {
      vi.mocked(deps.terminal.attach).mockResolvedValue({ success: false, error: '' } as never)
      await expect(store.getState().openTtyStream('pty-1', vi.fn())).rejects.toThrow('Failed to attach to PTY')
    })
  })

  describe('clearWorkspaceError', () => {
    it('no-ops for non-existent workspace', () => {
      store.getState().clearWorkspaceError('nonexistent')
      // Should not throw
    })

    it('no-ops for workspace not in OperationError status', async () => {
      const id = store.getState().addWorkspace('/test')
      await flushPromises()
      const entryBefore = store.getState().workspaces.get(id)!
      expect(entryBefore.status).toBe(WorkspaceEntryStatus.Loaded)

      store.getState().clearWorkspaceError(id)
      const entryAfter = store.getState().workspaces.get(id)!
      expect(entryAfter.status).toBe(WorkspaceEntryStatus.Loaded)
    })
  })

  describe('reorderWorkspace', () => {
    it('no-ops when drag workspace does not exist', async () => {
      const id = store.getState().addWorkspace('/test')
      await flushPromises()
      store.getState().reorderWorkspace('nonexistent', id, 'before')
      // Should not throw
    })

    it('no-ops when target workspace does not exist', async () => {
      const id = store.getState().addWorkspace('/test')
      await flushPromises()
      store.getState().reorderWorkspace(id, 'nonexistent', 'before')
      // Should not throw
    })

    it('no-ops when drag and target are the same workspace', async () => {
      const id = store.getState().addWorkspace('/test')
      await flushPromises()
      store.getState().reorderWorkspace(id, id, 'before')
      // Should not throw
    })

    it('reorders sibling workspaces', async () => {
      const id1 = store.getState().addWorkspace('/test1')
      const id2 = store.getState().addWorkspace('/test2')
      await flushPromises()
      store.getState().reorderWorkspace(id1, id2, 'after')
      // Should not throw — verifies the reorder path runs
    })
  })

  describe('moveWorkspace', () => {
    function getLoadedData(id: string) {
      const entry = store.getState().workspaces.get(id)
      if (!entry || (entry.status !== WorkspaceEntryStatus.Loaded && entry.status !== WorkspaceEntryStatus.OperationError)) return null
      return entry.data
    }

    it('no-ops when drag workspace does not exist', async () => {
      const id = store.getState().addWorkspace('/test')
      await flushPromises()
      store.getState().moveWorkspace('nonexistent', id, 'onto')
      // Should not throw
    })

    it('no-ops when target workspace does not exist', async () => {
      const id = store.getState().addWorkspace('/test')
      await flushPromises()
      store.getState().moveWorkspace(id, 'nonexistent', 'onto')
      // Should not throw
    })

    it('no-ops when drag and target are the same', async () => {
      const id = store.getState().addWorkspace('/test')
      await flushPromises()
      store.getState().moveWorkspace(id, id, 'onto')
      expect(getLoadedData(id)?.parentId).toBeNull()
    })

    it('delegates to reorderWorkspace for same-parent before/after', async () => {
      const id1 = store.getState().addWorkspace('/test1')
      const id2 = store.getState().addWorkspace('/test2')
      await flushPromises()
      // id1 sortOrder=0, id2 sortOrder=1
      store.getState().moveWorkspace(id1, id2, 'after')
      // id1 should now be after id2: sortOrder id2=0, id1=1
      expect(parseInt(getLoadedData(id1)?.metadata.sortOrder || '0')).toBeGreaterThan(
        parseInt(getLoadedData(id2)?.metadata.sortOrder || '0')
      )
    })

    it('reparents workspace onto target', async () => {
      const parent = store.getState().addWorkspace('/parent')
      const child = store.getState().addWorkspace('/child')
      await flushPromises()
      expect(getLoadedData(child)?.parentId).toBeNull()

      store.getState().moveWorkspace(child, parent, 'onto')
      expect(getLoadedData(child)?.parentId).toBe(parent)
    })

    it('moves workspace before target in different parent group', async () => {
      const root1 = store.getState().addWorkspace('/root1')
      const root2 = store.getState().addWorkspace('/root2')
      await flushPromises()
      // Create children under root1
      store.getState().addChildWorkspace(root1, 'child-a')
      await flushPromises()
      const childA = Array.from(store.getState().workspaces.entries())
        .find(([, e]) => e.status === WorkspaceEntryStatus.Loaded && e.data.name === 'child-a')?.[0]

      // Move root2 before childA (which has parentId=root1)
      store.getState().moveWorkspace(root2, childA!, 'before')
      // root2 should now have parentId=root1
      expect(getLoadedData(root2)?.parentId).toBe(root1)
      // root2 should be before childA in sort order
      expect(parseInt(getLoadedData(root2)?.metadata.sortOrder || '0')).toBeLessThan(
        parseInt(getLoadedData(childA!)?.metadata.sortOrder || '0')
      )
    })

    it('prevents cycle when dropping onto own descendant', async () => {
      const parent = store.getState().addWorkspace('/parent')
      await flushPromises()
      store.getState().addChildWorkspace(parent, 'child')
      await flushPromises()
      const child = Array.from(store.getState().workspaces.entries())
        .find(([, e]) => e.status === WorkspaceEntryStatus.Loaded && e.data.name === 'child')?.[0]

      // Try to drop parent onto its own child — should be a no-op
      store.getState().moveWorkspace(parent, child!, 'onto')
      expect(getLoadedData(parent)?.parentId).toBeNull()
    })

    it('reindexes old siblings after reparent', async () => {
      const ws1 = store.getState().addWorkspace('/ws1')
      const ws2 = store.getState().addWorkspace('/ws2')
      const ws3 = store.getState().addWorkspace('/ws3')
      const target = store.getState().addWorkspace('/target')
      await flushPromises()
      // ws1=0, ws2=1, ws3=2, target=3

      // Move ws2 onto target — remaining roots should be ws1=0, ws3=1, target=2
      store.getState().moveWorkspace(ws2, target, 'onto')
      expect(getLoadedData(ws2)?.parentId).toBe(target)
      expect(getLoadedData(ws1)?.metadata.sortOrder).toBe('0')
      expect(getLoadedData(ws3)?.metadata.sortOrder).toBe('1')
      expect(getLoadedData(target)?.metadata.sortOrder).toBe('2')
    })
  })

  describe('addChildWorkspace error paths', () => {
    it('handles createWorktree failure', async () => {
      vi.mocked(deps.git.createWorktree).mockResolvedValue({ success: false, error: 'already exists' })

      const parentId = store.getState().addWorkspace('/parent')
      await flushPromises()

      store.getState().addChildWorkspace(parentId, 'child')
      await flushPromises()
      // Unlock should still be called
      expect(deps.sessionApi.unlock).toHaveBeenCalled()
    })

    it('handles thrown exception during workspace creation', async () => {
      vi.mocked(deps.git.createWorktree).mockRejectedValue(new Error('disk full'))

      const parentId = store.getState().addWorkspace('/parent')
      await flushPromises()

      store.getState().addChildWorkspace(parentId, 'child')
      await flushPromises()

      // Should set error state on the workspace
      const entries = Array.from(store.getState().workspaces.values())
      const errorEntry = entries.find(e => e.status === WorkspaceEntryStatus.Error)
      expect(errorEntry).toBeDefined()
      if (errorEntry?.status === WorkspaceEntryStatus.Error) {
        expect(errorEntry.error).toContain('disk full')
      }
    })

    it('creates child without description in metadata', async () => {
      const parentId = store.getState().addWorkspace('/parent')
      await flushPromises()

      const result = store.getState().addChildWorkspace(parentId, 'child', false, undefined, undefined)
      expect(result.success).toBe(true)
      await flushPromises()
    })
  })

  describe('mergeAndRemoveWorkspace', () => {
    it('returns error when session lock is held by another', async () => {
      // Lock returns success but not acquired — held by another, force-unlock + retry still fails
      vi.mocked(deps.sessionApi.lock).mockResolvedValue({ success: true, acquired: false, session: { id: 'session-1', workspaces: [], createdAt: 0, lastActivity: 0, version: 1, lock: null } } as never)

      const id = store.getState().addWorkspace('/test')
      await flushPromises()

      const result = await store.getState().mergeAndRemoveWorkspace(id, false)
      expect(result).toEqual({ success: false, error: 'Session is locked by another window' })
    })

    it('returns actual error when lock IPC call fails', async () => {
      vi.mocked(deps.sessionApi.lock).mockResolvedValue({ success: false, error: 'gRPC connection lost' } as never)

      const id = store.getState().addWorkspace('/test')
      await flushPromises()

      const result = await store.getState().mergeAndRemoveWorkspace(id, false)
      expect(result).toEqual({ success: false, error: 'gRPC connection lost' })
    })

    it('returns error when workspace is not a worktree', async () => {
      const id = store.getState().addWorkspace('/test')
      await flushPromises()

      const result = await store.getState().mergeAndRemoveWorkspace(id, false)
      expect(result.success).toBe(false)
      if (!result.success) {
        expect(result.error).toContain('Not a worktree')
      }
    })
  })

  describe('mergeAndKeepWorkspace', () => {
    it('returns error when workspace is not a worktree', async () => {
      const id = store.getState().addWorkspace('/test')
      await flushPromises()

      const result = await store.getState().mergeAndKeepWorkspace(id, false)
      expect(result.success).toBe(false)
    })
  })

  describe('removeWorkspace', () => {
    it('removes a workspace', async () => {
      const id = store.getState().addWorkspace('/test')
      await flushPromises()
      await store.getState().removeWorkspace(id)
      expect(store.getState().workspaces.get(id)).toBeUndefined()
    })

    it('handles removeWorktree failure gracefully', async () => {
      vi.mocked(deps.git.removeWorktree).mockRejectedValue(new Error('removal failed'))

      const parentId = store.getState().addWorkspace('/parent')
      await flushPromises()

      store.getState().addChildWorkspace(parentId, 'child')
      await flushPromises()

      // Find the child workspace
      const childId = Array.from(store.getState().workspaces.entries())
        .find(([, e]) => e.status === WorkspaceEntryStatus.Loaded && e.data.isWorktree)?.[0]

      if (childId) {
        await store.getState().removeWorkspace(childId)
        // Should show operation error
        const entry = store.getState().workspaces.get(childId)
        if (entry) {
          expect(entry.status).toBe(WorkspaceEntryStatus.OperationError)
        }
      }
    })
  })

  describe('removeWorkspaceKeepBranch', () => {
    it('removes workspace but keeps the branch', async () => {
      const id = store.getState().addWorkspace('/test')
      await flushPromises()
      await store.getState().removeWorkspaceKeepBranch(id)
      expect(store.getState().workspaces.get(id)).toBeUndefined()
    })
  })

  describe('removeWorkspaceKeepBoth', () => {
    it('removes workspace keeping both worktree and branch', async () => {
      const id = store.getState().addWorkspace('/test')
      await flushPromises()
      await store.getState().removeWorkspaceKeepBoth(id)
      expect(store.getState().workspaces.get(id)).toBeUndefined()
    })
  })

  describe('forceUnlock', () => {
    it('force unlocks session with existing lock', async () => {
      // Set up a lock via handleRestore
      await store.getState().handleRestore({
        id: 'session-1',
        workspaces: [],
        createdAt: 0,
        lastActivity: 0,
        version: 1,
        lock: { acquiredAt: Date.now(), expiresAt: Date.now() + 60000 },
      })
      const result = await store.getState().forceUnlock()
      expect(result.success).toBe(true)
      expect(deps.sessionApi.forceUnlock).toHaveBeenCalled()
    })

    it('returns success when no lock exists', async () => {
      // No lock by default — still calls daemon to clear any stale locks
      const result = await store.getState().forceUnlock()
      expect(result.success).toBe(true)
      expect(deps.sessionApi.forceUnlock).toHaveBeenCalled()
    })

    it('handles forceUnlock failure', async () => {
      // First set up a lock so forceUnlock actually calls the API
      await store.getState().handleRestore({
        id: 'session-1',
        workspaces: [],
        createdAt: 0,
        lastActivity: 0,
        version: 1,
        lock: { acquiredAt: Date.now(), expiresAt: Date.now() + 60000 },
      })
      vi.mocked(deps.sessionApi.forceUnlock).mockResolvedValue({ success: false, error: 'failed' } as never)
      const result = await store.getState().forceUnlock()
      expect(result.success).toBe(false)
    })
  })

  describe('session restore', () => {
    it('handleRestore restores workspaces from daemon session', async () => {
      const daemonSession = {
        id: 'session-1',
        workspaces: [
          makeWorkspace({ id: 'ws-restored', name: 'restored', path: '/restored', appStates: {}, activeTabId: null }),
        ],
        createdAt: Date.now(),
        lastActivity: Date.now(),
        version: 1,
        lock: null,
      }

      await store.getState().handleRestore(daemonSession)

      const entry = store.getState().workspaces.get('ws-restored')!
      expect(entry).toBeDefined()
      expect(entry.status).toBe(WorkspaceEntryStatus.Loaded)
      expect((entry as Extract<typeof entry, { status: WorkspaceEntryStatus.Loaded }>).data.name).toBe('restored')
      expect(store.getState().isRestoring).toBe(false)
    })

    it('handleExternalUpdate applies external changes', async () => {
      // First add a workspace
      store.getState().addWorkspace('/existing')
      await flushPromises()
      const existingId = Array.from(store.getState().workspaces.keys())[0]

      const daemonSession = {
        id: 'session-1',
        workspaces: [
          makeWorkspace({ id: 'ws-new', name: 'new-workspace', path: '/new' }),
        ],
        createdAt: Date.now(),
        lastActivity: Date.now(),
        version: 1,
        lock: null,
      }

      await store.getState().handleExternalUpdate(daemonSession)

      // Old workspace should be removed (not in daemon session)
      expect(store.getState().workspaces.get(existingId!)).toBeUndefined()
      // New workspace should be added
      expect(store.getState().workspaces.get('ws-new')).toBeDefined()
      expect(store.getState().isRestoring).toBe(false)
    })

    it('handleRestore with tab changes — removes old tabs and adds new tabs', async () => {
      // First restore with tab1
      const daemonSession1 = {
        id: 'session-1',
        workspaces: [
          makeWorkspace({ id: 'ws-tabs', name: 'tabs-test', path: '/tabs', appStates: { 'tab-1': { applicationId: 'terminal', title: 'Terminal', state: {} } }, activeTabId: 'tab-1' }),
        ],
        createdAt: Date.now(),
        lastActivity: Date.now(),
        version: 1,
        lock: null,
      }
      await store.getState().handleRestore(daemonSession1)
      expect(store.getState().workspaces.get('ws-tabs')).toBeDefined()

      // Second restore with different tabs — restoreWorkspaceTabs should dispose tab-1, init tab-2
      const daemonSession2 = {
        id: 'session-1',
        workspaces: [
          makeWorkspace({ id: 'ws-tabs', name: 'tabs-test', path: '/tabs', appStates: { 'tab-2': { applicationId: 'terminal', title: 'New Tab', state: {} } }, activeTabId: 'tab-2' }),
        ],
        createdAt: Date.now(),
        lastActivity: Date.now(),
        version: 2,
        lock: null,
      }
      await store.getState().handleRestore(daemonSession2)

      const entry = store.getState().workspaces.get('ws-tabs')!
      expect(entry.status).toBe(WorkspaceEntryStatus.Loaded)
      // restoreWorkspaceTabs updates the workspace store, not the session entry data
      const wsStore = (entry as Extract<typeof entry, { status: WorkspaceEntryStatus.Loaded }>).store
      const wsState = wsStore.getState().workspace
      expect(wsState.appStates['tab-2']).toBeDefined()
      expect(wsState.appStates['tab-1']).toBeUndefined()
    })

    it('handleRestore restores child workspaces with parent relationship', async () => {
      const daemonSession = {
        id: 'session-1',
        workspaces: [
          makeWorkspace({ id: 'ws-parent', name: 'parent', path: '/parent' }),
          makeWorkspace({ id: 'ws-child', name: 'child', path: '/child', parentId: 'ws-parent', isWorktree: true }),
        ],
        createdAt: Date.now(),
        lastActivity: Date.now(),
        version: 1,
        lock: null,
      }

      await store.getState().handleRestore(daemonSession)

      expect(store.getState().workspaces.get('ws-parent')).toBeDefined()
      expect(store.getState().workspaces.get('ws-child')).toBeDefined()
      const childEntry = store.getState().workspaces.get('ws-child')!
      expect(childEntry.status).toBe(WorkspaceEntryStatus.Loaded)
      expect((childEntry as Extract<typeof childEntry, { status: WorkspaceEntryStatus.Loaded }>).data.parentId).toBe('ws-parent')
    })
  })
})
