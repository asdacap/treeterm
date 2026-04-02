import { describe, it, expect, vi, beforeEach } from 'vitest'
import { createAiHarnessVariant, type AiHarnessRef } from './renderer'
import type { Tab, Workspace, AiHarnessInstance } from '../../renderer/types'
import { createMockGitApi, createMockFilesystemApi, createMockRunActionsApi } from '../../shared/mockApis'
import { createStore } from 'zustand/vanilla'
import type { WorkspaceStoreState } from '../../renderer/store/createWorkspaceStore'
import type { GitControllerState } from '../../renderer/store/createGitControllerStore'
import type { ReviewCommentState } from '../../renderer/store/createReviewCommentStore'

// Mock React
vi.mock('react', () => ({
  createElement: vi.fn((component: any, props: any) => ({ component, props }))
}))

// Mock AiHarness component
vi.mock('../../renderer/components/AiHarness', () => ({
  default: vi.fn(() => null)
}))

const mockTerminalKill = vi.fn()
const mockDeps = { terminal: { kill: mockTerminalKill } }

const mockWorkspaceStoreStateData = {
  workspace: { id: 'ws-1', path: '/test' } as Workspace,
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
  initTab: vi.fn(),
  getTabRef: vi.fn().mockReturnValue(null),
  getCachedTerminal: vi.fn().mockReturnValue(null),
  setCachedTerminal: vi.fn(),
  disposeCachedTerminal: vi.fn(), disposeAllCachedTerminals: vi.fn(),
  initAnalyzer: vi.fn(),
  createTty: vi.fn().mockResolvedValue('pty-1'), getTtyWriter: vi.fn().mockResolvedValue({ write: vi.fn(), kill: vi.fn() }),
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

describe('AI Harness Renderer', () => {
  const mockInstance: AiHarnessInstance = {
    id: 'claude',
    name: 'Claude',
    icon: '\u2726',
    command: 'claude',
    isDefault: false,
    enableSandbox: true,
    allowNetwork: false,
    backgroundColor: '#1a1a24',
    disableScrollbar: true,
    stripScrollbackClear: false
  }

  beforeEach(() => {
    vi.clearAllMocks()
  })

  describe('createAiHarnessVariant', () => {
    it('creates AI Harness variant with correct properties', () => {
      const app = createAiHarnessVariant(mockInstance, mockDeps)

      expect(app.id).toBe('aiharness-claude')
      expect(app.name).toBe('Claude')
      expect(app.icon).toBe('\u2726')
      expect(app.canClose).toBe(true)
      expect(app.showInNewTabMenu).toBe(true)
      expect(app.displayStyle).toBe('flex')
    })

    it('sets isDefault from instance', () => {
      const app = createAiHarnessVariant(mockInstance, mockDeps)
      expect(app.isDefault).toBe(false)

      const defaultApp = createAiHarnessVariant({
        ...mockInstance,
        isDefault: true
      }, mockDeps)
      expect(defaultApp.isDefault).toBe(true)
    })

    describe('createInitialState', () => {
      it('returns AI Harness state with sandbox configuration', () => {
        const app = createAiHarnessVariant(mockInstance, mockDeps)
        const state = app.createInitialState()

        expect(state).toEqual({
          ptyId: null,
          ptyHandle: null,
          keepOnExit: false,
          sandbox: {
            enabled: true,
            allowNetwork: false,
            allowedPaths: []
          },
          autoApprove: false,
        })
      })

      it('uses instance sandbox settings', () => {
        const appWithoutSandbox = createAiHarnessVariant({
          ...mockInstance,
          enableSandbox: false,
          allowNetwork: true
        }, mockDeps)
        const state = appWithoutSandbox.createInitialState()

        expect(state.sandbox.enabled).toBe(false)
        expect(state.sandbox.allowNetwork).toBe(true)
      })

      it('always initializes allowedPaths as empty array', () => {
        const app = createAiHarnessVariant(mockInstance, mockDeps)
        const state = app.createInitialState()

        expect(state.sandbox.allowedPaths).toEqual([])
      })
    })

    describe('onWorkspaceLoad and dispose', () => {
      const mockAnalyzerState = { start: vi.fn(), stop: vi.fn(), getHistory: vi.fn().mockReturnValue([]) }
      const mockAnalyzer = { getState: vi.fn().mockReturnValue(mockAnalyzerState), setState: vi.fn(), subscribe: vi.fn() }

      it('starts analyzer directly when tab has existing ptyId (restore path)', () => {
        const mockAnalyzerState = { start: vi.fn(), stop: vi.fn(), getHistory: vi.fn().mockReturnValue([]) }
        const mockAnalyzer = { getState: vi.fn().mockReturnValue(mockAnalyzerState), setState: vi.fn(), subscribe: vi.fn() }

        const app = createAiHarnessVariant(mockInstance, mockDeps)
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'aiharness-claude',
          title: 'Claude',
          state: {
            ptyId: 'pty-existing',
            sandbox: { enabled: true, allowNetwork: false, allowedPaths: [] }
          }
        }

        const store = createStore<WorkspaceStoreState>()(() => ({
          ...mockWorkspaceStoreStateData,
          initAnalyzer: vi.fn().mockReturnValue(mockAnalyzer),
        }))

        app.onWorkspaceLoad(tab, store)

        expect(mockAnalyzerState.start).toHaveBeenCalledWith('pty-existing')
        expect(mockWorkspaceStoreStateData.createTty).not.toHaveBeenCalled()
      })

      it('creates PTY and starts analyzer for new tab (no ptyId)', async () => {
        const mockAnalyzerState = { start: vi.fn(), stop: vi.fn(), getHistory: vi.fn().mockReturnValue([]) }
        const mockAnalyzer = { getState: vi.fn().mockReturnValue(mockAnalyzerState), setState: vi.fn(), subscribe: vi.fn() }

        const app = createAiHarnessVariant(mockInstance, mockDeps)
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'aiharness-claude',
          title: 'Claude',
          state: {
            ptyId: null,
            sandbox: { enabled: true, allowNetwork: false, allowedPaths: [] }
          }
        }

        let resolveTty!: (value: string) => void
        const ttyPromise = new Promise<string>((resolve) => { resolveTty = resolve })
        const mockCreateTty = vi.fn().mockReturnValue(ttyPromise)
        const mockUpdateTabState = vi.fn()
        const store = createStore<WorkspaceStoreState>()(() => ({
          ...mockWorkspaceStoreStateData,
          initAnalyzer: vi.fn().mockReturnValue(mockAnalyzer),
          createTty: mockCreateTty,
          updateTabState: mockUpdateTabState,
        }))

        app.onWorkspaceLoad(tab, store)

        resolveTty('pty-new')
        await ttyPromise
        await new Promise(resolve => setTimeout(resolve, 0))

        expect(mockCreateTty).toHaveBeenCalledWith(
          '/test',
          { enabled: true, allowNetwork: false, allowedPaths: [] },
          'claude'
        )
        expect(mockUpdateTabState).toHaveBeenCalledWith('tab-1', expect.any(Function))
        expect(mockAnalyzerState.start).toHaveBeenCalledWith('pty-new')

        const updater = mockUpdateTabState.mock.calls[0][1]
        const updated = updater({ ptyId: null, sandbox: { enabled: true, allowNetwork: false, allowedPaths: [] } })
        expect(updated).toEqual({
          ptyId: 'pty-new',
          sandbox: { enabled: true, allowNetwork: false, allowedPaths: [] },
          connectionId: 'local',
        })
      })

      it('dispose kills PTY when tab has ptyId', () => {
        const app = createAiHarnessVariant(mockInstance, mockDeps)
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'aiharness-claude',
          title: 'Claude',
          state: {
            ptyId: 'pty-789',
            sandbox: { enabled: true, allowNetwork: false, allowedPaths: [] }
          }
        }

        const store = createStore<WorkspaceStoreState>()(() => ({
          ...mockWorkspaceStoreStateData,
          initAnalyzer: vi.fn().mockReturnValue(mockAnalyzer),
        }))

        const ref = app.onWorkspaceLoad(tab, store)
        ref.dispose()

        expect(mockTerminalKill).toHaveBeenCalledWith('local', 'pty-789')
      })

      it('dispose does not kill PTY when tab has no ptyId', () => {
        const app = createAiHarnessVariant(mockInstance, mockDeps)
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'aiharness-claude',
          title: 'Claude',
          state: {
            ptyId: null,
            sandbox: { enabled: true, allowNetwork: false, allowedPaths: [] }
          }
        }

        const store = createStore<WorkspaceStoreState>()(() => ({
          ...mockWorkspaceStoreStateData,
          initAnalyzer: vi.fn().mockReturnValue(mockAnalyzer),
        }))

        const ref = app.onWorkspaceLoad(tab, store)
        ref.dispose()

        expect(mockTerminalKill).not.toHaveBeenCalled()
      })

      it('returns ref with analyzer', () => {
        const app = createAiHarnessVariant(mockInstance, mockDeps)
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'aiharness-claude',
          title: 'Claude',
          state: {
            ptyId: null,
            sandbox: { enabled: true, allowNetwork: false, allowedPaths: [] }
          }
        }

        const store = createStore<WorkspaceStoreState>()(() => ({
          ...mockWorkspaceStoreStateData,
          initAnalyzer: vi.fn().mockReturnValue(mockAnalyzer),
        }))

        const ref = app.onWorkspaceLoad(tab, store) as AiHarnessRef

        expect(typeof ref.dispose).toBe('function')
        expect(ref.analyzer).toBe(mockAnalyzer)
      })
    })

    describe('render', () => {
      it('renders AiHarness component with correct props', () => {
        const app = createAiHarnessVariant(mockInstance, mockDeps)
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'aiharness-claude',
          title: 'Claude',
          state: {
            ptyId: null,
            sandbox: { enabled: true, allowNetwork: false, allowedPaths: [] }
          }
        }

        const result = app.render({
          tab,
          workspace: mockWorkspaceStore,
          isVisible: true,
        })

        expect(result).toEqual({
          component: expect.any(Function),
          props: expect.objectContaining({
            key: 'tab-1',
            cwd: '/test',
            workspace: mockWorkspaceStore,
            tabId: 'tab-1',
            sandbox: { enabled: true, allowNetwork: false, allowedPaths: [] },
            isVisible: true,
            command: 'claude',
            backgroundColor: '#1a1a24',
            disableScrollbar: true
          })
        })
      })

      it('returns null for invalid state', () => {
        const app = createAiHarnessVariant(mockInstance, mockDeps)
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'aiharness-claude',
          title: 'Claude',
          state: { invalid: true } // Not a valid AiHarnessState
        }

        const result = app.render({
          tab,
          workspace: mockWorkspaceStore,
          isVisible: true,
        })

        expect(result).toBeNull()
      })

      it('passes instance properties to component', () => {
        const app = createAiHarnessVariant(mockInstance, mockDeps)
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'aiharness-claude',
          title: 'Claude',
          state: {
            ptyId: null,
            sandbox: { enabled: true, allowNetwork: false, allowedPaths: [] }
          }
        }

        const result = app.render({
          tab,
          workspace: mockWorkspaceStore,
          isVisible: false,
        }) as { props: Record<string, unknown> }

        expect(result.props.command).toBe('claude')
        expect(result.props.backgroundColor).toBe('#1a1a24')
        expect(result.props.disableScrollbar).toBe(true)
      })

      it('passes false for disableScrollbar when false', () => {
        const appWithoutScrollbarOption = createAiHarnessVariant({
          ...mockInstance,
          disableScrollbar: false
        }, mockDeps)
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'aiharness-claude',
          title: 'Claude',
          state: {
            ptyId: null,
            sandbox: { enabled: true, allowNetwork: false, allowedPaths: [] }
          }
        }

        const result = appWithoutScrollbarOption.render({
          tab,
          workspace: mockWorkspaceStore,
          isVisible: true,
        }) as { props: Record<string, unknown> }

        expect(result.props.disableScrollbar).toBe(false)
      })
    })

    it('does not define getActivityState (activity state is in analyzer store)', () => {
      const app = createAiHarnessVariant(mockInstance, mockDeps)
      expect(app.getActivityState).toBeUndefined()
    })
  })
})
