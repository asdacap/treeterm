import { describe, it, expect, vi, beforeEach } from 'vitest'
import { reviewApplication } from './renderer'
import type { Tab, Workspace } from '../../renderer/types'
import { createStore } from 'zustand/vanilla'
import type { WorkspaceStoreState, WorkspaceStore } from '../../renderer/store/createWorkspaceStore'
import type { GitControllerState } from '../../renderer/store/createGitControllerStore'
import type { ReviewCommentState } from '../../renderer/store/createReviewCommentStore'

// Mock React
vi.mock('react', () => ({
  createElement: vi.fn((component: unknown, props: unknown) => ({ component, props }))
}))

// Mock ReviewBrowser component
vi.mock('../../renderer/components/ReviewBrowser', () => ({
  default: vi.fn(() => null)
}))

const mockReviewCommentStore = createStore<ReviewCommentState>()(() => ({
  getReviewComments: vi.fn().mockReturnValue([]),
  addReviewComment: vi.fn(),
  deleteReviewComment: vi.fn(),
  toggleReviewCommentAddressed: vi.fn(),
  updateOutdatedReviewComments: vi.fn(),
  clearReviewComments: vi.fn(),
  markAllReviewCommentsAddressed: vi.fn(),
} as ReviewCommentState))

const mockGitControllerStore = createStore<GitControllerState>()(() => ({
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
  triggerRefresh: vi.fn(),
  dispose: vi.fn(),
} as GitControllerState))

function createMockWorkspaceStoreStateData(overrides?: Partial<WorkspaceStoreState>): WorkspaceStoreState {
  return {
    workspace: { id: 'ws-1', path: '/test' } as Workspace,
    addTab: vi.fn(), openOrFocusTab: vi.fn(),
    removeTab: vi.fn(),
    setActiveTab: vi.fn(),
    updateTabTitle: vi.fn(),
    updateTabState: vi.fn(),
    reviewComments: mockReviewCommentStore,
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
    initTab: vi.fn(),
    getTabRef: vi.fn().mockReturnValue(null),
    disposeTabResources: vi.fn(),
    initAnalyzer: vi.fn(),
    createTty: vi.fn().mockResolvedValue('pty-1'), getTtyWriter: vi.fn().mockResolvedValue({ write: vi.fn<(data: string) => void>(), kill: vi.fn<() => void>() }),
    connectionId: 'local',
    settings: { defaultApplicationId: '' },
  metadata: {},
  appStates: {},
  setWorkspace: vi.fn<(...args: any[]) => void>(),
    gitController: mockGitControllerStore,
    ...overrides,
  } as WorkspaceStoreState
}

const mockWorkspaceStore = createStore<WorkspaceStoreState>()(() => createMockWorkspaceStoreStateData())

