import { describe, it, expect, vi, beforeEach } from 'vitest'
import { createSessionStore, getUnmergedSubWorkspaces } from './createSessionStore'
import type { SessionDeps, SessionState } from './createSessionStore'
import type { Workspace, Application, GitInfo } from '../types'
import type { StoreApi } from 'zustand'

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
      onOutput: vi.fn().mockReturnValue(() => {}),
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
    sessionApi: {
      create: vi.fn().mockResolvedValue({ success: true }),
      update: vi.fn().mockResolvedValue({ success: true }),
      list: vi.fn().mockResolvedValue({ success: true, sessions: [] }),
      get: vi.fn().mockResolvedValue({ success: true }),
      delete: vi.fn().mockResolvedValue({ success: true }),
      openInNewWindow: vi.fn().mockResolvedValue({ success: true }),
      onShowSessions: vi.fn().mockReturnValue(() => {}),
      onSync: vi.fn().mockReturnValue(() => {}),
    },
    terminal: {
      create: vi.fn().mockResolvedValue({ sessionId: 'pty-1', handle: 'handle-1' }),
      attach: vi.fn().mockResolvedValue({ success: true, handle: 'handle-1', scrollback: [] }),
      list: vi.fn().mockResolvedValue([]),
      write: vi.fn(),
      resize: vi.fn(),
      kill: vi.fn(),
      isAlive: vi.fn().mockResolvedValue(true),
      onEvent: vi.fn().mockReturnValue(() => {}),
      onActiveProcessesOpen: vi.fn().mockReturnValue(() => {}),
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
    },
    setActivityTabState: vi.fn(),
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
    appStates: {},
    activeTabId: null,
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
    onWorkspaceLoad: () => ({ dispose: () => {} }),
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
    store = createSessionStore({ sessionId: 'session-1', windowUuid: 'win-1' }, deps)
  })

  describe('initial state', () => {
    it('has correct session id', () => {
      expect(store.getState().sessionId).toBe('session-1')
    })

    it('has empty workspace collection', () => {
      expect(store.getState().workspaces).toEqual({})
      expect(store.getState().workspaceStores).toEqual({})
    })

    it('has null active workspace', () => {
      expect(store.getState().activeWorkspaceId).toBeNull()
    })

    it('is not restoring', () => {
      expect(store.getState().isRestoring).toBe(false)
    })

    it('has null connection by default', () => {
      expect(store.getState().connection).toBeNull()
    })

    it('preserves connection when provided', () => {
      const conn = { id: 'conn-1', target: { type: 'local' as const }, host: 'example.com', status: 'connected' as const }
      const s = createSessionStore({ sessionId: 's', windowUuid: null, connection: conn }, deps)
      expect(s.getState().connection).toEqual(conn)
    })
  })

  describe('TTY management', () => {
    it('createTty creates a PTY and stores writer', async () => {
      const ptyId = await store.getState().createTty('/home')
      expect(ptyId).toBe('pty-1')
      expect(store.getState().ttyWriters['pty-1']).toBeDefined()
      expect(store.getState().ttyWriters['pty-1'].write).toBeDefined()
      expect(store.getState().ttyWriters['pty-1'].kill).toBeDefined()
      expect(deps.terminal.create).toHaveBeenCalledWith('local', '/home', undefined, undefined)
    })

    it('createTty throws when terminal.create returns null', async () => {
      vi.mocked(deps.terminal.create).mockResolvedValue(null)
      await expect(store.getState().createTty('/home')).rejects.toThrow('Failed to create PTY')
    })

    it('openTtyStream opens a stream and returns Tty without storing', async () => {
      const result = await store.getState().openTtyStream('pty-2')
      expect(result.tty).toBeDefined()
      expect(result.scrollback).toEqual([])
      expect(result.exitCode).toBeUndefined()
      // Should NOT be stored in ttyWriters
      expect(store.getState().ttyWriters['pty-2']).toBeUndefined()
    })

    it('openTtyStream throws when attach fails', async () => {
      vi.mocked(deps.terminal.attach).mockResolvedValue({ success: false, error: 'not found' })
      await expect(store.getState().openTtyStream('pty-x')).rejects.toThrow('not found')
    })

    it('getTtyWriter auto-creates writer by attaching', async () => {
      const writer = await store.getState().getTtyWriter('pty-1')
      expect(writer.write).toBeDefined()
      expect(deps.terminal.attach).toHaveBeenCalledWith('local', 'pty-1')
    })

    it('getTtyWriter returns cached writer after createTty', async () => {
      await store.getState().createTty('/home')
      const writer = await store.getState().getTtyWriter('pty-1')
      expect(writer.write).toBeDefined()
      expect(deps.terminal.attach).not.toHaveBeenCalled()
    })

    it('getTtyWriter throws when attach fails', async () => {
      vi.mocked(deps.terminal.attach).mockResolvedValue({ success: false, error: 'not found' })
      await expect(store.getState().getTtyWriter('pty-x')).rejects.toThrow('not found')
    })

    it('killTty kills the PTY', () => {
      store.getState().killTty('pty-1')
      expect(deps.terminal.kill).toHaveBeenCalledWith('local', 'pty-1')
    })

    it('listTty delegates to terminal.list', async () => {
      vi.mocked(deps.terminal.list).mockResolvedValue([{ id: 'pty-1', cwd: '/home', cols: 80, rows: 24, createdAt: Date.now(), lastActivity: Date.now() }])
      const result = await store.getState().listTty()
      expect(result).toHaveLength(1)
    })
  })

  describe('workspace management', () => {
    it('addWorkspace creates workspace and sets active', async () => {
      const id = await store.getState().addWorkspace('/my/project')
      expect(id).toBeDefined()
      expect(store.getState().workspaces[id]).toBeDefined()
      expect(store.getState().workspaces[id].name).toBe('project')
      expect(store.getState().workspaces[id].path).toBe('/my/project')
      expect(store.getState().activeWorkspaceId).toBe(id)
    })

    it('addWorkspace queries git info', async () => {
      store.getState().addWorkspace('/my/repo')
      await flushPromises()
      expect(deps.git.getInfo).toHaveBeenCalledWith('/my/repo')
      const ws = Object.values(store.getState().workspaces)[0]
      expect(ws.isGitRepo).toBe(true)
      expect(ws.gitBranch).toBe('main')
    })

    it('addWorkspace creates default tab when app registry returns app', async () => {
      const app = makeFakeApp()
      vi.mocked(deps.appRegistry.getDefaultApp).mockReturnValue(app)
      const id = await store.getState().addWorkspace('/test')
      const ws = store.getState().workspaces[id]
      expect(Object.keys(ws.appStates)).toHaveLength(1)
      expect(ws.activeTabId).toBeDefined()
    })

    it('addWorkspace skips default tabs when option set', async () => {
      const app = makeFakeApp()
      vi.mocked(deps.appRegistry.getDefaultApp).mockReturnValue(app)
      const id = await store.getState().addWorkspace('/test', { skipDefaultTabs: true })
      const ws = store.getState().workspaces[id]
      expect(Object.keys(ws.appStates)).toHaveLength(0)
    })

    it('getWorkspace returns handle for existing workspace', async () => {
      const id = await store.getState().addWorkspace('/test')
      expect(store.getState().getWorkspace(id)).not.toBeNull()
    })

    it('getWorkspace returns null for non-existent workspace', () => {
      expect(store.getState().getWorkspace('nonexistent')).toBeNull()
    })

    it('setActiveWorkspace updates active workspace id', async () => {
      const id = await store.getState().addWorkspace('/test')
      store.getState().setActiveWorkspace(null)
      expect(store.getState().activeWorkspaceId).toBeNull()
      store.getState().setActiveWorkspace(id)
      expect(store.getState().activeWorkspaceId).toBe(id)
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
      const children = Object.values(workspaces).filter((ws) => ws.parentId === parentId)
      expect(children).toHaveLength(1)
      expect(children[0].name).toBe('feature')
      expect(children[0].isWorktree).toBe(true)
    })

    it('addChildWorkspace fails when parent not found', async () => {
      const result = await store.getState().addChildWorkspace('nonexistent', 'feat')
      expect(result).toEqual({ success: false, error: 'Parent workspace not found' })
    })

    it('addChildWorkspace fails when parent is not a git repo', async () => {
      vi.mocked(deps.git.getInfo).mockResolvedValue({ isRepo: false, branch: null, rootPath: null })
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
      const loadStates = store.getState().workspaceLoadStates
      const errorState = Object.values(loadStates).find(s => s.status === 'error')
      expect(errorState).toEqual({ status: 'error', error: 'git error' })
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
      expect(deps.git.createWorktreeFromBranch).toHaveBeenCalledWith('/repo', 'feature/my-feat', 'my-feat', expect.any(String))
    })

    it('createWorktreeFromBranch fails for non-git parent', async () => {
      vi.mocked(deps.git.getInfo).mockResolvedValue({ isRepo: false, branch: null, rootPath: null })
      const id = store.getState().addWorkspace('/no-git')
      await flushPromises()
      const result = store.getState().createWorktreeFromBranch(id, 'feat', false)
      expect(result).toEqual({ success: false, error: 'Parent workspace is not a git repository' })
    })

    it('createWorktreeFromRemote creates child from remote branch', async () => {
      const result = store.getState().createWorktreeFromRemote(parentId, 'origin/feature', false)
      expect(result).toEqual({ success: true })
      await flushPromises()
      expect(deps.git.createWorktreeFromRemote).toHaveBeenCalledWith('/repo', 'origin/feature', 'feature', expect.any(String))
    })

    it('createWorktreeFromRemote fails for non-existent parent', async () => {
      const result = await store.getState().createWorktreeFromRemote('bad', 'origin/feat', false)
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
      const childWs = Object.values(store.getState().workspaces).find((ws) => ws.name === 'child')!
      childId = childWs.id
    })

    it('removeWorkspace removes child and cleans up git', async () => {
      await store.getState().removeWorkspace(childId)
      expect(store.getState().workspaces[childId]).toBeUndefined()
      expect(deps.git.removeWorktree).toHaveBeenCalled()
    })

    it('removeWorkspaceKeepBranch keeps branch', async () => {
      await store.getState().removeWorkspaceKeepBranch(childId)
      expect(store.getState().workspaces[childId]).toBeUndefined()
      // removeWorktree called with deleteBranch=false
      expect(deps.git.removeWorktree).toHaveBeenCalledWith('/repo', expect.any(String), false, expect.any(String))
    })

    it('removeWorkspaceKeepBoth skips both worktree and branch removal', async () => {
      await store.getState().removeWorkspaceKeepBoth(childId)
      expect(store.getState().workspaces[childId]).toBeUndefined()
      expect(deps.git.removeWorktree).not.toHaveBeenCalled()
      expect(deps.git.deleteBranch).not.toHaveBeenCalled()
    })

    it('removeWorkspace resets active workspace when removing active', async () => {
      store.getState().setActiveWorkspace(childId)
      await store.getState().removeWorkspace(childId)
      expect(store.getState().activeWorkspaceId).toBeNull()
    })

    it('removeOrphanWorkspace removes without git cleanup', () => {
      store.getState().removeOrphanWorkspace(childId)
      expect(store.getState().workspaces[childId]).toBeUndefined()
      expect(deps.git.removeWorktree).not.toHaveBeenCalled()
    })

    it('removeOrphanWorkspace does nothing for non-existent workspace', () => {
      store.getState().removeOrphanWorkspace('bad-id')
      // Should not throw
    })

    it('removeOrphanWorkspace removes workspace from state', () => {
      store.getState().removeOrphanWorkspace(childId)
      expect(store.getState().workspaces[childId]).toBeUndefined()
    })
  })

  describe('git info', () => {
    it('updateGitInfo updates workspace git fields', async () => {
      const id = await store.getState().addWorkspace('/test')
      store.getState().updateGitInfo(id, { isRepo: true, branch: 'develop', rootPath: '/test' })
      const ws = store.getState().workspaces[id]
      expect(ws.gitBranch).toBe('develop')
    })

    it('updateGitInfo does nothing for non-existent workspace', () => {
      store.getState().updateGitInfo('bad', { isRepo: false, branch: null, rootPath: null })
      // Should not throw
    })

    it('refreshGitInfo re-queries git info', async () => {
      const id = await store.getState().addWorkspace('/test')
      vi.mocked(deps.git.getInfo).mockResolvedValue({ isRepo: true, branch: 'feature', rootPath: '/test' })
      await store.getState().refreshGitInfo(id)
      expect(store.getState().workspaces[id].gitBranch).toBe('feature')
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
      const childWs = Object.values(store.getState().workspaces).find((ws) => ws.name === 'child')!
      childId = childWs.id
    })

    it('mergeAndRemoveWorkspace merges, removes, and cleans up load state', async () => {
      const result = await store.getState().mergeAndRemoveWorkspace(childId, false)
      expect(result).toEqual({ success: true })
      expect(deps.git.merge).toHaveBeenCalled()
      expect(store.getState().workspaces[childId]).toBeUndefined()
      expect(store.getState().workspaceLoadStates[childId]).toBeUndefined()
    })

    it('mergeAndRemoveWorkspace auto-commits uncommitted changes', async () => {
      vi.mocked(deps.git.hasUncommittedChanges).mockResolvedValue(true)
      await store.getState().mergeAndRemoveWorkspace(childId, false)
      expect(deps.git.commitAll).toHaveBeenCalled()
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

    it('closeAndCleanWorkspace removes worktree', async () => {
      const result = await store.getState().closeAndCleanWorkspace(childId)
      expect(result).toEqual({ success: true })
      expect(store.getState().workspaces[childId]).toBeUndefined()
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
      vi.mocked(deps.git.getInfo).mockResolvedValue({ isRepo: false, branch: null, rootPath: null })
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

    it('deletes session when no workspaces', async () => {
      await store.getState().syncToDaemon()
      expect(deps.sessionApi.delete).toHaveBeenCalledWith('session-1')
    })
  })

  describe('getDefaultAppForWorktree', () => {
    it('uses worktree settings defaultApplicationId when set', async () => {
      const app = makeFakeApp({ id: 'custom-app' })
      vi.mocked(deps.appRegistry.get).mockReturnValue(app)

      const id = await store.getState().addWorkspace('/test', {
        settings: { defaultApplicationId: 'custom-app' },
      })
      const ws = store.getState().workspaces[id]
      const tab = Object.values(ws.appStates)[0]
      expect(tab.applicationId).toBe('custom-app')
    })

    it('falls back to global default app', async () => {
      const app = makeFakeApp({ id: 'global-default' })
      vi.mocked(deps.appRegistry.get).mockReturnValue(undefined)
      vi.mocked(deps.appRegistry.getDefaultApp).mockReturnValue(app)

      const id = await store.getState().addWorkspace('/test')
      const ws = store.getState().workspaces[id]
      const tab = Object.values(ws.appStates)[0]
      expect(tab.applicationId).toBe('global-default')
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

  describe('session restore', () => {
    it('handleRestore restores workspaces from daemon session', async () => {
      const daemonSession = {
        id: 'session-1',
        workspaces: [
          makeWorkspace({ id: 'ws-restored', name: 'restored', path: '/restored', appStates: {}, activeTabId: null }),
        ],
        createdAt: Date.now(),
        lastActivity: Date.now(),
      }

      await store.getState().handleRestore(daemonSession)

      expect(store.getState().workspaces['ws-restored']).toBeDefined()
      expect(store.getState().workspaces['ws-restored'].name).toBe('restored')
      expect(store.getState().isRestoring).toBe(false)
    })

    it('handleExternalUpdate applies external changes', async () => {
      // First add a workspace
      store.getState().addWorkspace('/existing')
      await flushPromises()
      const existingId = Object.keys(store.getState().workspaces)[0]

      const daemonSession = {
        id: 'session-1',
        workspaces: [
          makeWorkspace({ id: 'ws-new', name: 'new-workspace', path: '/new' }),
        ],
        createdAt: Date.now(),
        lastActivity: Date.now(),
      }

      await store.getState().handleExternalUpdate(daemonSession)

      // Old workspace should be removed (not in daemon session)
      expect(store.getState().workspaces[existingId]).toBeUndefined()
      // New workspace should be added
      expect(store.getState().workspaces['ws-new']).toBeDefined()
      expect(store.getState().isRestoring).toBe(false)
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
      }

      await store.getState().handleRestore(daemonSession)

      expect(store.getState().workspaces['ws-parent']).toBeDefined()
      expect(store.getState().workspaces['ws-child']).toBeDefined()
      expect(store.getState().workspaces['ws-child'].parentId).toBe('ws-parent')
    })
  })
})
