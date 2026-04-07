import { describe, it, expect, vi, beforeEach } from 'vitest'
import {
  createTerminalApplication,
  createTerminalVariant,
  type TerminalRef
} from './renderer'
import type { Tab, Workspace, TerminalInstance } from '../../renderer/types'
import { createMockGitApi, createMockFilesystemApi, createMockRunActionsApi, createMockExecApi } from '../../shared/mockApis'
import { createStore } from 'zustand/vanilla'
import type { WorkspaceStoreState } from '../../renderer/store/createWorkspaceStore'
import type { GitControllerState } from '../../renderer/store/createGitControllerStore'
import type { ReviewCommentState } from '../../renderer/store/createReviewCommentStore'

// Mock React
vi.mock('react', () => ({
  createElement: vi.fn((component: unknown, props: unknown) => ({ component, props }))
}))

// Mock Terminal component
vi.mock('../../renderer/components/Terminal', () => ({
  default: vi.fn(() => null)
}))

// Mock activity state store
const mockRemoveTabState = vi.fn<(tabId: string) => void>()
vi.mock('../../renderer/store/activityState', () => ({
  useActivityStateStore: {
    getState: vi.fn(() => ({
      removeTabState: mockRemoveTabState
    }))
  }
}))

const mockTerminalKill = vi.fn<(connectionId: string, ptyId: string) => void>()
const mockDeps = { terminal: { kill: mockTerminalKill } }

function createMockAnalyzer() {
  const state = { start: vi.fn(), stop: vi.fn(), getHistory: vi.fn<() => unknown[]>().mockReturnValue([]) }
  return { getState: vi.fn().mockReturnValue(state), setState: vi.fn(), subscribe: vi.fn(), _state: state }
}

const mockWorkspaceStoreStateData = {
  workspace: { id: 'ws-1', path: '/test', appStates: {} } as Workspace,
  addTab: vi.fn(),
  removeTab: vi.fn(),
  setActiveTab: vi.fn(),
  updateTabTitle: vi.fn(),
  updateTabState: vi.fn(),
  reviewComments: createStore<ReviewCommentState>()(() => ({
    getReviewComments: vi.fn().mockReturnValue([]),
    addReviewComment: vi.fn(),
    deleteReviewComment: vi.fn(),
    toggleReviewCommentAddressed: vi.fn(),
    updateOutdatedReviewComments: vi.fn(),
    clearReviewComments: vi.fn(),
    markAllReviewCommentsAddressed: vi.fn(),
  } as ReviewCommentState)),
  promptHarness: vi.fn(),
  quickForkWorkspace: vi.fn(),
  updateMetadata: vi.fn(),
  updateStatus: vi.fn(),
  refreshGitInfo: vi.fn(),
  mergeAndRemove: vi.fn(),
  mergeAndKeep: vi.fn(),
  closeAndClean: vi.fn(),
  lookupWorkspace: vi.fn(),
  remove: vi.fn(),
  removeKeepBranch: vi.fn(),
  removeKeepBoth: vi.fn(),
  gitApi: createMockGitApi(),
  filesystemApi: createMockFilesystemApi(),
  runActionsApi: createMockRunActionsApi(),
  execApi: createMockExecApi(),
  initTab: vi.fn(),
  getTabRef: vi.fn().mockReturnValue(null),
  getCachedTerminal: vi.fn().mockReturnValue(null),
  setCachedTerminal: vi.fn(),
  disposeCachedTerminal: vi.fn(), disposeAllCachedTerminals: vi.fn(), disposeTabResources: vi.fn(),
  initAnalyzer: vi.fn().mockReturnValue(createMockAnalyzer()),
  createTty: vi.fn().mockResolvedValue('pty-1'), getTtyWriter: vi.fn().mockResolvedValue({ write: vi.fn<(data: string) => void>(), kill: vi.fn<() => void>() }),
  connectionId: 'local',
  updateSettings: vi.fn(),
  focusTabId: null,
  requestFocus: vi.fn(),
  clearFocusRequest: vi.fn(),
  gitController: createStore<GitControllerState>()(() => ({
    hasUncommittedChanges: false,
    isDiffCleanFromParent: false,
    hasConflictsWithParent: false,
    behindCount: 0,
    pullLoading: false,
    gitRefreshing: false,
    prInfo: null,
    refreshDiffStatus: vi.fn(),
    refreshRemoteStatus: vi.fn(),
    pullFromRemote: vi.fn(),
    refreshPrStatus: vi.fn(),
    openGitHub: vi.fn(),
    startPolling: vi.fn(),
    dispose: vi.fn(),
  } as GitControllerState)),
} as WorkspaceStoreState

