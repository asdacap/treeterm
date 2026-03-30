import { describe, it, expect, vi, beforeEach } from 'vitest'
import { createWorkspaceStore } from './createWorkspaceStore'
import type { WorkspaceStoreDeps } from './createWorkspaceStore'
import { getUnmergedSubWorkspaces } from './createSessionStore'
import type { WorkspaceEntry } from './createSessionStore'
import type { Workspace, Application } from '../types'

function makeHandleDeps(overrides?: Partial<WorkspaceStoreDeps>): WorkspaceStoreDeps {
  return {
    appRegistry: {
      get: vi.fn().mockReturnValue(null),
      getDefaultApp: vi.fn().mockReturnValue(null),
    },
    openTtyStream: vi.fn().mockResolvedValue({ tty: null }),
    getTtyWriter: vi.fn().mockResolvedValue({ write: vi.fn(), kill: vi.fn() }),
    createTty: vi.fn().mockResolvedValue('pty-1'),
    connectionId: 'local',
    git: {} as any,
    filesystem: {} as any,
    runActions: { detect: vi.fn().mockResolvedValue([]), run: vi.fn().mockResolvedValue(null) },
    syncToDaemon: vi.fn(),
    removeWorkspace: vi.fn().mockResolvedValue(undefined),
    removeWorkspaceKeepBranch: vi.fn().mockResolvedValue(undefined),
    removeWorkspaceKeepBoth: vi.fn().mockResolvedValue(undefined),
    mergeAndRemoveWorkspace: vi.fn().mockResolvedValue({ success: true }),
    mergeAndKeepWorkspace: vi.fn().mockResolvedValue({ success: true }),
    closeAndCleanWorkspace: vi.fn().mockResolvedValue({ success: true }),
    quickForkWorkspace: vi.fn().mockResolvedValue({ success: true }),
    refreshGitInfo: vi.fn().mockResolvedValue(undefined),
    lookupWorkspace: vi.fn().mockReturnValue(undefined),
    getSettings: vi.fn().mockReturnValue({
      llm: { apiKey: '', baseUrl: '' },
      terminalAnalyzer: { model: '', systemPrompt: '', titleSystemPrompt: '', reasoningEffort: 'off', safePaths: [], bufferLines: 10 },
    }),
    llm: {
      analyzeTerminal: vi.fn().mockResolvedValue({ state: 'idle', reason: '' }),
      generateTitle: vi.fn().mockResolvedValue({ title: '', description: '', branchName: '' }),
    },
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
    ...overrides
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

function toLoaded(ws: Workspace): WorkspaceEntry {
  return { status: 'loaded', data: ws, store: createWorkspaceStore(ws, makeHandleDeps()) }
}

describe('getUnmergedSubWorkspaces', () => {
  it('returns empty array when no workspaces', () => {
    expect(getUnmergedSubWorkspaces({})).toEqual([])
  })

  it('returns only active worktree workspaces', () => {
    const workspaces: Record<string, WorkspaceEntry> = {
      root: toLoaded(makeWorkspace({ id: 'root', isWorktree: false, status: 'active' })),
      child1: toLoaded(makeWorkspace({ id: 'child1', isWorktree: true, status: 'active' })),
      child2: toLoaded(makeWorkspace({ id: 'child2', isWorktree: true, status: 'merged' })),
      child3: toLoaded(makeWorkspace({ id: 'child3', isWorktree: true, status: 'active' }))
    }
    const result = getUnmergedSubWorkspaces(workspaces)
    expect(result).toHaveLength(2)
    expect(result.map(w => w.id)).toContain('child1')
    expect(result.map(w => w.id)).toContain('child3')
  })

  it('excludes non-worktree workspaces even if active', () => {
    const workspaces: Record<string, WorkspaceEntry> = {
      root: toLoaded(makeWorkspace({ id: 'root', isWorktree: false, status: 'active' }))
    }
    expect(getUnmergedSubWorkspaces(workspaces)).toEqual([])
  })
})

describe('createWorkspaceStore', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('creates a store with initial workspace state', () => {
    const ws = makeWorkspace()
    const store = createWorkspaceStore(ws, makeHandleDeps())
    const state = store.getState()

    expect(state.workspace).toEqual(ws)
  })

  it('exposes all required action methods', () => {
    const store = createWorkspaceStore(makeWorkspace(), makeHandleDeps())
    const state = store.getState()

    expect(typeof state.addTab).toBe('function')
    expect(typeof state.removeTab).toBe('function')
    expect(typeof state.setActiveTab).toBe('function')
    expect(typeof state.updateTabTitle).toBe('function')
    expect(typeof state.updateTabState).toBe('function')
    expect(typeof state.updateMetadata).toBe('function')
    expect(typeof state.updateStatus).toBe('function')
    expect(state.reviewComments).toBeDefined()
    expect(state.gitController).toBeDefined()
    expect(typeof state.refreshGitInfo).toBe('function')
    expect(typeof state.remove).toBe('function')
  })

  it('updateStatus updates workspace status field', () => {
    const ws = makeWorkspace({ id: 'ws-1', status: 'active' })
    const store = createWorkspaceStore(ws, makeHandleDeps())

    store.getState().updateStatus('merged')

    expect(store.getState().workspace.status).toBe('merged')
  })

  it('updateTabTitle updates the tab title', () => {
    const ws = makeWorkspace({
      id: 'ws-1',
      appStates: { 'tab-1': { applicationId: 'terminal', title: 'Terminal 1', state: { ptyId: null } } },
      activeTabId: 'tab-1'
    })
    const store = createWorkspaceStore(ws, makeHandleDeps())

    store.getState().updateTabTitle('tab-1', 'my shell')

    expect(store.getState().workspace.appStates['tab-1'].title).toBe('my shell')
  })

  it('setActiveTab updates the workspace activeTabId', () => {
    const ws = makeWorkspace({
      id: 'ws-1',
      appStates: {
        'tab-1': { applicationId: 'terminal', title: 'T1', state: {} },
        'tab-2': { applicationId: 'terminal', title: 'T2', state: {} }
      },
      activeTabId: 'tab-1'
    })
    const store = createWorkspaceStore(ws, makeHandleDeps())

    store.getState().setActiveTab('tab-2')

    expect(store.getState().workspace.activeTabId).toBe('tab-2')
  })

  describe('addTab', () => {
    it('adds tab with correct title numbering', () => {
      const app = makeFakeApp()
      const deps = makeHandleDeps({
        appRegistry: {
          get: vi.fn().mockReturnValue(app),
          getDefaultApp: vi.fn().mockReturnValue(null),
        },
      })
      const ws = makeWorkspace({
        id: 'ws-1',
        appStates: { 'tab-existing': { applicationId: 'terminal', title: 'Terminal 1', state: {} } },
      })
      const store = createWorkspaceStore(ws, deps)

      const tabId = store.getState().addTab('terminal')

      const wsState = store.getState().workspace
      const newTab = wsState.appStates[tabId]
      expect(newTab).toBeDefined()
      expect(newTab!.title).toBe('Terminal 2')
      expect(wsState.activeTabId).toBe(tabId)
    })

    it('returns tabId even when app not found (no-op on state)', () => {
      const deps = makeHandleDeps()
      const ws = makeWorkspace({ id: 'ws-1' })
      const store = createWorkspaceStore(ws, deps)

      const tabId = store.getState().addTab('nonexistent')

      expect(typeof tabId).toBe('string')
      expect(Object.keys(store.getState().workspace.appStates)).toHaveLength(0)
    })

    it('creates new tab with merged initial + provided state', () => {
      const app = makeFakeApp({ createInitialState: () => ({ ptyId: null, cwd: '/default' }) })
      const deps = makeHandleDeps({
        appRegistry: {
          get: vi.fn().mockReturnValue(app),
          getDefaultApp: vi.fn().mockReturnValue(null),
        },
      })
      const ws = makeWorkspace({ id: 'ws-1' })
      const store = createWorkspaceStore(ws, deps)

      const tabId = store.getState().addTab('terminal', { cwd: '/custom' })

      const tab = store.getState().workspace.appStates[tabId]
      expect(tab).toBeDefined()
      expect(tab!.state).toEqual({ ptyId: null, cwd: '/custom' })
    })

  })

  describe('removeTab', () => {
    it('removes tab and adjusts activeTabId', async () => {
      const app = makeFakeApp({ canClose: true })
      const deps = makeHandleDeps({
        appRegistry: {
          get: vi.fn().mockReturnValue(app),
          getDefaultApp: vi.fn().mockReturnValue(null),
        },
      })
      const ws = makeWorkspace({
        id: 'ws-1',
        appStates: {
          'tab-1': { applicationId: 'terminal', title: 'T1', state: {} },
          'tab-2': { applicationId: 'terminal', title: 'T2', state: {} },
        },
        activeTabId: 'tab-1',
      })
      const store = createWorkspaceStore(ws, deps)

      await store.getState().removeTab('tab-1')

      const wsState = store.getState().workspace
      expect(Object.keys(wsState.appStates)).toHaveLength(1)
      expect(Object.keys(wsState.appStates)[0]).toBe('tab-2')
      expect(wsState.activeTabId).toBe('tab-2')
    })

    it('calls dispose on tab ref when removing', async () => {
      const dispose = vi.fn()
      const app = makeFakeApp({ canClose: true, onWorkspaceLoad: () => ({ dispose }) })
      const deps = makeHandleDeps({
        appRegistry: {
          get: vi.fn().mockReturnValue(app),
          getDefaultApp: vi.fn().mockReturnValue(null),
        },
      })
      const ws = makeWorkspace({
        id: 'ws-1',
        appStates: { 'tab-1': { applicationId: 'terminal', title: 'T1', state: {} } },
        activeTabId: 'tab-1',
      })
      const store = createWorkspaceStore(ws, deps)

      // initTab to create the ref
      store.getState().initTab('tab-1')
      await store.getState().removeTab('tab-1')

      expect(dispose).toHaveBeenCalledTimes(1)
    })

    it('no-ops when canClose is false', async () => {
      const app = makeFakeApp({ canClose: false })
      const deps = makeHandleDeps({
        appRegistry: {
          get: vi.fn().mockReturnValue(app),
          getDefaultApp: vi.fn().mockReturnValue(null),
        },
      })
      const ws = makeWorkspace({
        id: 'ws-1',
        appStates: { 'tab-1': { applicationId: 'terminal', title: 'T1', state: {} } },
      })
      const store = createWorkspaceStore(ws, deps)

      await store.getState().removeTab('tab-1')

      expect(Object.keys(store.getState().workspace.appStates)).toHaveLength(1)
    })

    it('no-ops for unknown tab', async () => {
      const ws = makeWorkspace({ id: 'ws-1' })
      const store = createWorkspaceStore(ws, makeHandleDeps())

      // Should not throw
      await store.getState().removeTab('nonexistent')
    })
  })

  describe('updateTabState', () => {
    it('applies updater function to tab state', () => {
      const deps = makeHandleDeps()
      const ws = makeWorkspace({
        id: 'ws-1',
        appStates: { 'tab-1': { applicationId: 'terminal', title: 'T1', state: { count: 0 } } },
      })
      const store = createWorkspaceStore(ws, deps)

      store.getState().updateTabState<{ count: number }>('tab-1', (s) => ({ ...s, count: s.count + 1 }))

      const tab = store.getState().workspace.appStates['tab-1']
      expect(tab.state).toEqual({ count: 1 })
    })

    it('syncs to daemon only when ptyId is present in updated state', () => {
      const deps = makeHandleDeps()
      const ws = makeWorkspace({
        id: 'ws-1',
        appStates: { 'tab-1': { applicationId: 'terminal', title: 'T1', state: { count: 0 } } },
      })
      const store = createWorkspaceStore(ws, deps)

      // Update without ptyId — syncToDaemon called only for the state update, not for ptyId
      store.getState().updateTabState('tab-1', (s: { count: number }) => ({ ...s, count: 1 }))
      // syncToDaemon should NOT be called for non-pty state updates
      expect(deps.syncToDaemon).not.toHaveBeenCalled()

      // Update WITH ptyId — should sync
      store.getState().updateTabState('tab-1', (s: { count: number }) => ({ ...s, count: 2, ptyId: 'pty-1' }))
      expect(deps.syncToDaemon).toHaveBeenCalled()
    })
  })

  describe('review comments', () => {
    it('addReviewComment adds a comment to metadata', () => {
      const deps = makeHandleDeps()
      const ws = makeWorkspace({ id: 'ws-1', metadata: {} })
      const store = createWorkspaceStore(ws, deps)

      store.getState().reviewComments.getState().addReviewComment({
        filePath: 'test.ts',
        lineNumber: 10,
        text: 'Fix this',
        commitHash: 'abc123',
        isOutdated: false,
        addressed: false,
        side: 'modified'
      })

      const wsState = store.getState().workspace
      const comments = JSON.parse(wsState.metadata.reviewComments)
      expect(comments).toHaveLength(1)
      expect(comments[0].text).toBe('Fix this')
      expect(comments[0].filePath).toBe('test.ts')
      expect(comments[0].id).toBeDefined()
      expect(comments[0].createdAt).toBeDefined()
    })

    it('deleteReviewComment removes a comment', () => {
      const deps = makeHandleDeps()
      const existingComments = JSON.stringify([
        { id: 'c1', filePath: 'a.ts', lineNumber: 1, text: 'A', commitHash: 'h1', createdAt: 1, isOutdated: false, addressed: false, side: 'modified' },
        { id: 'c2', filePath: 'b.ts', lineNumber: 2, text: 'B', commitHash: 'h1', createdAt: 2, isOutdated: false, addressed: false, side: 'modified' }
      ])
      const ws = makeWorkspace({ id: 'ws-1', metadata: { reviewComments: existingComments } })
      const store = createWorkspaceStore(ws, deps)

      store.getState().reviewComments.getState().deleteReviewComment('c1')

      const wsState = store.getState().workspace
      const comments = JSON.parse(wsState.metadata.reviewComments)
      expect(comments).toHaveLength(1)
      expect(comments[0].id).toBe('c2')
    })

    it('toggleReviewCommentAddressed toggles addressed flag', () => {
      const deps = makeHandleDeps()
      const existingComments = JSON.stringify([
        { id: 'c1', filePath: 'a.ts', lineNumber: 1, text: 'A', commitHash: 'h1', createdAt: 1, isOutdated: false, addressed: false, side: 'modified' }
      ])
      const ws = makeWorkspace({ id: 'ws-1', metadata: { reviewComments: existingComments } })
      const store = createWorkspaceStore(ws, deps)

      store.getState().reviewComments.getState().toggleReviewCommentAddressed('c1')

      const wsState = store.getState().workspace
      const comments = JSON.parse(wsState.metadata.reviewComments)
      expect(comments[0].addressed).toBe(true)
    })

    it('updateOutdatedReviewComments marks comments with different hash as outdated', () => {
      const deps = makeHandleDeps()
      const existingComments = JSON.stringify([
        { id: 'c1', filePath: 'a.ts', lineNumber: 1, text: 'A', commitHash: 'old', createdAt: 1, isOutdated: false, addressed: false, side: 'modified' },
        { id: 'c2', filePath: 'b.ts', lineNumber: 2, text: 'B', commitHash: 'new', createdAt: 2, isOutdated: false, addressed: false, side: 'modified' }
      ])
      const ws = makeWorkspace({ id: 'ws-1', metadata: { reviewComments: existingComments } })
      const store = createWorkspaceStore(ws, deps)

      store.getState().reviewComments.getState().updateOutdatedReviewComments('new')

      const wsState = store.getState().workspace
      const comments = JSON.parse(wsState.metadata.reviewComments)
      expect(comments[0].isOutdated).toBe(true)
      expect(comments[1].isOutdated).toBe(false)
    })

    it('updateOutdatedReviewComments never marks null-commitHash comments as outdated', () => {
      const deps = makeHandleDeps()
      const existingComments = JSON.stringify([
        { id: 'c1', filePath: 'a.ts', lineNumber: 1, text: 'A', commitHash: null, createdAt: 1, isOutdated: false, addressed: false, side: 'modified' }
      ])
      const ws = makeWorkspace({ id: 'ws-1', metadata: { reviewComments: existingComments } })
      const store = createWorkspaceStore(ws, deps)

      store.getState().reviewComments.getState().updateOutdatedReviewComments('any-hash')

      const wsState = store.getState().workspace
      const comments = JSON.parse(wsState.metadata.reviewComments)
      expect(comments[0].isOutdated).toBe(false)
    })

    it('clearReviewComments empties the comments', () => {
      const deps = makeHandleDeps()
      const existingComments = JSON.stringify([
        { id: 'c1', filePath: 'a.ts', lineNumber: 1, text: 'A', commitHash: 'h1', createdAt: 1, isOutdated: false, addressed: false, side: 'modified' }
      ])
      const ws = makeWorkspace({ id: 'ws-1', metadata: { reviewComments: existingComments } })
      const store = createWorkspaceStore(ws, deps)

      store.getState().reviewComments.getState().clearReviewComments()

      const wsState = store.getState().workspace
      const comments = JSON.parse(wsState.metadata.reviewComments)
      expect(comments).toHaveLength(0)
    })
  })

  describe('cross-cutting operations delegate to deps', () => {
    it('refreshGitInfo delegates to deps', async () => {
      const deps = makeHandleDeps()
      const ws = makeWorkspace({ id: 'ws-1' })
      const store = createWorkspaceStore(ws, deps)

      await store.getState().refreshGitInfo()

      expect(deps.refreshGitInfo).toHaveBeenCalledWith('ws-1')
    })

    it('remove delegates to deps', async () => {
      const deps = makeHandleDeps()
      const ws = makeWorkspace({ id: 'ws-1' })
      const store = createWorkspaceStore(ws, deps)

      await store.getState().remove()

      expect(deps.removeWorkspace).toHaveBeenCalledWith('ws-1')
    })

    it('removeKeepBranch delegates to deps', async () => {
      const deps = makeHandleDeps()
      const ws = makeWorkspace({ id: 'ws-1' })
      const store = createWorkspaceStore(ws, deps)

      await store.getState().removeKeepBranch()

      expect(deps.removeWorkspaceKeepBranch).toHaveBeenCalledWith('ws-1')
    })

    it('removeKeepBoth delegates to deps', async () => {
      const deps = makeHandleDeps()
      const ws = makeWorkspace({ id: 'ws-1' })
      const store = createWorkspaceStore(ws, deps)

      await store.getState().removeKeepBoth()

      expect(deps.removeWorkspaceKeepBoth).toHaveBeenCalledWith('ws-1')
    })

    it('mergeAndRemove delegates to deps', async () => {
      const deps = makeHandleDeps()
      const ws = makeWorkspace({ id: 'ws-1' })
      const store = createWorkspaceStore(ws, deps)

      await store.getState().mergeAndRemove(true)

      expect(deps.mergeAndRemoveWorkspace).toHaveBeenCalledWith('ws-1', true)
    })

    it('mergeAndKeep delegates to deps', async () => {
      const deps = makeHandleDeps()
      const ws = makeWorkspace({ id: 'ws-1' })
      const store = createWorkspaceStore(ws, deps)

      await store.getState().mergeAndKeep(true)

      expect(deps.mergeAndKeepWorkspace).toHaveBeenCalledWith('ws-1', true)
    })

    it('closeAndClean delegates to deps', async () => {
      const deps = makeHandleDeps()
      const ws = makeWorkspace({ id: 'ws-1' })
      const store = createWorkspaceStore(ws, deps)

      await store.getState().closeAndClean()

      expect(deps.closeAndCleanWorkspace).toHaveBeenCalledWith('ws-1')
    })

    it('quickForkWorkspace delegates to deps', async () => {
      const deps = makeHandleDeps()
      const ws = makeWorkspace({ id: 'ws-1' })
      const store = createWorkspaceStore(ws, deps)

      await store.getState().quickForkWorkspace()

      expect(deps.quickForkWorkspace).toHaveBeenCalledWith('ws-1')
    })
  })

  // TODO: Collection-level tests (addWorkspace, addChildWorkspace, removeWorkspace,
  // adoptExistingWorktree, createWorktreeFromBranch, createWorktreeFromRemote,
  // removeWorkspaceKeep*, removeOrphanWorkspace, mergeAndRemoveWorkspace,
  // closeAndCleanWorkspace, refreshGitInfo, syncSessionToDaemon) need to be
  // moved to a createSessionStore.test.ts file that tests against the session store.
})