describe('Review Renderer', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  describe('reviewApplication', () => {
    it('has correct application properties', () => {
      expect(reviewApplication.id).toBe('review')
      expect(reviewApplication.name).toBe('Review')
      expect(reviewApplication.icon).toBe('\u{1F4CB}')
      expect(reviewApplication.canClose).toBe(true)
      expect(reviewApplication.showInNewTabMenu).toBe(true)
      expect(reviewApplication.displayStyle).toBe('flex')
      expect(reviewApplication.isDefault).toBe(false)
    })

    describe('createInitialState', () => {
      it('returns empty object as initial state', () => {
        const state = reviewApplication.createInitialState()

        expect(state).toEqual({ viewMode: 'committed' })
      })

      it('returns a fresh state object on each call', () => {
        const state1 = reviewApplication.createInitialState()
        const state2 = reviewApplication.createInitialState()

        expect(state1).toEqual(state2)
        expect(state1).not.toBe(state2)
      })
    })

    describe('render', () => {
      it('renders ReviewBrowser component with correct props', () => {
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'review',
          title: 'Review',
          state: {}
        }

        const result = reviewApplication.render({
          tab,
          workspace: mockWorkspaceStore,
          isVisible: true,
        })

        expect(result).toEqual({
          component: expect.any(Function) as unknown,
          props: expect.objectContaining({
            key: 'tab-1',
            workspace: mockWorkspaceStore,
            tabId: 'tab-1',
            parentWorkspaceId: undefined
          }) as unknown,
        })
      })

      it('passes parentWorkspaceId when present in state', () => {
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'review',
          title: 'Review',
          state: {
            parentWorkspaceId: 'parent-ws-123'
          }
        }

        const result = reviewApplication.render({
          tab,
          workspace: mockWorkspaceStore,
          isVisible: true,
        }) as { props: { parentWorkspaceId: string } }

        expect(result.props.parentWorkspaceId).toBe('parent-ws-123')
      })

      it('returns null for invalid state', () => {
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'review',
          title: 'Review',
          state: null
        }

        const result = reviewApplication.render({
          tab,
          workspace: mockWorkspaceStore,
          isVisible: true,
        })

        expect(result).toBeNull()
      })

      it('returns null for non-object state', () => {
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'review',
          title: 'Review',
          state: 'invalid'
        }

        const result = reviewApplication.render({
          tab,
          workspace: mockWorkspaceStore,
          isVisible: true,
        })

        expect(result).toBeNull()
      })

      it('handles top-level worktree review (no parentWorkspaceId)', () => {
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'review',
          title: 'Review',
          state: {}
        }

        const result = reviewApplication.render({
          tab,
          workspace: mockWorkspaceStore,
          isVisible: true,
        }) as { props: { parentWorkspaceId: string | undefined } }

        expect(result.props.parentWorkspaceId).toBeUndefined()
      })

      it('passes workspace correctly', () => {
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'review',
          title: 'Review Changes',
          state: {
            parentWorkspaceId: 'main-branch'
          }
        }

        const wsHandle = createStore<WorkspaceStoreState>()(() => createMockWorkspaceStoreStateData({
          workspace: { id: 'feature-ws', path: '/workspace/feature' } as Workspace,
        }))

        const result = reviewApplication.render({
          tab,
          workspace: wsHandle,
          isVisible: true,
        }) as { props: { workspace: WorkspaceStore } }

        expect(result.props.workspace.getState().workspace.id).toBe('feature-ws')
        expect(result.props.workspace.getState().workspace.path).toBe('/workspace/feature')
      })

      it('renders regardless of isVisible flag', () => {
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'review',
          title: 'Review',
          state: {}
        }

        const visibleResult = reviewApplication.render({
          tab,
          workspace: mockWorkspaceStore,
          isVisible: true,
        })

        const hiddenResult = reviewApplication.render({
          tab,
          workspace: mockWorkspaceStore,
          isVisible: false,
        })

        expect(visibleResult).toBeDefined()
        expect(hiddenResult).toBeDefined()
      })

      it('handles state with non-string parentWorkspaceId gracefully', () => {
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'review',
          title: 'Review',
          state: {
            parentWorkspaceId: 123 as unknown
          }
        }

        const result = reviewApplication.render({
          tab,
          workspace: mockWorkspaceStore,
          isVisible: true,
        })

        // Should return null because parentWorkspaceId is not a string
        expect(result).toBeNull()
      })

      it('handles state with additional properties', () => {
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'review',
          title: 'Review',
          state: {
            parentWorkspaceId: 'parent-123',
            extraProperty: 'should-be-ignored'
          }
        }

        const result = reviewApplication.render({
          tab,
          workspace: mockWorkspaceStore,
          isVisible: true,
        }) as { props: { parentWorkspaceId: string } }

        expect(result.props.parentWorkspaceId).toBe('parent-123')
      })
    })

    describe('onWorkspaceLoad', () => {
      it('returns an AppRef with no-op dispose', () => {
        const tab: Tab = { id: 'tab-1', applicationId: 'review', title: 'Review', state: {} }
        const ref = reviewApplication.onWorkspaceLoad(tab, mockWorkspaceStore)
        expect(typeof ref.dispose).toBe('function')
        ref.dispose() // should not throw
      })
    })
  })
})
