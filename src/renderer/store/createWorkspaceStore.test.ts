import { describe, it, expect, vi, beforeEach } from 'vitest'
import { createWorkspaceStore, getUnmergedSubWorkspaces } from './createWorkspaceStore'
import type { WorkspaceDeps } from './createWorkspaceStore'
import type { Workspace, Application } from '../types'

function makeDeps(overrides?: Partial<WorkspaceDeps>): WorkspaceDeps {
  return {
    git: {
      getInfo: vi.fn().mockResolvedValue({ isRepo: true, branch: 'main', rootPath: '/repo' }),
      createWorktree: vi.fn().mockResolvedValue({ success: true }),
      createWorktreeFromBranch: vi.fn().mockResolvedValue({ success: true }),
      createWorktreeFromRemote: vi.fn().mockResolvedValue({ success: true }),
      removeWorktree: vi.fn().mockResolvedValue({ success: true }),
      hasUncommittedChanges: vi.fn().mockResolvedValue(false),
      commitAll: vi.fn().mockResolvedValue({ success: true }),
      merge: vi.fn().mockResolvedValue({ success: true }),
      listWorktrees: vi.fn().mockResolvedValue([]),
      getChildWorktrees: vi.fn().mockResolvedValue([]),
      listLocalBranches: vi.fn().mockResolvedValue([]),
      listRemoteBranches: vi.fn().mockResolvedValue([]),
      getBranchesInWorktrees: vi.fn().mockResolvedValue([]),
      getDiff: vi.fn().mockResolvedValue({ success: true }),
      getFileDiff: vi.fn().mockResolvedValue({ success: true }),
      checkMergeConflicts: vi.fn().mockResolvedValue({ success: true }),
      deleteBranch: vi.fn().mockResolvedValue({ success: true }),
      getUncommittedChanges: vi.fn().mockResolvedValue({ success: true }),
      getUncommittedFileDiff: vi.fn().mockResolvedValue({ success: true }),
      stageFile: vi.fn().mockResolvedValue({ success: true }),
      unstageFile: vi.fn().mockResolvedValue({ success: true }),
      stageAll: vi.fn().mockResolvedValue({ success: true }),
      unstageAll: vi.fn().mockResolvedValue({ success: true }),
      commitStaged: vi.fn().mockResolvedValue({ success: true }),
      getFileContentsForDiff: vi.fn().mockResolvedValue({ success: true }),
      getUncommittedFileContentsForDiff: vi.fn().mockResolvedValue({ success: true }),
      getHeadCommitHash: vi.fn().mockResolvedValue({ success: true }),
    },
    session: {
      create: vi.fn().mockResolvedValue({ success: true }),
      update: vi.fn().mockResolvedValue({ success: true }),
      list: vi.fn().mockResolvedValue({ success: true, sessions: [] }),
      get: vi.fn().mockResolvedValue({ success: true }),
      delete: vi.fn().mockResolvedValue({ success: true }),
      openInNewWindow: vi.fn().mockResolvedValue({ success: true }),
      onShowSessions: vi.fn().mockReturnValue(() => {}),
      onSync: vi.fn().mockReturnValue(() => {}),
    },
    getSettings: () => ({
      terminal: { fontSize: 14, fontFamily: 'monospace', cursorStyle: 'block', cursorBlink: false, showRawChars: false, startByDefault: true, instances: [] },
      sandbox: { enabledByDefault: false, allowNetworkByDefault: false },
      aiHarness: { instances: [] },
      appearance: { theme: 'dark' },
      prefixMode: { enabled: false, prefixKey: 'Control+B', timeout: 1500 },
      keybindings: { newTab: 'c', closeTab: 'x', nextTab: 'n', prevTab: 'p', openSettings: ',', workspaceFocus: 'w' },
      stt: { enabled: false, provider: 'openaiWhisper', openaiApiKey: '', localWhisperModelPath: '', pushToTalkKey: '', language: 'en' },
      daemon: { orphanTimeout: 30000, scrollbackLimit: 10000, killOnQuit: true },
      globalDefaultApplicationId: '',
      recentDirectories: [],
    }),
    appRegistry: {
      get: vi.fn().mockReturnValue(null),
      getDefaultApp: vi.fn().mockReturnValue(null),
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
    children: [],
    status: 'active',
    isGitRepo: false,
    gitBranch: null,
    gitRootPath: null,
    isWorktree: false,
    tabs: [],
    activeTabId: null,
    metadata: {},
    createdAt: Date.now(),
    lastActivity: Date.now(),
    attachedClients: 0,
    ...overrides
  }
}

function makeFakeApp(overrides: Partial<Application> = {}): Application {
  return {
    id: 'terminal',
    name: 'Terminal',
    icon: 'terminal',
    createInitialState: () => ({ ptyId: null }),
    canClose: true,
    canHaveMultiple: true,
    showInNewTabMenu: true,
    keepAlive: false,
    displayStyle: 'block',
    isDefault: false,
    render: () => null,
    ...overrides,
  }
}

describe('getUnmergedSubWorkspaces', () => {
  it('returns empty array when no workspaces', () => {
    expect(getUnmergedSubWorkspaces({})).toEqual([])
  })

  it('returns only active worktree workspaces', () => {
    const workspaces: Record<string, Workspace> = {
      root: makeWorkspace({ id: 'root', isWorktree: false, status: 'active' }),
      child1: makeWorkspace({ id: 'child1', isWorktree: true, status: 'active' }),
      child2: makeWorkspace({ id: 'child2', isWorktree: true, status: 'merged' }),
      child3: makeWorkspace({ id: 'child3', isWorktree: true, status: 'active' })
    }
    const result = getUnmergedSubWorkspaces(workspaces)
    expect(result).toHaveLength(2)
    expect(result.map(w => w.id)).toContain('child1')
    expect(result.map(w => w.id)).toContain('child3')
  })

  it('excludes non-worktree workspaces even if active', () => {
    const workspaces: Record<string, Workspace> = {
      root: makeWorkspace({ id: 'root', isWorktree: false, status: 'active' })
    }
    expect(getUnmergedSubWorkspaces(workspaces)).toEqual([])
  })
})

describe('createWorkspaceStore', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('creates a store with initial state', () => {
    const store = createWorkspaceStore({ sessionId: 'session-1', windowUuid: 'uuid-1' }, makeDeps())
    const state = store.getState()

    expect(state.workspaces).toEqual({})
    expect(state.activeWorkspaceId).toBeNull()
    expect(state.isRestoring).toBe(false)
  })

  it('exposes all required action methods', () => {
    const store = createWorkspaceStore({ sessionId: 'session-1', windowUuid: null }, makeDeps())
    const state = store.getState()

    expect(typeof state.addWorkspace).toBe('function')
    expect(typeof state.addChildWorkspace).toBe('function')
    expect(typeof state.removeWorkspace).toBe('function')
    expect(typeof state.setActiveWorkspace).toBe('function')
    expect(typeof state.addTab).toBe('function')
    expect(typeof state.addTabWithState).toBe('function')
    expect(typeof state.removeTab).toBe('function')
    expect(typeof state.setActiveTab).toBe('function')
    expect(typeof state.updateTabTitle).toBe('function')
    expect(typeof state.updateTabState).toBe('function')
    expect(typeof state.syncToDaemon).toBe('function')
  })

  it('setActiveWorkspace updates activeWorkspaceId', () => {
    const store = createWorkspaceStore({ sessionId: 'session-1', windowUuid: null }, makeDeps())
    store.setState({
      workspaces: { 'ws-1': makeWorkspace({ id: 'ws-1' }) }
    })
    store.getState().setActiveWorkspace('ws-1')
    expect(store.getState().activeWorkspaceId).toBe('ws-1')
  })

  it('setActiveWorkspace can set to null', () => {
    const store = createWorkspaceStore({ sessionId: 'session-1', windowUuid: null }, makeDeps())
    store.setState({ activeWorkspaceId: 'ws-1' })
    store.getState().setActiveWorkspace(null)
    expect(store.getState().activeWorkspaceId).toBeNull()
  })

  it('isRestoring can be set externally via setState', () => {
    const store = createWorkspaceStore({ sessionId: 'session-1', windowUuid: null }, makeDeps())
    expect(store.getState().isRestoring).toBe(false)
    store.setState({ isRestoring: true })
    expect(store.getState().isRestoring).toBe(true)
  })

  it('updateGitInfo updates workspace git fields', () => {
    const store = createWorkspaceStore({ sessionId: 'session-1', windowUuid: null }, makeDeps())
    store.setState({
      workspaces: { 'ws-1': makeWorkspace({ id: 'ws-1', isGitRepo: false, gitBranch: null, gitRootPath: null }) }
    })
    store.getState().updateGitInfo('ws-1', { isRepo: true, branch: 'feature', rootPath: '/repo' })
    const ws = store.getState().workspaces['ws-1']
    expect(ws.isGitRepo).toBe(true)
    expect(ws.gitBranch).toBe('feature')
    expect(ws.gitRootPath).toBe('/repo')
  })

  it('updateGitInfo is a no-op for unknown workspace', () => {
    const store = createWorkspaceStore({ sessionId: 'session-1', windowUuid: null }, makeDeps())
    // Should not throw
    store.getState().updateGitInfo('nonexistent', { isRepo: true, branch: 'main', rootPath: '/r' })
    expect(store.getState().workspaces).toEqual({})
  })

  it('updateWorkspaceStatus updates status field', () => {
    const store = createWorkspaceStore({ sessionId: 'session-1', windowUuid: null }, makeDeps())
    store.setState({
      workspaces: { 'ws-1': makeWorkspace({ id: 'ws-1', status: 'active' }) }
    })
    store.getState().updateWorkspaceStatus('ws-1', 'merged')
    expect(store.getState().workspaces['ws-1'].status).toBe('merged')
  })

  it('updateTabTitle updates the tab title', () => {
    const store = createWorkspaceStore({ sessionId: 'session-1', windowUuid: null }, makeDeps())
    store.setState({
      workspaces: {
        'ws-1': makeWorkspace({
          id: 'ws-1',
          tabs: [{ id: 'tab-1', applicationId: 'terminal', title: 'Terminal 1', state: { ptyId: null } }],
          activeTabId: 'tab-1'
        })
      }
    })
    store.getState().updateTabTitle('ws-1', 'tab-1', 'my shell')
    expect(store.getState().workspaces['ws-1'].tabs[0].title).toBe('my shell')
  })

  it('setActiveTab updates the workspace activeTabId', () => {
    const store = createWorkspaceStore({ sessionId: 'session-1', windowUuid: null }, makeDeps())
    store.setState({
      workspaces: {
        'ws-1': makeWorkspace({
          id: 'ws-1',
          tabs: [
            { id: 'tab-1', applicationId: 'terminal', title: 'T1', state: {} },
            { id: 'tab-2', applicationId: 'terminal', title: 'T2', state: {} }
          ],
          activeTabId: 'tab-1'
        })
      }
    })
    store.getState().setActiveTab('ws-1', 'tab-2')
    expect(store.getState().workspaces['ws-1'].activeTabId).toBe('tab-2')
  })

  it('each store instance has its own debounce timer (no cross-instance leakage)', () => {
    const store1 = createWorkspaceStore({ sessionId: 'session-1', windowUuid: null }, makeDeps())
    const store2 = createWorkspaceStore({ sessionId: 'session-2', windowUuid: null }, makeDeps())
    // Both start with clean state — they are independent instances
    expect(store1.getState().workspaces).toEqual({})
    expect(store2.getState().workspaces).toEqual({})
    store1.setState({ activeWorkspaceId: 'ws-1' })
    expect(store2.getState().activeWorkspaceId).toBeNull()
  })

  describe('addWorkspace', () => {
    it('creates workspace with name extracted from path and git info from deps', async () => {
      const deps = makeDeps()
      const store = createWorkspaceStore({ sessionId: 's1', windowUuid: null }, deps)

      const id = await store.getState().addWorkspace('/home/user/my-project')

      const ws = store.getState().workspaces[id]
      expect(ws).toBeDefined()
      expect(ws.name).toBe('my-project')
      expect(ws.path).toBe('/home/user/my-project')
      expect(ws.isGitRepo).toBe(true)
      expect(ws.gitBranch).toBe('main')
      expect(ws.gitRootPath).toBe('/repo')
      expect(ws.status).toBe('active')
      expect(ws.isWorktree).toBe(false)
      expect(ws.parentId).toBeNull()
      expect(store.getState().activeWorkspaceId).toBe(id)
    })

    it('creates default tab when app registry returns a default app', async () => {
      const app = makeFakeApp()
      const deps = makeDeps({
        appRegistry: {
          get: vi.fn().mockReturnValue(null),
          getDefaultApp: vi.fn().mockReturnValue(app),
        },
      })
      const store = createWorkspaceStore({ sessionId: 's1', windowUuid: null }, deps)

      const id = await store.getState().addWorkspace('/project')

      const ws = store.getState().workspaces[id]
      expect(ws.tabs).toHaveLength(1)
      expect(ws.tabs[0].applicationId).toBe('terminal')
      expect(ws.tabs[0].title).toBe('Terminal')
      expect(ws.activeTabId).toBe(ws.tabs[0].id)
    })

    it('skips default tabs when skipDefaultTabs is true', async () => {
      const app = makeFakeApp()
      const deps = makeDeps({
        appRegistry: {
          get: vi.fn().mockReturnValue(null),
          getDefaultApp: vi.fn().mockReturnValue(app),
        },
      })
      const store = createWorkspaceStore({ sessionId: 's1', windowUuid: null }, deps)

      const id = await store.getState().addWorkspace('/project', { skipDefaultTabs: true })

      const ws = store.getState().workspaces[id]
      expect(ws.tabs).toHaveLength(0)
      expect(ws.activeTabId).toBeNull()
    })

    it('calls session.update to sync to daemon', async () => {
      const deps = makeDeps()
      const store = createWorkspaceStore({ sessionId: 's1', windowUuid: 'uuid-1' }, deps)

      await store.getState().addWorkspace('/project')

      expect(deps.session.update).toHaveBeenCalledWith(
        's1',
        expect.arrayContaining([expect.objectContaining({ path: '/project' })]),
        'uuid-1'
      )
    })
  })

  describe('addChildWorkspace', () => {
    it('returns error when parent not found', async () => {
      const store = createWorkspaceStore({ sessionId: 's1', windowUuid: null }, makeDeps())

      const result = await store.getState().addChildWorkspace('nonexistent', 'child')
      expect(result).toEqual({ success: false, error: 'Parent workspace not found' })
    })

    it('returns error when parent is not a git repo', async () => {
      const store = createWorkspaceStore({ sessionId: 's1', windowUuid: null }, makeDeps())
      store.setState({
        workspaces: { 'ws-1': makeWorkspace({ id: 'ws-1', isGitRepo: false, gitRootPath: null }) }
      })

      const result = await store.getState().addChildWorkspace('ws-1', 'child')
      expect(result).toEqual({ success: false, error: 'Parent workspace is not a git repository' })
    })

    it('creates child workspace linked to parent and sets it active', async () => {
      const deps = makeDeps({
        git: {
          ...makeDeps().git,
          getInfo: vi.fn().mockResolvedValue({ isRepo: true, branch: 'main', rootPath: '/repo' }),
          createWorktree: vi.fn().mockResolvedValue({ success: true, path: '/repo/.worktrees/child', branch: 'child' }),
        },
      })
      const store = createWorkspaceStore({ sessionId: 's1', windowUuid: null }, deps)
      store.setState({
        workspaces: { 'parent': makeWorkspace({ id: 'parent', isGitRepo: true, gitRootPath: '/repo', gitBranch: 'main', path: '/repo' }) }
      })

      const result = await store.getState().addChildWorkspace('parent', 'child')
      expect(result.success).toBe(true)

      const state = store.getState()
      const parent = state.workspaces['parent']
      expect(parent.children).toHaveLength(1)

      const childId = parent.children[0]
      const child = state.workspaces[childId]
      expect(child.name).toBe('child')
      expect(child.parentId).toBe('parent')
      expect(child.isWorktree).toBe(true)
      expect(child.isGitRepo).toBe(true)
      expect(child.gitBranch).toBe('child')
      expect(state.activeWorkspaceId).toBe(childId)
    })

    it('calls git.createWorktree with correct args', async () => {
      const deps = makeDeps({
        git: {
          ...makeDeps().git,
          getInfo: vi.fn().mockResolvedValue({ isRepo: true, branch: 'main', rootPath: '/repo' }),
          createWorktree: vi.fn().mockResolvedValue({ success: true, path: '/repo/.worktrees/child', branch: 'child' }),
        },
      })
      const store = createWorkspaceStore({ sessionId: 's1', windowUuid: null }, deps)
      store.setState({
        workspaces: { 'parent': makeWorkspace({ id: 'parent', isGitRepo: true, gitRootPath: '/repo', gitBranch: 'main', path: '/repo' }) }
      })

      await store.getState().addChildWorkspace('parent', 'child')

      expect(deps.git.createWorktree).toHaveBeenCalledWith('/repo', 'child', 'main')
    })
  })

  describe('adoptExistingWorktree', () => {
    it('returns error when parent not found', async () => {
      const store = createWorkspaceStore({ sessionId: 's1', windowUuid: null }, makeDeps())

      const result = await store.getState().adoptExistingWorktree('nonexistent', '/wt', 'branch', 'name')
      expect(result).toEqual({ success: false, error: 'Parent workspace not found' })
    })

    it('returns error when worktree path is already open', async () => {
      const store = createWorkspaceStore({ sessionId: 's1', windowUuid: null }, makeDeps())
      store.setState({
        workspaces: {
          'parent': makeWorkspace({ id: 'parent', isGitRepo: true, gitRootPath: '/repo' }),
          'existing': makeWorkspace({ id: 'existing', path: '/wt-path' }),
        }
      })

      const result = await store.getState().adoptExistingWorktree('parent', '/wt-path', 'feat', 'name')
      expect(result).toEqual({ success: false, error: 'This worktree is already open' })
    })

    it('creates child workspace without calling git.createWorktree', async () => {
      const deps = makeDeps()
      const store = createWorkspaceStore({ sessionId: 's1', windowUuid: null }, deps)
      store.setState({
        workspaces: { 'parent': makeWorkspace({ id: 'parent', isGitRepo: true, gitRootPath: '/repo' }) }
      })

      const result = await store.getState().adoptExistingWorktree('parent', '/wt', 'feat', 'feat-ws')
      expect(result.success).toBe(true)
      expect(deps.git.createWorktree).not.toHaveBeenCalled()

      const parent = store.getState().workspaces['parent']
      expect(parent.children).toHaveLength(1)
      const child = store.getState().workspaces[parent.children[0]]
      expect(child.path).toBe('/wt')
      expect(child.gitBranch).toBe('feat')
      expect(child.name).toBe('feat-ws')
    })
  })

  describe('createWorktreeFromBranch', () => {
    it('returns error when parent not found', async () => {
      const store = createWorkspaceStore({ sessionId: 's1', windowUuid: null }, makeDeps())

      const result = await store.getState().createWorktreeFromBranch('missing', 'feat', false)
      expect(result).toEqual({ success: false, error: 'Parent workspace not found' })
    })

    it('returns error when parent is not a git repo', async () => {
      const store = createWorkspaceStore({ sessionId: 's1', windowUuid: null }, makeDeps())
      store.setState({
        workspaces: { 'parent': makeWorkspace({ id: 'parent', isGitRepo: false }) }
      })

      const result = await store.getState().createWorktreeFromBranch('parent', 'feat', false)
      expect(result).toEqual({ success: false, error: 'Parent workspace is not a git repository' })
    })

    it('calls git.createWorktreeFromBranch and creates child workspace', async () => {
      const deps = makeDeps({
        git: {
          ...makeDeps().git,
          createWorktreeFromBranch: vi.fn().mockResolvedValue({ success: true, path: '/repo/.worktrees/feat', branch: 'feat' }),
        },
      })
      const store = createWorkspaceStore({ sessionId: 's1', windowUuid: null }, deps)
      store.setState({
        workspaces: { 'parent': makeWorkspace({ id: 'parent', isGitRepo: true, gitRootPath: '/repo', gitBranch: 'main' }) }
      })

      const result = await store.getState().createWorktreeFromBranch('parent', 'feat', false)
      expect(result.success).toBe(true)
      expect(deps.git.createWorktreeFromBranch).toHaveBeenCalledWith('/repo', 'feat', 'feat')

      const parent = store.getState().workspaces['parent']
      expect(parent.children).toHaveLength(1)
    })
  })

  describe('createWorktreeFromRemote', () => {
    it('returns error when parent not found', async () => {
      const store = createWorkspaceStore({ sessionId: 's1', windowUuid: null }, makeDeps())

      const result = await store.getState().createWorktreeFromRemote('missing', 'origin/feat', false)
      expect(result).toEqual({ success: false, error: 'Parent workspace not found' })
    })

    it('returns error when parent is not a git repo', async () => {
      const store = createWorkspaceStore({ sessionId: 's1', windowUuid: null }, makeDeps())
      store.setState({
        workspaces: { 'parent': makeWorkspace({ id: 'parent', isGitRepo: false }) }
      })

      const result = await store.getState().createWorktreeFromRemote('parent', 'origin/feat', false)
      expect(result).toEqual({ success: false, error: 'Parent workspace is not a git repository' })
    })

    it('calls git.createWorktreeFromRemote and creates child workspace', async () => {
      const deps = makeDeps({
        git: {
          ...makeDeps().git,
          createWorktreeFromRemote: vi.fn().mockResolvedValue({ success: true, path: '/repo/.worktrees/feat', branch: 'feat' }),
        },
      })
      const store = createWorkspaceStore({ sessionId: 's1', windowUuid: null }, deps)
      store.setState({
        workspaces: { 'parent': makeWorkspace({ id: 'parent', isGitRepo: true, gitRootPath: '/repo', gitBranch: 'main' }) }
      })

      const result = await store.getState().createWorktreeFromRemote('parent', 'origin/feat', false)
      expect(result.success).toBe(true)
      expect(deps.git.createWorktreeFromRemote).toHaveBeenCalledWith('/repo', 'origin/feat', 'feat')

      const parent = store.getState().workspaces['parent']
      expect(parent.children).toHaveLength(1)
    })
  })

  describe('removeWorkspace', () => {
    it('removes workspace from state and resets activeWorkspaceId if it was active', async () => {
      const deps = makeDeps()
      const store = createWorkspaceStore({ sessionId: 's1', windowUuid: null }, deps)
      store.setState({
        workspaces: { 'ws-1': makeWorkspace({ id: 'ws-1' }) },
        activeWorkspaceId: 'ws-1'
      })

      await store.getState().removeWorkspace('ws-1')

      expect(store.getState().workspaces['ws-1']).toBeUndefined()
      expect(store.getState().activeWorkspaceId).toBeNull()
    })

    it('removes child from parent children array', async () => {
      const deps = makeDeps()
      const store = createWorkspaceStore({ sessionId: 's1', windowUuid: null }, deps)
      store.setState({
        workspaces: {
          'parent': makeWorkspace({ id: 'parent', children: ['child-1'] }),
          'child-1': makeWorkspace({ id: 'child-1', parentId: 'parent' }),
        }
      })

      await store.getState().removeWorkspace('child-1')

      expect(store.getState().workspaces['parent'].children).toEqual([])
      expect(store.getState().workspaces['child-1']).toBeUndefined()
    })

    it('calls git.removeWorktree for worktree workspaces with deleteBranch', async () => {
      const deps = makeDeps()
      const store = createWorkspaceStore({ sessionId: 's1', windowUuid: null }, deps)
      store.setState({
        workspaces: {
          'parent': makeWorkspace({ id: 'parent', children: ['wt-1'] }),
          'wt-1': makeWorkspace({ id: 'wt-1', parentId: 'parent', isWorktree: true, gitRootPath: '/repo', path: '/repo/.worktrees/feat' }),
        }
      })

      await store.getState().removeWorkspace('wt-1')

      expect(deps.git.removeWorktree).toHaveBeenCalledWith('/repo', '/repo/.worktrees/feat', true)
    })

    it('calls app cleanup for each tab', async () => {
      const cleanup = vi.fn()
      const app = makeFakeApp({ cleanup })
      const deps = makeDeps({
        appRegistry: {
          get: vi.fn().mockReturnValue(app),
          getDefaultApp: vi.fn().mockReturnValue(null),
        },
      })
      const store = createWorkspaceStore({ sessionId: 's1', windowUuid: null }, deps)
      const ws = makeWorkspace({
        id: 'ws-1',
        tabs: [
          { id: 'tab-1', applicationId: 'terminal', title: 'T1', state: {} },
          { id: 'tab-2', applicationId: 'terminal', title: 'T2', state: {} },
        ],
      })
      store.setState({ workspaces: { 'ws-1': ws } })

      await store.getState().removeWorkspace('ws-1')

      expect(cleanup).toHaveBeenCalledTimes(2)
    })

    it('recursively removes children', async () => {
      const deps = makeDeps()
      const store = createWorkspaceStore({ sessionId: 's1', windowUuid: null }, deps)
      store.setState({
        workspaces: {
          'parent': makeWorkspace({ id: 'parent', children: ['child'] }),
          'child': makeWorkspace({ id: 'child', parentId: 'parent', children: ['grandchild'], isWorktree: true, gitRootPath: '/repo', path: '/repo/.wt/child' }),
          'grandchild': makeWorkspace({ id: 'grandchild', parentId: 'child', isWorktree: true, gitRootPath: '/repo', path: '/repo/.wt/gc' }),
        }
      })

      await store.getState().removeWorkspace('parent')

      expect(store.getState().workspaces).toEqual({})
    })
  })

  describe('removeWorkspaceKeepBranch', () => {
    it('does NOT delete branch (deleteBranch=false)', async () => {
      const deps = makeDeps()
      const store = createWorkspaceStore({ sessionId: 's1', windowUuid: null }, deps)
      store.setState({
        workspaces: {
          'parent': makeWorkspace({ id: 'parent', children: ['wt-1'] }),
          'wt-1': makeWorkspace({ id: 'wt-1', parentId: 'parent', isWorktree: true, gitRootPath: '/repo', path: '/repo/.wt/feat' }),
        }
      })

      await store.getState().removeWorkspaceKeepBranch('wt-1')

      expect(deps.git.removeWorktree).toHaveBeenCalledWith('/repo', '/repo/.wt/feat', false)
    })
  })

  describe('removeWorkspaceKeepWorktree', () => {
    it('does NOT call removeWorktree but deletes branch', async () => {
      const deps = makeDeps()
      const store = createWorkspaceStore({ sessionId: 's1', windowUuid: null }, deps)
      store.setState({
        workspaces: {
          'parent': makeWorkspace({ id: 'parent', children: ['wt-1'] }),
          'wt-1': makeWorkspace({ id: 'wt-1', parentId: 'parent', isWorktree: true, gitRootPath: '/repo', path: '/repo/.wt/feat', gitBranch: 'feat' }),
        }
      })

      await store.getState().removeWorkspaceKeepWorktree('wt-1')

      expect(deps.git.removeWorktree).not.toHaveBeenCalled()
      expect(deps.git.deleteBranch).toHaveBeenCalledWith('/repo', 'feat')
      expect(store.getState().workspaces['wt-1']).toBeUndefined()
    })
  })

  describe('removeWorkspaceKeepBoth', () => {
    it('does NOT call removeWorktree or deleteBranch', async () => {
      const deps = makeDeps()
      const store = createWorkspaceStore({ sessionId: 's1', windowUuid: null }, deps)
      store.setState({
        workspaces: {
          'parent': makeWorkspace({ id: 'parent', children: ['wt-1'] }),
          'wt-1': makeWorkspace({ id: 'wt-1', parentId: 'parent', isWorktree: true, gitRootPath: '/repo', path: '/repo/.wt/feat', gitBranch: 'feat' }),
        }
      })

      await store.getState().removeWorkspaceKeepBoth('wt-1')

      expect(deps.git.removeWorktree).not.toHaveBeenCalled()
      expect(deps.git.deleteBranch).not.toHaveBeenCalled()
      expect(store.getState().workspaces['wt-1']).toBeUndefined()
    })
  })

  describe('removeOrphanWorkspace', () => {
    it('removes workspace from local state without any git operations or daemon sync', () => {
      const deps = makeDeps()
      const store = createWorkspaceStore({ sessionId: 's1', windowUuid: null }, deps)
      store.setState({
        workspaces: {
          'parent': makeWorkspace({ id: 'parent', children: ['wt-1'] }),
          'wt-1': makeWorkspace({ id: 'wt-1', parentId: 'parent', isWorktree: true, gitRootPath: '/repo', path: '/repo/.wt/feat', gitBranch: 'feat' }),
        }
      })

      store.getState().removeOrphanWorkspace('wt-1')

      expect(deps.git.removeWorktree).not.toHaveBeenCalled()
      expect(deps.git.deleteBranch).not.toHaveBeenCalled()
      expect(store.getState().workspaces['wt-1']).toBeUndefined()
      expect(store.getState().workspaces['parent'].children).toEqual([])
    })

    it('is a no-op for non-existent workspace', () => {
      const deps = makeDeps()
      const store = createWorkspaceStore({ sessionId: 's1', windowUuid: null }, deps)

      store.getState().removeOrphanWorkspace('non-existent')

      expect(deps.git.removeWorktree).not.toHaveBeenCalled()
      expect(deps.git.deleteBranch).not.toHaveBeenCalled()
    })
  })

  describe('mergeAndRemoveWorkspace', () => {
    function setupMergeScenario(depsOverrides?: Partial<WorkspaceDeps>) {
      const deps = makeDeps(depsOverrides)
      const store = createWorkspaceStore({ sessionId: 's1', windowUuid: null }, deps)
      store.setState({
        workspaces: {
          'parent': makeWorkspace({ id: 'parent', isGitRepo: true, gitRootPath: '/repo', gitBranch: 'main', children: ['wt-1'] }),
          'wt-1': makeWorkspace({ id: 'wt-1', parentId: 'parent', isWorktree: true, isGitRepo: true, gitRootPath: '/repo', gitBranch: 'feat', path: '/repo/.wt/feat' }),
        }
      })
      return { store, deps }
    }

    it('returns error for non-worktree workspace', async () => {
      const store = createWorkspaceStore({ sessionId: 's1', windowUuid: null }, makeDeps())
      store.setState({
        workspaces: { 'ws-1': makeWorkspace({ id: 'ws-1', isWorktree: false }) }
      })

      const result = await store.getState().mergeAndRemoveWorkspace('ws-1', false)
      expect(result).toEqual({ success: false, error: 'Not a worktree workspace' })
    })

    it('returns error for missing workspace', async () => {
      const store = createWorkspaceStore({ sessionId: 's1', windowUuid: null }, makeDeps())

      const result = await store.getState().mergeAndRemoveWorkspace('missing', false)
      expect(result).toEqual({ success: false, error: 'Workspace not found' })
    })

    it('auto-commits uncommitted changes before merge', async () => {
      const { store, deps } = setupMergeScenario({
        git: {
          ...makeDeps().git,
          hasUncommittedChanges: vi.fn().mockResolvedValue(true),
          commitAll: vi.fn().mockResolvedValue({ success: true }),
          merge: vi.fn().mockResolvedValue({ success: true }),
          removeWorktree: vi.fn().mockResolvedValue({ success: true }),
        },
      })

      await store.getState().mergeAndRemoveWorkspace('wt-1', false)

      expect(deps.git.commitAll).toHaveBeenCalledWith(
        '/repo/.wt/feat',
        expect.stringContaining('Auto-commit before merge')
      )
    })

    it('calls git.merge with correct args', async () => {
      const { store, deps } = setupMergeScenario({
        git: {
          ...makeDeps().git,
          merge: vi.fn().mockResolvedValue({ success: true }),
          removeWorktree: vi.fn().mockResolvedValue({ success: true }),
        },
      })

      await store.getState().mergeAndRemoveWorkspace('wt-1', true)

      expect(deps.git.merge).toHaveBeenCalledWith('/repo', 'feat', 'main', true)
    })

    it('updates status to merged then removes workspace', async () => {
      const { store } = setupMergeScenario({
        git: {
          ...makeDeps().git,
          merge: vi.fn().mockResolvedValue({ success: true }),
          removeWorktree: vi.fn().mockResolvedValue({ success: true }),
        },
      })

      const result = await store.getState().mergeAndRemoveWorkspace('wt-1', false)

      expect(result.success).toBe(true)
      // Workspace should be removed after merge
      expect(store.getState().workspaces['wt-1']).toBeUndefined()
    })

    it('returns error if merge fails', async () => {
      const { store } = setupMergeScenario({
        git: {
          ...makeDeps().git,
          merge: vi.fn().mockResolvedValue({ success: false, error: 'conflict' }),
        },
      })

      const result = await store.getState().mergeAndRemoveWorkspace('wt-1', false)

      expect(result.success).toBe(false)
      expect(result.error).toContain('conflict')
      // Workspace should NOT be removed on failure
      expect(store.getState().workspaces['wt-1']).toBeDefined()
    })
  })

  describe('closeAndCleanWorkspace', () => {
    it('returns error for non-worktree workspace', async () => {
      const store = createWorkspaceStore({ sessionId: 's1', windowUuid: null }, makeDeps())
      store.setState({
        workspaces: { 'ws-1': makeWorkspace({ id: 'ws-1', isWorktree: false }) }
      })

      const result = await store.getState().closeAndCleanWorkspace('ws-1')
      expect(result).toEqual({ success: false, error: 'Not a worktree workspace' })
    })

    it('returns error for missing workspace', async () => {
      const store = createWorkspaceStore({ sessionId: 's1', windowUuid: null }, makeDeps())

      const result = await store.getState().closeAndCleanWorkspace('missing')
      expect(result).toEqual({ success: false, error: 'Workspace not found' })
    })

    it('removes workspace on success', async () => {
      const deps = makeDeps()
      const store = createWorkspaceStore({ sessionId: 's1', windowUuid: null }, deps)
      store.setState({
        workspaces: {
          'parent': makeWorkspace({ id: 'parent', isGitRepo: true, gitRootPath: '/repo', children: ['wt-1'] }),
          'wt-1': makeWorkspace({ id: 'wt-1', parentId: 'parent', isWorktree: true, gitRootPath: '/repo', path: '/repo/.wt/feat' }),
        }
      })

      const result = await store.getState().closeAndCleanWorkspace('wt-1')

      expect(result.success).toBe(true)
      expect(store.getState().workspaces['wt-1']).toBeUndefined()
    })
  })

  describe('addTab', () => {
    it('adds tab with correct title numbering', () => {
      const app = makeFakeApp()
      const deps = makeDeps({
        appRegistry: {
          get: vi.fn().mockReturnValue(app),
          getDefaultApp: vi.fn().mockReturnValue(null),
        },
      })
      const store = createWorkspaceStore({ sessionId: 's1', windowUuid: null }, deps)
      store.setState({
        workspaces: {
          'ws-1': makeWorkspace({
            id: 'ws-1',
            tabs: [{ id: 'tab-existing', applicationId: 'terminal', title: 'Terminal 1', state: {} }],
          })
        }
      })

      const tabId = store.getState().addTab('ws-1', 'terminal')

      const ws = store.getState().workspaces['ws-1']
      const newTab = ws.tabs.find(t => t.id === tabId)
      expect(newTab).toBeDefined()
      expect(newTab!.title).toBe('Terminal 2')
      expect(ws.activeTabId).toBe(tabId)
    })

    it('respects canHaveMultiple: false — no-ops if tab of same app exists', () => {
      const app = makeFakeApp({ canHaveMultiple: false })
      const deps = makeDeps({
        appRegistry: {
          get: vi.fn().mockReturnValue(app),
          getDefaultApp: vi.fn().mockReturnValue(null),
        },
      })
      const store = createWorkspaceStore({ sessionId: 's1', windowUuid: null }, deps)
      store.setState({
        workspaces: {
          'ws-1': makeWorkspace({
            id: 'ws-1',
            tabs: [{ id: 'tab-1', applicationId: 'terminal', title: 'Terminal 1', state: {} }],
          })
        }
      })

      store.getState().addTab('ws-1', 'terminal')

      const ws = store.getState().workspaces['ws-1']
      expect(ws.tabs).toHaveLength(1)
    })

    it('returns tabId even when app not found (no-op on state)', () => {
      const deps = makeDeps()
      const store = createWorkspaceStore({ sessionId: 's1', windowUuid: null }, deps)
      store.setState({
        workspaces: { 'ws-1': makeWorkspace({ id: 'ws-1' }) }
      })

      const tabId = store.getState().addTab('ws-1', 'nonexistent')

      expect(typeof tabId).toBe('string')
      expect(store.getState().workspaces['ws-1'].tabs).toHaveLength(0)
    })
  })

  describe('addTabWithState', () => {
    it('creates new tab with merged initial + provided state', () => {
      const app = makeFakeApp({ createInitialState: () => ({ ptyId: null, cwd: '/default' }) })
      const deps = makeDeps({
        appRegistry: {
          get: vi.fn().mockReturnValue(app),
          getDefaultApp: vi.fn().mockReturnValue(null),
        },
      })
      const store = createWorkspaceStore({ sessionId: 's1', windowUuid: null }, deps)
      store.setState({
        workspaces: { 'ws-1': makeWorkspace({ id: 'ws-1' }) }
      })

      const tabId = store.getState().addTabWithState('ws-1', 'terminal', { cwd: '/custom' })

      const ws = store.getState().workspaces['ws-1']
      const tab = ws.tabs.find(t => t.id === tabId)
      expect(tab).toBeDefined()
      expect(tab!.state).toEqual({ ptyId: null, cwd: '/custom' })
    })

    it('updates existing tab state when existingTabId matches', () => {
      const app = makeFakeApp()
      const deps = makeDeps({
        appRegistry: {
          get: vi.fn().mockReturnValue(app),
          getDefaultApp: vi.fn().mockReturnValue(null),
        },
      })
      const store = createWorkspaceStore({ sessionId: 's1', windowUuid: null }, deps)
      store.setState({
        workspaces: {
          'ws-1': makeWorkspace({
            id: 'ws-1',
            tabs: [{ id: 'tab-1', applicationId: 'terminal', title: 'T1', state: { ptyId: 'p1' } }],
          })
        }
      })

      const tabId = store.getState().addTabWithState('ws-1', 'terminal', { extra: 'data' }, 'tab-1')

      expect(tabId).toBe('tab-1')
      const ws = store.getState().workspaces['ws-1']
      expect(ws.tabs).toHaveLength(1)
      expect(ws.tabs[0].state).toEqual({ ptyId: 'p1', extra: 'data' })
      expect(ws.activeTabId).toBe('tab-1')
    })

    it('respects canHaveMultiple: false — merges state into existing tab', () => {
      const app = makeFakeApp({ canHaveMultiple: false })
      const deps = makeDeps({
        appRegistry: {
          get: vi.fn().mockReturnValue(app),
          getDefaultApp: vi.fn().mockReturnValue(null),
        },
      })
      const store = createWorkspaceStore({ sessionId: 's1', windowUuid: null }, deps)
      store.setState({
        workspaces: {
          'ws-1': makeWorkspace({
            id: 'ws-1',
            tabs: [{ id: 'tab-1', applicationId: 'terminal', title: 'T1', state: { ptyId: 'p1' } }],
          })
        }
      })

      store.getState().addTabWithState('ws-1', 'terminal', { newField: 'val' })

      const ws = store.getState().workspaces['ws-1']
      expect(ws.tabs).toHaveLength(1)
      expect(ws.tabs[0].state).toEqual({ ptyId: 'p1', newField: 'val' })
      expect(ws.activeTabId).toBe('tab-1')
    })
  })

  describe('removeTab', () => {
    it('removes tab and adjusts activeTabId', async () => {
      const app = makeFakeApp({ canClose: true })
      const deps = makeDeps({
        appRegistry: {
          get: vi.fn().mockReturnValue(app),
          getDefaultApp: vi.fn().mockReturnValue(null),
        },
      })
      const store = createWorkspaceStore({ sessionId: 's1', windowUuid: null }, deps)
      store.setState({
        workspaces: {
          'ws-1': makeWorkspace({
            id: 'ws-1',
            tabs: [
              { id: 'tab-1', applicationId: 'terminal', title: 'T1', state: {} },
              { id: 'tab-2', applicationId: 'terminal', title: 'T2', state: {} },
            ],
            activeTabId: 'tab-1',
          })
        }
      })

      await store.getState().removeTab('ws-1', 'tab-1')

      const ws = store.getState().workspaces['ws-1']
      expect(ws.tabs).toHaveLength(1)
      expect(ws.tabs[0].id).toBe('tab-2')
      expect(ws.activeTabId).toBe('tab-2')
    })

    it('calls app cleanup', async () => {
      const cleanup = vi.fn()
      const app = makeFakeApp({ canClose: true, cleanup })
      const deps = makeDeps({
        appRegistry: {
          get: vi.fn().mockReturnValue(app),
          getDefaultApp: vi.fn().mockReturnValue(null),
        },
      })
      const store = createWorkspaceStore({ sessionId: 's1', windowUuid: null }, deps)
      const ws = makeWorkspace({
        id: 'ws-1',
        tabs: [{ id: 'tab-1', applicationId: 'terminal', title: 'T1', state: {} }],
        activeTabId: 'tab-1',
      })
      store.setState({ workspaces: { 'ws-1': ws } })

      await store.getState().removeTab('ws-1', 'tab-1')

      expect(cleanup).toHaveBeenCalledTimes(1)
    })

    it('no-ops when canClose is false', async () => {
      const app = makeFakeApp({ canClose: false })
      const deps = makeDeps({
        appRegistry: {
          get: vi.fn().mockReturnValue(app),
          getDefaultApp: vi.fn().mockReturnValue(null),
        },
      })
      const store = createWorkspaceStore({ sessionId: 's1', windowUuid: null }, deps)
      store.setState({
        workspaces: {
          'ws-1': makeWorkspace({
            id: 'ws-1',
            tabs: [{ id: 'tab-1', applicationId: 'terminal', title: 'T1', state: {} }],
          })
        }
      })

      await store.getState().removeTab('ws-1', 'tab-1')

      expect(store.getState().workspaces['ws-1'].tabs).toHaveLength(1)
    })

    it('no-ops for unknown workspace', async () => {
      const store = createWorkspaceStore({ sessionId: 's1', windowUuid: null }, makeDeps())

      // Should not throw
      await store.getState().removeTab('nonexistent', 'tab-1')
    })

    it('no-ops for unknown tab', async () => {
      const store = createWorkspaceStore({ sessionId: 's1', windowUuid: null }, makeDeps())
      store.setState({
        workspaces: { 'ws-1': makeWorkspace({ id: 'ws-1' }) }
      })

      // Should not throw
      await store.getState().removeTab('ws-1', 'nonexistent')
    })
  })

  describe('updateTabState', () => {
    it('applies updater function to tab state', () => {
      const deps = makeDeps()
      const store = createWorkspaceStore({ sessionId: 's1', windowUuid: null }, deps)
      store.setState({
        workspaces: {
          'ws-1': makeWorkspace({
            id: 'ws-1',
            tabs: [{ id: 'tab-1', applicationId: 'terminal', title: 'T1', state: { count: 0 } }],
          })
        }
      })

      store.getState().updateTabState<{ count: number }>('ws-1', 'tab-1', (s) => ({ ...s, count: s.count + 1 }))

      const tab = store.getState().workspaces['ws-1'].tabs[0]
      expect(tab.state).toEqual({ count: 1 })
    })

    it('syncs to daemon only when ptyId is present in updated state', () => {
      const deps = makeDeps()
      const store = createWorkspaceStore({ sessionId: 's1', windowUuid: null }, deps)
      store.setState({
        workspaces: {
          'ws-1': makeWorkspace({
            id: 'ws-1',
            tabs: [{ id: 'tab-1', applicationId: 'terminal', title: 'T1', state: { count: 0 } }],
          })
        }
      })

      // Update without ptyId — should NOT sync
      store.getState().updateTabState('ws-1', 'tab-1', (s: { count: number }) => ({ ...s, count: 1 }))
      expect(deps.session.update).not.toHaveBeenCalled()

      // Update WITH ptyId — should sync
      store.getState().updateTabState('ws-1', 'tab-1', (s: { count: number }) => ({ ...s, count: 2, ptyId: 'pty-1' }))
      // Give the fire-and-forget promise time to resolve
      expect(deps.session.update).toHaveBeenCalled()
    })
  })

  describe('refreshGitInfo', () => {
    it('calls git.getInfo and updates workspace git fields', async () => {
      const deps = makeDeps({
        git: {
          ...makeDeps().git,
          getInfo: vi.fn().mockResolvedValue({ isRepo: true, branch: 'develop', rootPath: '/new-repo' }),
        },
      })
      const store = createWorkspaceStore({ sessionId: 's1', windowUuid: null }, deps)
      store.setState({
        workspaces: { 'ws-1': makeWorkspace({ id: 'ws-1', path: '/project', isGitRepo: false }) }
      })

      await store.getState().refreshGitInfo('ws-1')

      expect(deps.git.getInfo).toHaveBeenCalledWith('/project')
      const ws = store.getState().workspaces['ws-1']
      expect(ws.isGitRepo).toBe(true)
      expect(ws.gitBranch).toBe('develop')
      expect(ws.gitRootPath).toBe('/new-repo')
    })

    it('no-ops for unknown workspace', async () => {
      const deps = makeDeps()
      const store = createWorkspaceStore({ sessionId: 's1', windowUuid: null }, deps)

      await store.getState().refreshGitInfo('nonexistent')

      expect(deps.git.getInfo).not.toHaveBeenCalled()
    })
  })

  describe('syncSessionToDaemon behavior', () => {
    it('skips sync when isRestoring is true', async () => {
      const deps = makeDeps()
      const store = createWorkspaceStore({ sessionId: 's1', windowUuid: null }, deps)
      store.setState({ isRestoring: true })

      await store.getState().syncToDaemon()

      expect(deps.session.update).not.toHaveBeenCalled()
      expect(deps.session.delete).not.toHaveBeenCalled()
    })

    it('deletes session when workspaces are empty', async () => {
      const deps = makeDeps()
      const store = createWorkspaceStore({ sessionId: 's1', windowUuid: null }, deps)
      // No workspaces set, default is empty

      await store.getState().syncToDaemon()

      expect(deps.session.delete).toHaveBeenCalledWith('s1')
      expect(deps.session.update).not.toHaveBeenCalled()
    })

    it('calls session.update with stripped workspaces (no createdAt/lastActivity/attachedClients)', async () => {
      const deps = makeDeps()
      const store = createWorkspaceStore({ sessionId: 's1', windowUuid: 'uuid-1' }, deps)
      store.setState({
        workspaces: {
          'ws-1': makeWorkspace({
            id: 'ws-1',
            name: 'test',
            path: '/test',
            createdAt: 123456,
            lastActivity: 789012,
            attachedClients: 3,
          })
        }
      })

      await store.getState().syncToDaemon()

      expect(deps.session.update).toHaveBeenCalledWith(
        's1',
        expect.arrayContaining([
          expect.not.objectContaining({ createdAt: expect.anything() })
        ]),
        'uuid-1'
      )
      // Verify the stripped workspace does NOT have these fields
      const calledWith = (deps.session.update as ReturnType<typeof vi.fn>).mock.calls[0][1]
      const ws = calledWith[0]
      expect(ws).not.toHaveProperty('createdAt')
      expect(ws).not.toHaveProperty('lastActivity')
      expect(ws).not.toHaveProperty('attachedClients')
    })
  })

  describe('review comments', () => {
    it('addReviewComment adds a comment to metadata', () => {
      const deps = makeDeps()
      const store = createWorkspaceStore({ sessionId: 's1', windowUuid: null }, deps)
      store.setState({
        workspaces: {
          'ws-1': makeWorkspace({ id: 'ws-1', metadata: {} })
        }
      })

      store.getState().addReviewComment('ws-1', {
        filePath: 'test.ts',
        lineNumber: 10,
        text: 'Fix this',
        commitHash: 'abc123',
        isOutdated: false,
        addressed: false,
        side: 'modified'
      })

      const ws = store.getState().workspaces['ws-1']
      const comments = JSON.parse(ws.metadata.reviewComments)
      expect(comments).toHaveLength(1)
      expect(comments[0].text).toBe('Fix this')
      expect(comments[0].filePath).toBe('test.ts')
      expect(comments[0].id).toBeDefined()
      expect(comments[0].createdAt).toBeDefined()
    })

    it('deleteReviewComment removes a comment', () => {
      const deps = makeDeps()
      const store = createWorkspaceStore({ sessionId: 's1', windowUuid: null }, deps)
      const existingComments = JSON.stringify([
        { id: 'c1', filePath: 'a.ts', lineNumber: 1, text: 'A', commitHash: 'h1', createdAt: 1, isOutdated: false, addressed: false, side: 'modified' },
        { id: 'c2', filePath: 'b.ts', lineNumber: 2, text: 'B', commitHash: 'h1', createdAt: 2, isOutdated: false, addressed: false, side: 'modified' }
      ])
      store.setState({
        workspaces: {
          'ws-1': makeWorkspace({ id: 'ws-1', metadata: { reviewComments: existingComments } })
        }
      })

      store.getState().deleteReviewComment('ws-1', 'c1')

      const ws = store.getState().workspaces['ws-1']
      const comments = JSON.parse(ws.metadata.reviewComments)
      expect(comments).toHaveLength(1)
      expect(comments[0].id).toBe('c2')
    })

    it('toggleReviewCommentAddressed toggles addressed flag', () => {
      const deps = makeDeps()
      const store = createWorkspaceStore({ sessionId: 's1', windowUuid: null }, deps)
      const existingComments = JSON.stringify([
        { id: 'c1', filePath: 'a.ts', lineNumber: 1, text: 'A', commitHash: 'h1', createdAt: 1, isOutdated: false, addressed: false, side: 'modified' }
      ])
      store.setState({
        workspaces: {
          'ws-1': makeWorkspace({ id: 'ws-1', metadata: { reviewComments: existingComments } })
        }
      })

      store.getState().toggleReviewCommentAddressed('ws-1', 'c1')

      const ws = store.getState().workspaces['ws-1']
      const comments = JSON.parse(ws.metadata.reviewComments)
      expect(comments[0].addressed).toBe(true)
    })

    it('updateOutdatedReviewComments marks comments with different hash as outdated', () => {
      const deps = makeDeps()
      const store = createWorkspaceStore({ sessionId: 's1', windowUuid: null }, deps)
      const existingComments = JSON.stringify([
        { id: 'c1', filePath: 'a.ts', lineNumber: 1, text: 'A', commitHash: 'old', createdAt: 1, isOutdated: false, addressed: false, side: 'modified' },
        { id: 'c2', filePath: 'b.ts', lineNumber: 2, text: 'B', commitHash: 'new', createdAt: 2, isOutdated: false, addressed: false, side: 'modified' }
      ])
      store.setState({
        workspaces: {
          'ws-1': makeWorkspace({ id: 'ws-1', metadata: { reviewComments: existingComments } })
        }
      })

      store.getState().updateOutdatedReviewComments('ws-1', 'new')

      const ws = store.getState().workspaces['ws-1']
      const comments = JSON.parse(ws.metadata.reviewComments)
      expect(comments[0].isOutdated).toBe(true)
      expect(comments[1].isOutdated).toBe(false)
    })

    it('clearReviewComments empties the comments', () => {
      const deps = makeDeps()
      const store = createWorkspaceStore({ sessionId: 's1', windowUuid: null }, deps)
      const existingComments = JSON.stringify([
        { id: 'c1', filePath: 'a.ts', lineNumber: 1, text: 'A', commitHash: 'h1', createdAt: 1, isOutdated: false, addressed: false, side: 'modified' }
      ])
      store.setState({
        workspaces: {
          'ws-1': makeWorkspace({ id: 'ws-1', metadata: { reviewComments: existingComments } })
        }
      })

      store.getState().clearReviewComments('ws-1')

      const ws = store.getState().workspaces['ws-1']
      const comments = JSON.parse(ws.metadata.reviewComments)
      expect(comments).toHaveLength(0)
    })
  })
})
