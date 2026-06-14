/* eslint-disable custom/no-string-literal-comparison -- test fixtures use string literals intentionally; domain types are already enum-backed */
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { createSessionStore, WorkspaceEntryStatus } from './createSessionStore'
import type { SessionDeps, SessionState } from './createSessionStore'
import type { LlmApi, Application, GitInfo } from '../types'
import { ConnectionStatus, ConnectionTargetType, type ConnectionInfo } from '../../shared/types'
import type { StoreApi } from 'zustand'
import { createMockExecApi } from '../../shared/mockApis'
import { makeWorkspace, makeSession } from '../../shared/test-fixtures/workspace'
import { FileWatchEventType, type FileWatchEvent } from '../../shared/ipc-types'
import { toStoredWorkspaceFile, type Workspace } from '../../shared/workspaceFile'
import { sha256Hex } from '../lib/sha256'

const flushPromises = () => new Promise(r => setTimeout(r, 0))

// Captures the onEvent callback for each watched workspace file so tests can
// drive file-watch events (the daemon's WatchFile stream is mocked).
const fileWatchCallbacks = new Map<string, (e: FileWatchEvent) => void>()

// Build a daemon Session carrying only the membership refs for these workspaces.
function sessionWithRefs(workspaces: Workspace[], version = 1): ReturnType<typeof makeSession> {
  return makeSession({
    workspaceRefs: workspaces.map(w => ({ id: w.id, path: w.path })),
    version,
  })
}

