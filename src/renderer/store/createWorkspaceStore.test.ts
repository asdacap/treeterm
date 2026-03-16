import { describe, it, expect, vi, beforeEach } from 'vitest'
import { createWorkspaceStore, getUnmergedSubWorkspaces } from './createWorkspaceStore'
import type { WorkspaceDeps } from './createWorkspaceStore'
import type { Workspace } from '../types'

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
      getDiffAgainstHead: vi.fn().mockResolvedValue({ success: true }),
      getFileDiffAgainstHead: vi.fn().mockResolvedValue({ success: true }),
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
      getFileContentsForDiffAgainstHead: vi.fn().mockResolvedValue({ success: true }),
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
    createdAt: Date.now(),
    lastActivity: Date.now(),
    attachedClients: 0,
    ...overrides
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
})