const mockWorkspaceStore = createStore<WorkspaceStoreState>()(() => mockWorkspaceStoreStateData)

describe('Terminal Renderer', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  describe('createTerminalApplication', () => {
    it('creates terminal application with correct properties', () => {
      const app = createTerminalApplication(mockDeps)

      expect(app.id).toBe('terminal')
      expect(app.name).toBe('Terminal')
      expect(app.icon).toBe('>')
      expect(app.canClose).toBe(true)
      expect(app.showInNewTabMenu).toBe(true)
      expect(app.displayStyle).toBe('flex')
    })

    it('has isDefault set to true', () => {
      const app = createTerminalApplication(mockDeps)
      expect(app.isDefault).toBe(true)
    })

    describe('createInitialState', () => {
      it('returns terminal state with null ptyId', () => {
        const app = createTerminalApplication(mockDeps)
        const state = app.createInitialState()

        expect(state).toEqual({ ptyId: null, ptyHandle: null, keepOnExit: false })
      })
    })

    describe('onWorkspaceLoad and dispose', () => {
      it('dispose kills PTY when tab has ptyId and removes activity state', () => {
        const app = createTerminalApplication(mockDeps)
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'terminal',
          title: 'Terminal',
          state: { ptyId: 'pty-123' }
        }

        const ref = app.onWorkspaceLoad(tab, mockWorkspaceStore)
        ref.dispose()

        expect(mockTerminalKill).toHaveBeenCalledWith('local', 'pty-123')
        expect(mockRemoveTabState).toHaveBeenCalledWith('tab-1')
      })

      it('dispose does not kill PTY when tab has no ptyId but removes activity state', () => {
        const app = createTerminalApplication(mockDeps)
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'terminal',
          title: 'Terminal',
          state: { ptyId: null }
        }

        const ref = app.onWorkspaceLoad(tab, mockWorkspaceStore)
        ref.dispose()

        expect(mockTerminalKill).not.toHaveBeenCalled()
        expect(mockRemoveTabState).toHaveBeenCalledWith('tab-1')
      })

      it('creates PTY and updates tab state when ptyId is null', async () => {
        let resolveTty!: (value: string) => void
        const ttyPromise = new Promise<string>((resolve) => { resolveTty = resolve })
        const mockCreateTty = vi.fn().mockReturnValue(ttyPromise)
        const mockUpdateTabState = vi.fn()
        const store = createStore<WorkspaceStoreState>()(() => ({
          ...mockWorkspaceStoreStateData,
          createTty: mockCreateTty,
          updateTabState: mockUpdateTabState,
        }))

        const app = createTerminalApplication(mockDeps)
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'terminal',
          title: 'Terminal',
          state: { ptyId: null }
        }

        app.onWorkspaceLoad(tab, store)

        expect(mockCreateTty).toHaveBeenCalledWith('/test', undefined, undefined)
        expect(mockUpdateTabState).not.toHaveBeenCalled()

        resolveTty('pty-new')
        await ttyPromise
        // Flush the .then() microtask scheduled by createTty().then(...)
        await new Promise(resolve => setTimeout(resolve, 0))

        expect(mockUpdateTabState).toHaveBeenCalledWith('tab-1', expect.any(Function))

        // Verify the state updater function produces correct state
        const updater = mockUpdateTabState.mock.calls[0][1] as (prev: Record<string, unknown>) => Record<string, unknown>
        const updated = updater({ ptyId: null, ptyHandle: null, keepOnExit: false })
        expect(updated).toEqual({
          ptyId: 'pty-new',
          ptyHandle: null,
          keepOnExit: false,
          connectionId: 'local',
        })
      })

      it('does not call createTty when ptyId already exists', () => {
        const app = createTerminalApplication(mockDeps)
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'terminal',
          title: 'Terminal',
          state: { ptyId: 'existing-pty' }
        }

        app.onWorkspaceLoad(tab, mockWorkspaceStore)

        expect(mockWorkspaceStoreStateData.createTty).not.toHaveBeenCalled()
      })

      it('returns ref with analyzer and dispose', () => {
        const app = createTerminalApplication(mockDeps)
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'terminal',
          title: 'Terminal',
          state: { ptyId: null }
        }

        const ref = app.onWorkspaceLoad(tab, mockWorkspaceStore) as TerminalRef

        expect(typeof ref.dispose).toBe('function')
        expect(ref.analyzer).toBeDefined()
      })

      it('starts analyzer immediately when ptyId exists (restore path)', () => {
        const mockAnalyzer = createMockAnalyzer()
        const store = createStore<WorkspaceStoreState>()(() => ({
          ...mockWorkspaceStoreStateData,
          initAnalyzer: vi.fn().mockReturnValue(mockAnalyzer),
        }))

        const app = createTerminalApplication(mockDeps)
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'terminal',
          title: 'Terminal',
          state: { ptyId: 'pty-existing' }
        }

        app.onWorkspaceLoad(tab, store)

        expect(mockAnalyzer._state.start).toHaveBeenCalledWith('pty-existing')
        expect(store.getState().createTty).not.toHaveBeenCalled()
      })

      it('starts analyzer after createTty resolves (new tab path)', async () => {
        const mockAnalyzer = createMockAnalyzer()
        let resolveTty!: (value: string) => void
        const ttyPromise = new Promise<string>((resolve) => { resolveTty = resolve })
        const store = createStore<WorkspaceStoreState>()(() => ({
          ...mockWorkspaceStoreStateData,
          initAnalyzer: vi.fn().mockReturnValue(mockAnalyzer),
          createTty: vi.fn().mockReturnValue(ttyPromise),
          updateTabState: vi.fn(),
        }))

        const app = createTerminalApplication(mockDeps)
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'terminal',
          title: 'Terminal',
          state: { ptyId: null }
        }

        app.onWorkspaceLoad(tab, store)

        expect(mockAnalyzer._state.start).not.toHaveBeenCalled()

        resolveTty('pty-new')
        await ttyPromise
        await new Promise(resolve => setTimeout(resolve, 0))

        expect(mockAnalyzer._state.start).toHaveBeenCalledWith('pty-new')
      })

      it('dispose stops analyzer', () => {
        const mockAnalyzer = createMockAnalyzer()
        const store = createStore<WorkspaceStoreState>()(() => ({
          ...mockWorkspaceStoreStateData,
          initAnalyzer: vi.fn().mockReturnValue(mockAnalyzer),
        }))

        const app = createTerminalApplication(mockDeps)
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'terminal',
          title: 'Terminal',
          state: { ptyId: 'pty-123' }
        }

        const ref = app.onWorkspaceLoad(tab, store)
        ref.dispose()

        expect(mockAnalyzer._state.stop).toHaveBeenCalled()
      })
    })

    describe('render', () => {
      it('renders Terminal component with correct props', () => {
        const app = createTerminalApplication(mockDeps)
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'terminal',
          title: 'Terminal',
          state: { ptyId: null }
        }

        const result = app.render({
          tab,
          workspace: mockWorkspaceStore,
          isVisible: true,
        })

        expect(result).toEqual(expect.objectContaining({
          component: expect.any(Function) as unknown,
          props: expect.objectContaining({
            key: 'tab-1',
            cwd: '/test',
            workspace: mockWorkspaceStore,
            tabId: 'tab-1',
            isVisible: true
          }) as unknown,
        }))
      })
    })
  })

  describe('terminalApplication (via createTerminalApplication)', () => {
    it('is a terminal application with isDefault set to true', () => {
      const app = createTerminalApplication(mockDeps)
      expect(app.id).toBe('terminal')
      expect(app.name).toBe('Terminal')
      expect(app.isDefault).toBe(true)
    })

    it('can create initial state', () => {
      const app = createTerminalApplication(mockDeps)
      const state = app.createInitialState()
      expect(state).toEqual({ ptyId: null, ptyHandle: null, keepOnExit: false })
    })

    it('can render Terminal component', () => {
      const app = createTerminalApplication(mockDeps)
      const tab: Tab = {
        id: 'tab-1',
        applicationId: 'terminal',
        title: 'Terminal',
        state: { ptyId: null }
      }

      const result = app.render({
        tab,
        workspace: mockWorkspaceStore,
        isVisible: true,
      })

      expect(result).toBeDefined()
      expect(result).toHaveProperty('component')
      expect(result).toHaveProperty('props')
    })

    it('onWorkspaceLoad returns ref that disposes correctly', () => {
      const app = createTerminalApplication(mockDeps)
      const tab: Tab = {
        id: 'tab-1',
        applicationId: 'terminal',
        title: 'Terminal',
        state: { ptyId: 'pty-123' }
      }

      const ref = app.onWorkspaceLoad(tab, mockWorkspaceStore)
      ref.dispose()

      expect(mockTerminalKill).toHaveBeenCalledWith('local', 'pty-123')
      expect(mockRemoveTabState).toHaveBeenCalledWith('tab-1')
    })
  })

  describe('createTerminalVariant', () => {
    const mockInstance: TerminalInstance = {
      id: 'custom-term',
      name: 'Custom Terminal',
      icon: '\u{1F680}',
      startupCommand: 'echo "Hello"',
      isDefault: false
    }

    it('creates variant with custom id and name', () => {
      const variant = createTerminalVariant(mockInstance, mockDeps)

      expect(variant.id).toBe('terminal-custom-term')
      expect(variant.name).toBe('Custom Terminal')
      expect(variant.icon).toBe('\u{1F680}')
    })

    it('preserves other application properties', () => {
      const variant = createTerminalVariant(mockInstance, mockDeps)

      expect(variant.canClose).toBe(true)
      expect(variant.showInNewTabMenu).toBe(true)
      expect(variant.displayStyle).toBe('flex')
    })

    it('sets isDefault from instance', () => {
      const variant = createTerminalVariant(mockInstance, mockDeps)
      expect(variant.isDefault).toBe(false)

      const defaultVariant = createTerminalVariant({
        ...mockInstance,
        isDefault: true
      }, mockDeps)
      expect(defaultVariant.isDefault).toBe(true)
    })

    describe('createInitialState', () => {
      it('returns terminal state with null ptyId', () => {
        const variant = createTerminalVariant(mockInstance, mockDeps)
        const state = variant.createInitialState()

        expect(state).toEqual({ ptyId: null, ptyHandle: null, keepOnExit: false })
      })
    })

    describe('onWorkspaceLoad and dispose', () => {
      it('dispose kills PTY when tab has ptyId', () => {
        const variant = createTerminalVariant(mockInstance, mockDeps)
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'terminal-custom-term',
          title: 'Custom Terminal',
          state: { ptyId: 'pty-456' }
        }

        const ref = variant.onWorkspaceLoad(tab, mockWorkspaceStore)
        ref.dispose()

        expect(mockTerminalKill).toHaveBeenCalledWith('local', 'pty-456')
        expect(mockRemoveTabState).toHaveBeenCalledWith('tab-1')
      })

      it('dispose does not kill PTY when state has no ptyId', () => {
        const variant = createTerminalVariant(mockInstance, mockDeps)
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'terminal-custom-term',
          title: 'Custom Terminal',
          state: { ptyId: null }
        }

        const ref = variant.onWorkspaceLoad(tab, mockWorkspaceStore)
        ref.dispose()

        expect(mockTerminalKill).not.toHaveBeenCalled()
        expect(mockRemoveTabState).toHaveBeenCalledWith('tab-1')
      })
    })

    describe('render', () => {
      it('renders Terminal component with startup command', () => {
        const variant = createTerminalVariant(mockInstance, mockDeps)
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'terminal-custom-term',
          title: 'Custom Terminal',
          state: { ptyId: null }
        }

        const result = variant.render({
          tab,
          workspace: mockWorkspaceStore,
          isVisible: true,
        })

        expect(result).toEqual(expect.objectContaining({
          component: expect.any(Function) as unknown,
          props: expect.objectContaining({
            key: 'tab-1',
            cwd: '/test',
            workspace: mockWorkspaceStore,
            tabId: 'tab-1',
            isVisible: true,
            startupCommand: 'echo "Hello"'
          }) as unknown,
        }))
      })

      it('passes empty startupCommand when not set', () => {
        const variantWithoutCommand = createTerminalVariant({
          ...mockInstance,
          startupCommand: ''
        }, mockDeps)
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'terminal-custom-term',
          title: 'Custom Terminal',
          state: { ptyId: null }
        }

        const result = variantWithoutCommand.render({
          tab,
          workspace: mockWorkspaceStore,
          isVisible: true,
        }) as { props: { startupCommand: string } }

        expect(result.props.startupCommand).toBe('')
      })
    })
  })
})
