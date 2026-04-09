import { describe, it, expect, vi, beforeEach } from 'vitest'
import { createWorkspaceStore } from './createWorkspaceStore'
import type { WorkspaceStoreDeps, CachedTerminal } from './createWorkspaceStore'
import { getUnmergedSubWorkspaces, WorkspaceEntryStatus } from './createSessionStore'
import type { WorkspaceEntry } from './createSessionStore'
import type { LlmApi, Workspace, Application } from '../types'
import { createMockExecApi } from '../../shared/mockApis'

interface TestComment { id: string; filePath: string; lineNumber: number; text: string; commitHash: string | null; createdAt: number; isOutdated: boolean; addressed: boolean; side: string }

function makeHandleDeps(overrides?: Partial<WorkspaceStoreDeps>): WorkspaceStoreDeps {
  return {
    appRegistry: {
      get: vi.fn<(...args: any[]) => any>().mockReturnValue(null),
      getDefaultApp: vi.fn<(...args: any[]) => any>().mockReturnValue(null),
    },
    openTtyStream: vi.fn<(...args: any[]) => any>().mockImplementation(() => Promise.resolve({ tty: null })),
    createTty: vi.fn<(...args: any[]) => Promise<string>>().mockResolvedValue('pty-1'),
    connectionId: 'local',
    git: {} as unknown as WorkspaceStoreDeps['git'],
    filesystem: {} as unknown as WorkspaceStoreDeps['filesystem'],
    runActions: { detect: vi.fn<(...args: any[]) => Promise<any[]>>().mockResolvedValue([]), run: vi.fn<(...args: any[]) => Promise<any>>().mockResolvedValue(null) },
    exec: createMockExecApi(),
    syncToDaemon: vi.fn<() => void>(),
    removeWorkspace: vi.fn<(...args: any[]) => Promise<void>>().mockResolvedValue(undefined),
    removeWorkspaceKeepBranch: vi.fn<(...args: any[]) => Promise<void>>().mockResolvedValue(undefined),
    removeWorkspaceKeepBoth: vi.fn<(...args: any[]) => Promise<void>>().mockResolvedValue(undefined),
    mergeAndRemoveWorkspace: vi.fn<(...args: any[]) => Promise<{ success: boolean }>>().mockResolvedValue({ success: true }),
    mergeAndKeepWorkspace: vi.fn<(...args: any[]) => Promise<{ success: boolean }>>().mockResolvedValue({ success: true }),
    closeAndCleanWorkspace: vi.fn<(...args: any[]) => Promise<{ success: boolean }>>().mockResolvedValue({ success: true }),
    quickForkWorkspace: vi.fn<(...args: any[]) => Promise<{ success: boolean }>>().mockResolvedValue({ success: true }),
    refreshGitInfo: vi.fn<(...args: any[]) => Promise<void>>().mockResolvedValue(undefined),
    lookupWorkspace: vi.fn<(...args: any[]) => any>().mockReturnValue(undefined),
    getSettings: vi.fn<() => any>().mockReturnValue({
      llm: { apiKey: '', baseUrl: '' },
      terminalAnalyzer: { model: '', systemPrompt: '', titleSystemPrompt: '', reasoningEffort: 'off', safePaths: [], bufferLines: 10 },
    }),
    llm: {
      analyzeTerminal: vi.fn<(...args: any[]) => Promise<any>>().mockResolvedValue({ state: 'idle', reason: '' }),
      generateTitle: vi.fn<(...args: any[]) => Promise<any>>().mockResolvedValue({ title: '', description: '', branchName: '' }),
    } as unknown as LlmApi,
    setActivityTabState: vi.fn<(...args: any[]) => void>(),
    github: {
      getPrInfo: vi.fn<(...args: any[]) => Promise<any>>().mockResolvedValue({ noPr: true, createUrl: 'https://github.com/test/repo/compare/main...feat?expand=1' }),
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
    onWorkspaceLoad: () => ({ close: () => {}, dispose: () => {} }),
    canClose: true,
    showInNewTabMenu: true,
    displayStyle: 'block',
    isDefault: false,
    render: () => null,
    ...overrides,
  }
}

function toLoaded(ws: Workspace): WorkspaceEntry {
  return { status: WorkspaceEntryStatus.Loaded, data: ws, store: createWorkspaceStore(ws, makeHandleDeps()) }
}

describe('getUnmergedSubWorkspaces', () => {
  it('returns empty array when no workspaces', () => {
    expect(getUnmergedSubWorkspaces(new Map())).toEqual([])
  })

  it('returns only active worktree workspaces', () => {
    const workspaces = new Map<string, WorkspaceEntry>([
      ['root', toLoaded(makeWorkspace({ id: 'root', isWorktree: false, status: 'active' }))],
      ['child1', toLoaded(makeWorkspace({ id: 'child1', isWorktree: true, status: 'active' }))],
      ['child2', toLoaded(makeWorkspace({ id: 'child2', isWorktree: true, status: 'merged' }))],
      ['child3', toLoaded(makeWorkspace({ id: 'child3', isWorktree: true, status: 'active' }))],
    ])
    const result = getUnmergedSubWorkspaces(workspaces)
    expect(result).toHaveLength(2)
    expect(result.map(w => w.id)).toContain('child1')
    expect(result.map(w => w.id)).toContain('child3')
  })

  it('excludes non-worktree workspaces even if active', () => {
    const workspaces = new Map<string, WorkspaceEntry>([
      ['root', toLoaded(makeWorkspace({ id: 'root', isWorktree: false, status: 'active' }))],
    ])
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

    expect(store.getState().workspace.appStates['tab-1']!.title).toBe('my shell')
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
          get: vi.fn<(...args: any[]) => any>().mockReturnValue(app),
          getDefaultApp: vi.fn<(...args: any[]) => any>().mockReturnValue(null),
        },
      })
      const ws = makeWorkspace({
        id: 'ws-1',
        appStates: { 'tab-existing': { applicationId: 'terminal', title: 'Terminal 1', state: {} } },
      })
      const store = createWorkspaceStore(ws, deps)

      const tabId = store.getState().addTab('terminal')

      const wsState = store.getState().workspace
      const newTab = wsState.appStates[tabId]!
      expect(newTab).toBeDefined()
      expect(newTab.title).toBe('Terminal 2')
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
          get: vi.fn<(...args: any[]) => any>().mockReturnValue(app),
          getDefaultApp: vi.fn<(...args: any[]) => any>().mockReturnValue(null),
        },
      })
      const ws = makeWorkspace({ id: 'ws-1' })
      const store = createWorkspaceStore(ws, deps)

      const tabId = store.getState().addTab('terminal', { cwd: '/custom' })

      const tab = store.getState().workspace.appStates[tabId]!
      expect(tab).toBeDefined()
      expect(tab.state).toEqual({ ptyId: null, cwd: '/custom' })
    })

  })

  describe('openOrFocusTab', () => {
    it('focuses existing tab when applicationId matches', () => {
      const app = makeFakeApp()
      const deps = makeHandleDeps({
        appRegistry: {
          get: vi.fn<(...args: any[]) => any>().mockReturnValue(app),
          getDefaultApp: vi.fn<(...args: any[]) => any>().mockReturnValue(null),
        },
      })
      const ws = makeWorkspace({
        id: 'ws-1',
        appStates: {
          'tab-existing': { applicationId: 'review', title: 'Review 1', state: { parentWorkspaceId: 'ws-parent' } },
          'tab-other': { applicationId: 'terminal', title: 'Terminal 1', state: {} },
        },
        activeTabId: 'tab-other',
      })
      const store = createWorkspaceStore(ws, deps)

      const tabId = store.getState().openOrFocusTab('review', { parentWorkspaceId: 'ws-parent' })

      expect(tabId).toBe('tab-existing')
      expect(store.getState().workspace.activeTabId).toBe('tab-existing')
      expect(Object.keys(store.getState().workspace.appStates)).toHaveLength(2)
    })

    it('creates new tab when no matching applicationId exists', () => {
      const app = makeFakeApp()
      const deps = makeHandleDeps({
        appRegistry: {
          get: vi.fn<(...args: any[]) => any>().mockReturnValue(app),
          getDefaultApp: vi.fn<(...args: any[]) => any>().mockReturnValue(null),
        },
      })
      const ws = makeWorkspace({
        id: 'ws-1',
        appStates: {
          'tab-terminal': { applicationId: 'terminal', title: 'Terminal 1', state: {} },
        },
        activeTabId: 'tab-terminal',
      })
      const store = createWorkspaceStore(ws, deps)

      const tabId = store.getState().openOrFocusTab('review', { parentWorkspaceId: 'ws-parent' })

      expect(tabId).not.toBe('tab-terminal')
      expect(store.getState().workspace.activeTabId).toBe(tabId)
      expect(Object.keys(store.getState().workspace.appStates)).toHaveLength(2)
      expect(store.getState().workspace.appStates[tabId]!.applicationId).toBe('review')
    })
  })

  describe('removeTab', () => {
    it('removes tab and adjusts activeTabId', async () => {
      const app = makeFakeApp({ canClose: true })
      const deps = makeHandleDeps({
        appRegistry: {
          get: vi.fn<(...args: any[]) => any>().mockReturnValue(app),
          getDefaultApp: vi.fn<(...args: any[]) => any>().mockReturnValue(null),
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
      const close = vi.fn<() => void>()
      const dispose = vi.fn<() => void>()
      const app = makeFakeApp({ canClose: true, onWorkspaceLoad: () => ({ close, dispose }) })
      const deps = makeHandleDeps({
        appRegistry: {
          get: vi.fn<(...args: any[]) => any>().mockReturnValue(app),
          getDefaultApp: vi.fn<(...args: any[]) => any>().mockReturnValue(null),
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
          get: vi.fn<(...args: any[]) => any>().mockReturnValue(app),
          getDefaultApp: vi.fn<(...args: any[]) => any>().mockReturnValue(null),
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

      const tab = store.getState().workspace.appStates['tab-1']!
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
      const comments = JSON.parse(wsState.metadata.reviewComments!) as TestComment[]
      expect(comments).toHaveLength(1)
      expect(comments[0]!.text).toBe('Fix this')
      expect(comments[0]!.filePath).toBe('test.ts')
      expect(comments[0]!.id).toBeDefined()
      expect(comments[0]!.createdAt).toBeDefined()
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
      const comments = JSON.parse(wsState.metadata.reviewComments!) as TestComment[]
      expect(comments).toHaveLength(1)
      expect(comments[0]!.id).toBe('c2')
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
      const comments = JSON.parse(wsState.metadata.reviewComments!) as TestComment[]
      expect(comments[0]!.addressed).toBe(true)
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
      const comments = JSON.parse(wsState.metadata.reviewComments!) as TestComment[]
      expect(comments[0]!.isOutdated).toBe(true)
      expect(comments[1]!.isOutdated).toBe(false)
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
      const comments = JSON.parse(wsState.metadata.reviewComments!) as TestComment[]
      expect(comments[0]!.isOutdated).toBe(false)
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
      const comments = JSON.parse(wsState.metadata.reviewComments!) as TestComment[]
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

  describe('promptHarness', () => {
    it('returns false when no ai harness tab exists', async () => {
      const ws = makeWorkspace({
        id: 'ws-1',
        appStates: { 'tab-1': { applicationId: 'terminal', title: 'T1', state: { ptyId: 'pty-1', ptyHandle: null, keepOnExit: false } } },
      })
      const store = createWorkspaceStore(ws, makeHandleDeps())
      expect(await store.getState().promptHarness('hello')).toBe(false)
    })

    it('writes text and sets active tab when ai harness tab has ptyId', async () => {
      const writeFn = vi.fn<(...args: any[]) => void>()
      const killFn = vi.fn<() => void>()
      const ttyStore = {
        getState: () => ({ ptyId: 'pty-h', write: writeFn, resize: vi.fn<(...args: any[]) => void>(), kill: killFn }),
        setState: vi.fn<(...args: any[]) => void>(),
        subscribe: vi.fn<(...args: any[]) => any>(),
        destroy: vi.fn<() => void>(),
        getInitialState: vi.fn<() => any>(),
      }
      const deps = makeHandleDeps({
        openTtyStream: vi.fn<(...args: any[]) => any>().mockResolvedValue({ tty: ttyStore }),
      })
      const ws = makeWorkspace({
        id: 'ws-1',
        appStates: {
          'tab-h': {
            applicationId: 'aiharness-claude',
            title: 'AI',
            state: {
              ptyId: 'pty-h',
              ptyHandle: null,
              keepOnExit: false,
              sandbox: { enabled: false, allowNetwork: true },
              autoApprove: false,
            },
          },
        },
        activeTabId: null,
      })
      const store = createWorkspaceStore(ws, deps)

      const result = await store.getState().promptHarness('run tests')
      expect(result).toBe(true)
      expect(writeFn).toHaveBeenCalledWith('run tests\r')
      expect(store.getState().workspace.activeTabId).toBe('tab-h')
    })

    it('retries on first getTtyWriter failure', async () => {
      let callCount = 0
      const writeFn = vi.fn<(...args: any[]) => void>()
      const ttyStore = {
        getState: () => ({ ptyId: 'pty-h', write: writeFn, resize: vi.fn<(...args: any[]) => void>(), kill: vi.fn<() => void>() }),
        setState: vi.fn<(...args: any[]) => void>(),
        subscribe: vi.fn<(...args: any[]) => any>(),
        destroy: vi.fn<() => void>(),
        getInitialState: vi.fn<() => any>(),
      }
      const deps = makeHandleDeps({
        openTtyStream: vi.fn<(...args: any[]) => any>().mockImplementation(() => {
          callCount++
          if (callCount === 1) return Promise.reject(new Error('fail'))
          return Promise.resolve({ tty: ttyStore })
        }),
      })
      const ws = makeWorkspace({
        id: 'ws-1',
        appStates: {
          'tab-h': {
            applicationId: 'aiharness-claude',
            title: 'AI',
            state: {
              ptyId: 'pty-h',
              ptyHandle: null,
              keepOnExit: false,
              sandbox: { enabled: false, allowNetwork: true },
              autoApprove: false,
            },
          },
        },
      })
      const store = createWorkspaceStore(ws, deps)

      const result = await store.getState().promptHarness('retry test')
      expect(result).toBe(true)
      expect(writeFn).toHaveBeenCalledWith('retry test\r')
    })
  })

  describe('gitApi delegation', () => {
    function makeGitDeps(): WorkspaceStoreDeps {
      return makeHandleDeps({
        git: {
          getInfo: vi.fn<(...args: any[]) => Promise<any>>().mockResolvedValue({}),
          getDiff: vi.fn<(...args: any[]) => Promise<any>>().mockResolvedValue({ success: true }),
          commitAll: vi.fn<(...args: any[]) => Promise<any>>().mockResolvedValue({ success: true }),
          stageFile: vi.fn<(...args: any[]) => Promise<any>>().mockResolvedValue({ success: true }),
          fetch: vi.fn<(...args: any[]) => Promise<any>>().mockResolvedValue({ success: true }),
          createWorktree: vi.fn<(...args: any[]) => Promise<any>>().mockResolvedValue({ success: true }),
          listWorktrees: vi.fn<(...args: any[]) => Promise<any>>().mockResolvedValue([]),
          getFileDiff: vi.fn<(...args: any[]) => Promise<any>>().mockResolvedValue({ success: true }),
          getBehindCount: vi.fn<(...args: any[]) => Promise<number>>().mockResolvedValue(0),
          hasUncommittedChanges: vi.fn<(...args: any[]) => Promise<boolean>>().mockResolvedValue(false),
          checkMergeConflicts: vi.fn<(...args: any[]) => Promise<any>>().mockResolvedValue({ success: true }),
          pull: vi.fn<(...args: any[]) => Promise<any>>().mockResolvedValue({ success: true }),
        } as unknown as WorkspaceStoreDeps['git'],
      })
    }

    it('getInfo delegates with workspace.path', async () => {
      const deps = makeGitDeps()
      const store = createWorkspaceStore(makeWorkspace({ path: '/repo' }), deps)
      await store.getState().gitApi.getInfo()
      expect(deps.git.getInfo).toHaveBeenCalledWith('/repo')
    })

    it('getDiff delegates with workspace.path and parentBranch', async () => {
      const deps = makeGitDeps()
      const store = createWorkspaceStore(makeWorkspace({ path: '/repo' }), deps)
      await store.getState().gitApi.getDiff('main')
      expect(deps.git.getDiff).toHaveBeenCalledWith('/repo', 'main')
    })

    it('commitAll delegates with workspace.path and message', async () => {
      const deps = makeGitDeps()
      const store = createWorkspaceStore(makeWorkspace({ path: '/repo' }), deps)
      await store.getState().gitApi.commitAll('fix bug')
      expect(deps.git.commitAll).toHaveBeenCalledWith('/repo', 'fix bug')
    })

    it('stageFile delegates with workspace.path and filePath', async () => {
      const deps = makeGitDeps()
      const store = createWorkspaceStore(makeWorkspace({ path: '/repo' }), deps)
      await store.getState().gitApi.stageFile('src/a.ts')
      expect(deps.git.stageFile).toHaveBeenCalledWith('/repo', 'src/a.ts')
    })

    it('fetch delegates with workspace.path', async () => {
      const deps = makeGitDeps()
      const store = createWorkspaceStore(makeWorkspace({ path: '/repo' }), deps)
      await store.getState().gitApi.fetch()
      expect(deps.git.fetch).toHaveBeenCalledWith('/repo')
    })
  })

  describe('filesystemApi delegation', () => {
    function makeFsDeps(): WorkspaceStoreDeps {
      return makeHandleDeps({
        filesystem: {
          readDirectory: vi.fn<(...args: any[]) => Promise<any>>().mockResolvedValue([]),
          readFile: vi.fn<(...args: any[]) => Promise<any>>().mockResolvedValue('content'),
          writeFile: vi.fn<(...args: any[]) => Promise<any>>().mockResolvedValue(undefined),
          searchFiles: vi.fn<(...args: any[]) => Promise<any>>().mockResolvedValue([]),
        } as unknown as WorkspaceStoreDeps['filesystem'],
      })
    }

    it('readDirectory delegates with workspace.path', async () => {
      const deps = makeFsDeps()
      const store = createWorkspaceStore(makeWorkspace({ path: '/repo' }), deps)
      await store.getState().filesystemApi.readDirectory('/src')
      expect(deps.filesystem.readDirectory).toHaveBeenCalledWith('/repo', '/src')
    })

    it('writeFile delegates with workspace.path', async () => {
      const deps = makeFsDeps()
      const store = createWorkspaceStore(makeWorkspace({ path: '/repo' }), deps)
      await store.getState().filesystemApi.writeFile('f.ts', 'code')
      expect(deps.filesystem.writeFile).toHaveBeenCalledWith('/repo', 'f.ts', 'code')
    })
  })

  describe('getTtyWriter', () => {
    function makeTtyDeps() {
      let onEventCb: ((event: { type: string }) => void) | null = null
      const writeFn = vi.fn<(...args: any[]) => void>()
      const killFn = vi.fn<() => void>()
      const ttyStore = {
        getState: () => ({ ptyId: 'pty-1', write: writeFn, resize: vi.fn<(...args: any[]) => void>(), kill: killFn }),
        setState: vi.fn<(...args: any[]) => void>(),
        subscribe: vi.fn<(...args: any[]) => any>(),
        destroy: vi.fn<() => void>(),
        getInitialState: vi.fn<() => any>(),
      }
      const deps = makeHandleDeps({
        openTtyStream: vi.fn<(...args: any[]) => any>().mockImplementation((_ptyId: string, onEvent: (event: { type: string }) => void) => {
          onEventCb = onEvent
          return Promise.resolve({ tty: ttyStore })
        }),
      })
      return { deps, writeFn, killFn, getOnEvent: () => {
        if (!onEventCb) throw new Error('onEventCb not set')
        return onEventCb
      } }
    }

    it('opens stream and returns writer with write/kill', async () => {
      const { deps, writeFn, killFn } = makeTtyDeps()
      const store = createWorkspaceStore(makeWorkspace(), deps)

      const writer = await store.getState().getTtyWriter('pty-1')
      writer.write('hello')
      expect(writeFn).toHaveBeenCalledWith('hello')
      writer.kill()
      expect(killFn).toHaveBeenCalled()
    })

    it('returns cached writer on second call', async () => {
      const { deps } = makeTtyDeps()
      const store = createWorkspaceStore(makeWorkspace(), deps)

      const w1 = await store.getState().getTtyWriter('pty-1')
      const w2 = await store.getState().getTtyWriter('pty-1')
      expect(w1).toBe(w2)
      expect(deps.openTtyStream).toHaveBeenCalledTimes(1)
    })

    it('removes cache on end event, next call opens new stream', async () => {
      const { deps, getOnEvent } = makeTtyDeps()
      const store = createWorkspaceStore(makeWorkspace(), deps)

      await store.getState().getTtyWriter('pty-1')
      getOnEvent()({ type: 'end' })
      await store.getState().getTtyWriter('pty-1')
      expect(deps.openTtyStream).toHaveBeenCalledTimes(2)
    })

    it('throws on write after disconnect', async () => {
      const { deps, getOnEvent } = makeTtyDeps()
      const store = createWorkspaceStore(makeWorkspace(), deps)

      const writer = await store.getState().getTtyWriter('pty-1')
      getOnEvent()({ type: 'end' })
      expect(() => { writer.write('x'); }).toThrow('disconnected')
    })

    it('throws on kill after disconnect', async () => {
      const { deps, getOnEvent } = makeTtyDeps()
      const store = createWorkspaceStore(makeWorkspace(), deps)

      const writer = await store.getState().getTtyWriter('pty-1')
      getOnEvent()({ type: 'error' })
      expect(() => { writer.kill(); }).toThrow('disconnected')
    })
  })

  describe('disposeAllCachedTerminals', () => {
    it('disposes all cached terminals', () => {
      const store = createWorkspaceStore(makeWorkspace(), makeHandleDeps())
      const disposeFns = [vi.fn<() => void>(), vi.fn<() => void>()]
      const terminalMocks = disposeFns.map((unsub) => ({
        terminal: { dispose: vi.fn<() => void>() },
        tty: {} as unknown as CachedTerminal['tty'],
        unsubscribeEvents: unsub,
        mountedHandler: null,
        stripScrollbackClear: false,
        connectedAt: Date.now(),
        dataVersion: 0,
        onExitUnmounted: vi.fn<(...args: any[]) => void>(),
      }))

      store.getState().setCachedTerminal('tab-a', terminalMocks[0] as unknown as CachedTerminal)
      store.getState().setCachedTerminal('tab-b', terminalMocks[1] as unknown as CachedTerminal)
      expect(store.getState().getCachedTerminal('tab-a')).not.toBeNull()
      expect(store.getState().getCachedTerminal('tab-b')).not.toBeNull()

      store.getState().disposeAllCachedTerminals()

      expect(store.getState().getCachedTerminal('tab-a')).toBeNull()
      expect(store.getState().getCachedTerminal('tab-b')).toBeNull()
      expect(disposeFns[0]).toHaveBeenCalled()
      expect(disposeFns[1]).toHaveBeenCalled()
      expect(terminalMocks[0]!.terminal.dispose).toHaveBeenCalled()
      expect(terminalMocks[1]!.terminal.dispose).toHaveBeenCalled()
    })
  })

  describe('initTab', () => {
    it('no-ops when already initialized', () => {
      const onWorkspaceLoad = vi.fn<(...args: any[]) => any>().mockReturnValue({ dispose: vi.fn<() => void>() })
      const app = makeFakeApp({ onWorkspaceLoad })
      const deps = makeHandleDeps({
        appRegistry: { get: vi.fn<(...args: any[]) => any>().mockReturnValue(app), getDefaultApp: vi.fn<(...args: any[]) => any>().mockReturnValue(null) },
      })
      const ws = makeWorkspace({
        appStates: { 'tab-1': { applicationId: 'terminal', title: 'T1', state: {} } },
      })
      const store = createWorkspaceStore(ws, deps)

      store.getState().initTab('tab-1')
      store.getState().initTab('tab-1')
      expect(onWorkspaceLoad).toHaveBeenCalledTimes(1)
    })

    it('no-ops when appState is missing', () => {
      const deps = makeHandleDeps()
      const store = createWorkspaceStore(makeWorkspace(), deps)
      // Should not throw
      store.getState().initTab('nonexistent')
      expect(store.getState().getTabRef('nonexistent')).toBeNull()
    })

    it('no-ops when app not found in registry', () => {
      const deps = makeHandleDeps()
      const ws = makeWorkspace({
        appStates: { 'tab-1': { applicationId: 'unknown-app', title: 'T1', state: {} } },
      })
      const store = createWorkspaceStore(ws, deps)
      store.getState().initTab('tab-1')
      expect(store.getState().getTabRef('tab-1')).toBeNull()
    })

    it('stores ref accessible via getTabRef', () => {
      const ref = { dispose: vi.fn<() => void>() }
      const app = makeFakeApp({ onWorkspaceLoad: vi.fn<(...args: any[]) => any>().mockReturnValue(ref) })
      const deps = makeHandleDeps({
        appRegistry: { get: vi.fn<(...args: any[]) => any>().mockReturnValue(app), getDefaultApp: vi.fn<(...args: any[]) => any>().mockReturnValue(null) },
      })
      const ws = makeWorkspace({
        appStates: { 'tab-1': { applicationId: 'terminal', title: 'T1', state: {} } },
      })
      const store = createWorkspaceStore(ws, deps)

      store.getState().initTab('tab-1')
      expect(store.getState().getTabRef('tab-1')).toBe(ref)
    })
  })

  describe('lookupWorkspace', () => {
    it('delegates to deps.lookupWorkspace', () => {
      const mockWs = makeWorkspace({ id: 'other' })
      const deps = makeHandleDeps({ lookupWorkspace: vi.fn<(...args: any[]) => any>().mockReturnValue(mockWs) })
      const store = createWorkspaceStore(makeWorkspace(), deps)

      expect(store.getState().lookupWorkspace('other')).toBe(mockWs)
      expect(deps.lookupWorkspace).toHaveBeenCalledWith('other')
    })
  })

  // TODO: Collection-level tests (addWorkspace, addChildWorkspace, removeWorkspace,
  // adoptExistingWorktree, createWorktreeFromBranch, createWorktreeFromRemote,
  // removeWorkspaceKeep*, removeOrphanWorkspace, mergeAndRemoveWorkspace,
  // closeAndCleanWorkspace, refreshGitInfo, syncSessionToDaemon) need to be
  // moved to a createSessionStore.test.ts file that tests against the session store.
})