// Emit a Present file-watch event delivering a workspace body. The on-disk envelope
// carries a parentHash; tests that don't care pass '' and an arbitrary sha (the store
// dedups by sha, not by re-hashing the delivered content).
function emitFilePresent(ws: Workspace, sha?: string, parentHash = ''): void {
  const cb = fileWatchCallbacks.get(`${ws.id}.json`)
  if (!cb) throw new Error(`no file watch registered for ${ws.id}`)
  cb({ type: FileWatchEventType.Present, content: JSON.stringify(toStoredWorkspaceFile(ws, parentHash)), sha256: sha ?? `sha-${ws.id}-${String(Math.random())}` })
}

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
      isAncestor: vi.fn().mockResolvedValue(false),
    },
    filesystem: {
      readDirectory: vi.fn().mockResolvedValue({ success: true }),
      readFile: vi.fn().mockResolvedValue({ success: true }),
      writeFile: vi.fn().mockResolvedValue({ success: true }),
      deleteFile: vi.fn().mockResolvedValue({ success: true }),
      searchFiles: vi.fn().mockResolvedValue({ success: true }),
      watchFile: vi.fn((_workspacePath: string, filePath: string, onEvent: (e: FileWatchEvent) => void) => {
        fileWatchCallbacks.set(filePath, onEvent)
        return { unsubscribe: () => { fileWatchCallbacks.delete(filePath) } }
      }),
    },
    runActions: {
      detect: vi.fn().mockResolvedValue([]),
      run: vi.fn().mockResolvedValue(null),
    },
    exec: createMockExecApi(),
    sessionApi: {
      // Echo the sent refs back at version+1 so the accept path is exercised by default.
      update: vi.fn().mockImplementation((_sid: string, refs: { id: string; path: string }[], _sender?: string, expectedVersion?: number) =>
        Promise.resolve({ success: true, session: makeSession({ workspaceRefs: refs, version: (expectedVersion ?? 0) + 1 }) })),
      lock: vi.fn().mockResolvedValue({ success: true, acquired: true, session: makeSession() }),
      unlock: vi.fn().mockResolvedValue({ success: true, session: makeSession() }),
      forceUnlock: vi.fn().mockResolvedValue({ success: true, session: makeSession() }),
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
    worktreeRegistry: {
      list: vi.fn().mockResolvedValue([]),
      upsert: vi.fn().mockResolvedValue(undefined),
      remove: vi.fn().mockResolvedValue(undefined),
    },
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
    fileWatchCallbacks.clear()
    deps = makeDeps()
    const localConn: ConnectionInfo = { id: 'local', target: { type: ConnectionTargetType.Local }, status: ConnectionStatus.Connected }
    store = createSessionStore({ sessionId: 'session-1', connection: localConn }, deps)
    // The daemon advertises this via SessionWatch; set it directly for tests that
    // exercise create/remove without first replaying a session event.
    store.setState({ workspaceDataDir: '/test/.treeterm/workspaces' })
  })

  describe('initial state', () => {
    it('has correct session id', () => {
      expect(store.getState().sessionId).toBe('session-1')
    })

    it('has empty workspace collection', () => {
      expect(store.getState().workspaces).toEqual(new Map())
    })

    it('has null active workspace', () => {
      expect(store.getState().activeWorkspaceId).toBeUndefined()
    })

    it('is not restoring', () => {
      expect(store.getState().isRestoring).toBe(false)
    })

    it('preserves connection from config', () => {
      const conn: ConnectionInfo = { id: 'conn-1', target: { type: ConnectionTargetType.Local }, status: ConnectionStatus.Connected }
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
      store.getState().setActiveWorkspace(undefined)
      expect(store.getState().activeWorkspaceId).toBeUndefined()
      store.getState().setActiveWorkspace(id)
      expect(store.getState().activeWorkspaceId).toBe(id)
    })

    it('setActiveWorkspace triggers git refresh on new active workspace', async () => {
      const id = store.getState().addWorkspace('/test')
      await flushPromises()

      const entry = store.getState().workspaces.get(id)
      expect(entry?.status).toBe(WorkspaceEntryStatus.Loaded)
      if (entry?.status !== WorkspaceEntryStatus.Loaded) return

      const refreshGit = vi.spyOn(entry.store.getState().gitController.getState(), 'refreshGit')

      store.getState().setActiveWorkspace(undefined)
      store.getState().setActiveWorkspace(id)
      expect(refreshGit).toHaveBeenCalled()
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

    it('autoOpenWorktrees loads a batch under the root, nesting by parentPath', async () => {
      const result = await store.getState().autoOpenWorktrees(parentId, [
        { path: '/repo/.worktrees/feat', branch: 'feat', name: 'feat', parentPath: null },
        { path: '/repo/.worktrees/sub', branch: 'sub', name: 'sub', parentPath: '/repo/.worktrees/feat' },
      ])
      expect(result).toEqual({ success: true })

      const workspaces = store.getState().workspaces
      const byPath = new Map(
        Array.from(workspaces.values())
          .filter((e): e is Extract<typeof e, { status: WorkspaceEntryStatus.Loaded }> => e.status === WorkspaceEntryStatus.Loaded)
          .map(e => [e.data.path, e.data])
      )
      const feat = byPath.get('/repo/.worktrees/feat')
      const sub = byPath.get('/repo/.worktrees/sub')
      expect(feat?.parentId).toBe(parentId)
      // child nests under the newly-created feat workspace, not the root
      expect(sub?.parentId).toBe(feat?.id)
    })

    it('autoOpenWorktrees skips already-open worktrees', async () => {
      await store.getState().adoptExistingWorktree(parentId, '/repo/.worktrees/dup', 'dup', 'dup')
      const before = store.getState().workspaces.size
      const result = await store.getState().autoOpenWorktrees(parentId, [
        { path: '/repo/.worktrees/dup', branch: 'dup', name: 'dup', parentPath: null },
        { path: '/repo/.worktrees/fresh', branch: 'fresh', name: 'fresh', parentPath: null },
      ])
      expect(result).toEqual({ success: true })
      // only the fresh worktree was added; dup was skipped
      expect(store.getState().workspaces.size).toBe(before + 1)
    })

    it('autoOpenWorktrees falls back to root for an item with an unknown parent', async () => {
      const result = await store.getState().autoOpenWorktrees(parentId, [
        { path: '/repo/.worktrees/orphan', branch: 'orphan', name: 'orphan', parentPath: '/repo/.worktrees/missing' },
      ])
      expect(result).toEqual({ success: true })
      const orphan = Array.from(store.getState().workspaces.values())
        .filter((e): e is Extract<typeof e, { status: WorkspaceEntryStatus.Loaded }> => e.status === WorkspaceEntryStatus.Loaded)
        .find(e => e.data.path === '/repo/.worktrees/orphan')
      expect(orphan?.data.parentId).toBe(parentId)
    })

    it('autoOpenWorktrees fails when root workspace not found', async () => {
      const result = await store.getState().autoOpenWorktrees('nonexistent', [])
      expect(result).toEqual({ success: false, error: 'Root workspace not found' })
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
      expect(store.getState().activeWorkspaceId).toBeUndefined()
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
      await store.getState().syncToDaemon('test')
      expect(deps.sessionApi.update).toHaveBeenCalled()
    })

  })

  describe('sessionVersion reconciliation', () => {
    it('sets sessionVersion from the daemon response after a sync', async () => {
      vi.mocked(deps.sessionApi.update).mockResolvedValue({
        success: true,
        session: makeSession(),
      })
      store.getState().addWorkspace('/test')
      await flushPromises()
      await store.getState().syncToDaemon('test')
      expect(store.getState().sessionVersion).toBe(1)
    })

    it('reconciles to daemon version when UpdateSession is rejected', async () => {
      // First sync settles sessionVersion to 1 (mock returns makeSession version=1).
      vi.mocked(deps.sessionApi.update).mockResolvedValue({
        success: true,
        session: makeSession(),
      })
      store.getState().addWorkspace('/test')
      await flushPromises()
      await store.getState().syncToDaemon('first')
      expect(store.getState().sessionVersion).toBe(1)

      // Second sync: daemon reports a version jump (another client moved the session on).
      vi.mocked(deps.sessionApi.update).mockResolvedValue({
        success: true,
        session: makeSession({ version: 5 }),
      })
      store.getState().addWorkspace('/test2')
      await flushPromises()
      await store.getState().syncToDaemon('rejected')
      expect(store.getState().sessionVersion).toBe(5)
    })

    it('skips external updates where daemon version is behind local', async () => {
      store.setState({ sessionVersion: 5 })

      const ext = makeWorkspace({ id: 'ws-ext', name: 'external', path: '/external' })
      await store.getState().handleExternalUpdate(sessionWithRefs([ext], 3))

      expect(store.getState().workspaces.has('ws-ext')).toBe(false)
    })

    it('applies external update when daemon version is ahead of local', async () => {
      store.setState({ sessionVersion: 0 })

      const ext = makeWorkspace({ id: 'ws-ext', name: 'external', path: '/external' })
      await store.getState().handleExternalUpdate(sessionWithRefs([ext], 1))
      // The ref creates a Loading placeholder; the file watch delivers the body.
      emitFilePresent(ext)

      const found = Array.from(store.getState().workspaces.values()).some(
        e => (e.status === WorkspaceEntryStatus.Loaded || e.status === WorkspaceEntryStatus.OperationError) && e.data.path === '/external'
      )
      expect(found).toBe(true)
    })

    it('applies same-version external update when membership differs', async () => {
      const ws1 = makeWorkspace({ id: 'ws-1', name: 'original', path: '/original' })
      await store.getState().handleExternalUpdate(sessionWithRefs([ws1], 3))
      emitFilePresent(ws1)
      expect(store.getState().sessionVersion).toBe(3)

      const ws2 = makeWorkspace({ id: 'ws-2', name: 'changed', path: '/changed' })
      await store.getState().handleExternalUpdate(sessionWithRefs([ws2], 3))
      emitFilePresent(ws2)

      const found = Array.from(store.getState().workspaces.values()).some(
        e => (e.status === WorkspaceEntryStatus.Loaded || e.status === WorkspaceEntryStatus.OperationError) && e.data.path === '/changed'
      )
      expect(found).toBe(true)
      expect(store.getState().workspaces.has('ws-1')).toBe(false)
    })

    it('treats an unchanged ref list as a no-op', async () => {
      const ws = makeWorkspace({ id: 'ws-1', name: 'same', path: '/same' })
      const session = sessionWithRefs([ws], 3)
      await store.getState().handleExternalUpdate(session)
      await store.getState().handleExternalUpdate(session)
      expect(store.getState().isRestoring).toBe(false)
    })

    it('applies external update when daemon version is strictly ahead', async () => {
      store.setState({ sessionVersion: 3 })

      const ext = makeWorkspace({ id: 'ws-ext', name: 'external', path: '/external' })
      await store.getState().handleExternalUpdate(sessionWithRefs([ext], 5))
      emitFilePresent(ext)

      const found = Array.from(store.getState().workspaces.values()).some(
        e => (e.status === WorkspaceEntryStatus.Loaded || e.status === WorkspaceEntryStatus.OperationError) && e.data.path === '/external'
      )
      expect(found).toBe(true)
    })

    it('serializes concurrent syncToDaemon calls — at most one UpdateSession RPC in flight', async () => {
      // Regression test for the original ptyId/connectionId divergence bug:
      // the old design let releaseLock's UnlockSession race with an in-flight
      // UpdateSession, so when the daemon processed UnlockSession first the
      // racing UpdateSession was rejected but the response coincidentally
      // satisfied `v === expected + 1` — the client read that as accepted.
      // With a single queue, daemon-version-bumping RPCs never overlap on the
      // wire, so that coincidence cannot arise.
      let active = 0
      let maxActive = 0
      vi.mocked(deps.sessionApi.update).mockImplementation(async () => {
        active++
        maxActive = Math.max(maxActive, active)
        await new Promise(r => setTimeout(r, 5))
        active--
        return { success: true, session: makeSession() }
      })

      store.getState().addWorkspace('/a')
      await flushPromises()

      await Promise.all([
        store.getState().syncToDaemon('s1'),
        store.getState().syncToDaemon('s2'),
        store.getState().syncToDaemon('s3'),
      ])

      expect(maxActive).toBe(1)
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
      await store.getState().syncToDaemon('test')
      // Should not throw
    })

    it('handles sync exception gracefully', async () => {
      vi.mocked(deps.sessionApi.update).mockRejectedValue(new Error('network error'))
      store.getState().addWorkspace('/test')
      await flushPromises()
      await store.getState().syncToDaemon('test')
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
      const storeState = entry.store.getState()
      return { ...entry.data, metadata: storeState.metadata, appStates: storeState.appStates }
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
      expect(getLoadedData(id)?.parentId).toBeUndefined()
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
      expect(getLoadedData(child)?.parentId).toBeUndefined()

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
      expect(getLoadedData(parent)?.parentId).toBeUndefined()
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
      vi.mocked(deps.sessionApi.lock).mockResolvedValue({ success: true, acquired: false, session: makeSession() } as never)

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
      await store.getState().handleRestore(makeSession({ lock: { acquiredAt: Date.now(), expiresAt: Date.now() + 60000 } }))
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
      await store.getState().handleRestore(makeSession({ lock: { acquiredAt: Date.now(), expiresAt: Date.now() + 60000 } }))
      vi.mocked(deps.sessionApi.forceUnlock).mockResolvedValue({ success: false, error: 'failed' } as never)
      const result = await store.getState().forceUnlock()
      expect(result.success).toBe(false)
    })
  })

  describe('session restore', () => {
    it('handleRestore creates a placeholder per ref, then loads each from its file', async () => {
      const ws = makeWorkspace({ id: 'ws-restored', name: 'restored', path: '/restored', appStates: {} })
      await store.getState().handleRestore(sessionWithRefs([ws], 1))

      // Placeholder exists immediately (Loading); body arrives via the watch.
      expect(store.getState().workspaces.get('ws-restored')).toBeDefined()
      emitFilePresent(ws)

      const entry = store.getState().workspaces.get('ws-restored')!
      expect(entry.status).toBe(WorkspaceEntryStatus.Loaded)
      expect((entry as Extract<typeof entry, { status: WorkspaceEntryStatus.Loaded }>).data.name).toBe('restored')
      expect(store.getState().isRestoring).toBe(false)
    })

    it('handleExternalUpdate removes refs gone from the daemon and adds new ones', async () => {
      store.getState().addWorkspace('/existing')
      await flushPromises()
      const existingId = Array.from(store.getState().workspaces.keys())[0]

      const ws = makeWorkspace({ id: 'ws-new', name: 'new-workspace', path: '/new' })
      await store.getState().handleExternalUpdate(sessionWithRefs([ws], 2))

      // Old workspace (not in the new ref list) is removed.
      expect(store.getState().workspaces.get(existingId!)).toBeUndefined()
      // New ref placeholder is present and loads from its file.
      expect(store.getState().workspaces.get('ws-new')).toBeDefined()
      emitFilePresent(ws)
      expect(store.getState().workspaces.get('ws-new')!.status).toBe(WorkspaceEntryStatus.Loaded)
      expect(store.getState().isRestoring).toBe(false)
    })

    it('applies a content file event — removes old tabs and adds new tabs', async () => {
      const withTab1 = makeWorkspace({ id: 'ws-tabs', name: 'tabs-test', path: '/tabs', appStates: { 'tab-1': { applicationId: 'terminal', title: 'Terminal', state: {} } }, activeTabId: 'tab-1' })
      await store.getState().handleRestore(sessionWithRefs([withTab1], 1))
      emitFilePresent(withTab1)
      expect(store.getState().workspaces.get('ws-tabs')!.status).toBe(WorkspaceEntryStatus.Loaded)

      // A later content edit (same ref) arrives purely via the file watch.
      const withTab2 = makeWorkspace({ id: 'ws-tabs', name: 'tabs-test', path: '/tabs', appStates: { 'tab-2': { applicationId: 'terminal', title: 'New Tab', state: {} } }, activeTabId: 'tab-2' })
      emitFilePresent(withTab2)

      const entry = store.getState().workspaces.get('ws-tabs')!
      expect(entry.status).toBe(WorkspaceEntryStatus.Loaded)
      const wsState = (entry as Extract<typeof entry, { status: WorkspaceEntryStatus.Loaded }>).store.getState().workspace
      expect(wsState.appStates['tab-2']).toBeDefined()
      expect(wsState.appStates['tab-1']).toBeUndefined()
    })

    it('suppresses the watch echo of our own write that arrives before lastSeenSha advances', async () => {
      const ws = makeWorkspace({ id: 'ws-echo', name: 'echo', path: '/echo', appStates: { 'tab-1': { applicationId: 'terminal', title: 'Terminal', state: {} } }, activeTabId: 'tab-1' })
      await store.getState().handleRestore(sessionWithRefs([ws], 1))
      emitFilePresent(ws)
      const entry = store.getState().workspaces.get('ws-echo')!
      expect(entry.status).toBe(WorkspaceEntryStatus.Loaded)
      const handle = (entry as Extract<typeof entry, { status: WorkspaceEntryStatus.Loaded }>).store

      // Simulate the daemon emitting the watch echo *during* writeFile, before it
      // resolves: lastSeenSha has not advanced yet, so the body is suppressed only by
      // sha match against the recent-hashes ring (recorded up front, before the write).
      // Without the fix this re-applies our own body, replacing the workspace object and
      // tearing down a just-mounted terminal.
      let echoFired = false
      vi.mocked(deps.filesystem.writeFile).mockImplementation(async (_wp: string, filePath: string, content: string) => {
        const cb = fileWatchCallbacks.get(filePath)
        if (cb) { echoFired = true; cb({ type: FileWatchEventType.Present, content, sha256: await sha256Hex(content) }) }
        return { success: true }
      })

      handle.getState().updateMetadata('displayName', 'Renamed', 'test')
      const refAfterMutation = handle.getState().workspace
      // Several flushes: the echo is emitted from inside the writeFile mock after an
      // async sha256 (crypto.subtle) hop, so it lands across a few microtask turns.
      for (let i = 0; i < 5; i++) await flushPromises()
      expect(deps.filesystem.writeFile).toHaveBeenCalled()
      expect(echoFired).toBe(true)

      // Echo suppressed: applyWorkspaceFile never ran, so the object reference is intact.
      expect(handle.getState().workspace).toBe(refAfterMutation)
      expect(handle.getState().workspace.metadata.displayName).toBe('Renamed')
    })

    it('still applies a genuine external edit whose content we never wrote', async () => {
      const ws = makeWorkspace({ id: 'ws-ext', name: 'ext', path: '/ext', appStates: { 'tab-1': { applicationId: 'terminal', title: 'Terminal', state: {} } }, activeTabId: 'tab-1' })
      await store.getState().handleRestore(sessionWithRefs([ws], 1))
      emitFilePresent(ws)
      const entry = store.getState().workspaces.get('ws-ext')!
      expect(entry.status).toBe(WorkspaceEntryStatus.Loaded)
      const handle = (entry as Extract<typeof entry, { status: WorkspaceEntryStatus.Loaded }>).store

      // Another window edits the file: content we never wrote ourselves.
      const edited = makeWorkspace({ id: 'ws-ext', name: 'ext', path: '/ext', appStates: { 'tab-2': { applicationId: 'terminal', title: 'External', state: {} } }, activeTabId: 'tab-2' })
      emitFilePresent(edited, 'sha-external-never-written')

      expect(handle.getState().workspace.appStates['tab-2']).toBeDefined()
      expect(handle.getState().workspace.appStates['tab-1']).toBeUndefined()
    })

    // Capture every body the store writes, in order.
    function captureWrites(): string[] {
      const writes: string[] = []
      vi.mocked(deps.filesystem.writeFile).mockImplementation((_wp: string, _fp: string, content: string) => {
        writes.push(content)
        return Promise.resolve({ success: true })
      })
      return writes
    }

    // Resolve a restored workspace's loaded store handle (fails loudly if not loaded).
    function loadedHandle(id: string) {
      const entry = store.getState().workspaces.get(id)!
      expect(entry.status).toBe(WorkspaceEntryStatus.Loaded)
      return (entry as Extract<typeof entry, { status: WorkspaceEntryStatus.Loaded }>).store
    }

    it('chains each write\'s parentHash to the previous body\'s sha', async () => {
      const ws = makeWorkspace({ id: 'ws-chain', name: 'chain', path: '/chain' })
      await store.getState().handleRestore(sessionWithRefs([ws], 1))
      emitFilePresent(ws, 'sha-initial')
      const handle = loadedHandle('ws-chain')

      const writes = captureWrites()
      handle.getState().updateMetadata('note', 'one', 'test')
      for (let i = 0; i < 5; i++) await flushPromises()
      handle.getState().updateMetadata('note', 'two', 'test')
      for (let i = 0; i < 5; i++) await flushPromises()

      expect(writes.length).toBeGreaterThanOrEqual(2)
      // First write supersedes the body delivered by the watch (sha-initial).
      expect((JSON.parse(writes[0]!) as { parentHash: string }).parentHash).toBe('sha-initial')
      // Second write's parent is the first body's own sha — the chain links forward.
      expect((JSON.parse(writes[1]!) as { parentHash: string }).parentHash).toBe(await sha256Hex(writes[0]!))
    })

    it('produces a distinct sha even when the logical content reverts to an earlier state', async () => {
      const ws = makeWorkspace({ id: 'ws-revert', name: 'revert', path: '/revert' })
      await store.getState().handleRestore(sessionWithRefs([ws], 1))
      emitFilePresent(ws, 'sha-initial')
      const handle = loadedHandle('ws-revert')

      const writes = captureWrites()
      // Set a metadata key, then delete it — the logical body returns to its initial
      // state, but the chained parentHash makes each written body hash to a new value.
      handle.getState().updateMetadata('note', 'temp', 'test')
      for (let i = 0; i < 5; i++) await flushPromises()
      handle.getState().deleteMetadata('note', 'test')
      for (let i = 0; i < 5; i++) await flushPromises()

      expect(writes.length).toBeGreaterThanOrEqual(2)
      const shas = await Promise.all(writes.map(w => sha256Hex(w)))
      expect(new Set(shas).size).toBe(shas.length) // all distinct — no repeats
    })

    it('suppresses an echo of an older self-write still within the 32-hash ring', async () => {
      const ws = makeWorkspace({ id: 'ws-ring', name: 'ring', path: '/ring' })
      await store.getState().handleRestore(sessionWithRefs([ws], 1))
      emitFilePresent(ws, 'sha-initial')
      const handle = loadedHandle('ws-ring')

      // Make several writes; capture the body + sha of an early one.
      const writes = captureWrites()
      for (let n = 0; n < 5; n++) {
        handle.getState().updateMetadata('note', `v${String(n)}`, 'test')
        for (let i = 0; i < 5; i++) await flushPromises()
      }
      const earlyBody = writes[0]!
      const earlySha = await sha256Hex(earlyBody)
      const refBefore = handle.getState().workspace

      // A late, out-of-order echo of that early write arrives. Its sha is still in the
      // ring, so it must be suppressed (no re-apply, object reference preserved).
      const cb = fileWatchCallbacks.get('ws-ring.json')!
      cb({ type: FileWatchEventType.Present, content: earlyBody, sha256: earlySha })
      for (let i = 0; i < 3; i++) await flushPromises()

      expect(handle.getState().workspace).toBe(refBefore)
    })

    it('reconstructs child workspaces with parent relationship from their files', async () => {
      const parent = makeWorkspace({ id: 'ws-parent', name: 'parent', path: '/parent' })
      const child = makeWorkspace({ id: 'ws-child', name: 'child', path: '/child', parentId: 'ws-parent', isWorktree: true })
      await store.getState().handleRestore(sessionWithRefs([parent, child], 1))
      emitFilePresent(parent)
      emitFilePresent(child)

      expect(store.getState().workspaces.get('ws-parent')!.status).toBe(WorkspaceEntryStatus.Loaded)
      const childEntry = store.getState().workspaces.get('ws-child')!
      expect(childEntry.status).toBe(WorkspaceEntryStatus.Loaded)
      expect((childEntry as Extract<typeof childEntry, { status: WorkspaceEntryStatus.Loaded }>).data.parentId).toBe('ws-parent')
    })

    it('surfaces an invalid workspace file as an error entry', async () => {
      const ws = makeWorkspace({ id: 'ws-bad', name: 'bad', path: '/bad' })
      await store.getState().handleRestore(sessionWithRefs([ws], 1))
      const cb = fileWatchCallbacks.get('ws-bad.json')!
      cb({ type: FileWatchEventType.Present, content: '{ not valid json', sha256: 'badsha' })

      expect(store.getState().workspaces.get('ws-bad')!.status).toBe(WorkspaceEntryStatus.Error)
    })

    it('writes the JSON body and publishes a ref when creating a workspace', async () => {
      store.getState().addWorkspace('/created')
      await flushPromises()

      // The JSON file is written (CAS must-not-exist) and the ref list is published.
      expect(deps.filesystem.writeFile).toHaveBeenCalled()
      const writeArgs = vi.mocked(deps.filesystem.writeFile).mock.calls[0]!
      expect(writeArgs[3]).toBe('') // expectedSha256 '' → must-not-exist
      expect(deps.sessionApi.update).toHaveBeenCalled()
    })

    it('deletes the JSON file when a workspace is removed', async () => {
      store.getState().addWorkspace('/doomed')
      await flushPromises()
      const id = Array.from(store.getState().workspaces.keys())[0]!

      await store.getState().removeWorkspace(id)
      await flushPromises()

      expect(deps.filesystem.deleteFile).toHaveBeenCalledWith('/test/.treeterm/workspaces', `${id}.json`)
    })
  })
})
