import { describe, it, expect, vi, beforeEach } from 'vitest'
import { commentsApplication } from './renderer'
import type { Tab, Workspace } from '../../renderer/types'
import { createMockGitApi, createMockFilesystemApi, createMockRunActionsApi, createMockExecApi, createMockWorktreeRegistryApi } from '../../shared/mockApis'
import { createStore } from 'zustand/vanilla'
import type { WorkspaceStoreState } from '../../renderer/store/createWorkspaceStore'
import type { GitControllerState } from '../../renderer/store/createGitControllerStore'
import type { ReviewCommentState } from '../../renderer/store/createReviewCommentStore'

// Mock React
vi.mock('react', () => ({
  createElement: vi.fn((component: unknown, props: unknown) => ({ component, props }))
}))

// Mock CommentsList component
vi.mock('../../renderer/components/CommentsList', () => ({
  default: vi.fn(() => null)
}))

const mockWorkspaceStoreStateData = {
  workspace: { id: 'ws-1', path: '/test' } as Workspace,
  addTab: vi.fn(), openOrFocusTab: vi.fn(),
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
  worktreeRegistryApi: createMockWorktreeRegistryApi(),
  saveRegistryEntry: vi.fn(),
  initTab: vi.fn(),
  getTabRef: vi.fn().mockReturnValue(null),
  disposeTabResources: vi.fn(),
  initAnalyzer: vi.fn(),
  createTty: vi.fn().mockResolvedValue('pty-1'), getTtyWriter: vi.fn().mockResolvedValue({ write: vi.fn<(data: string) => void>(), kill: vi.fn<() => void>() }),
  connectionId: 'local',
  updateSettings: vi.fn(),
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
    triggerRefresh: vi.fn(),
    dispose: vi.fn(),
  } as GitControllerState)),
} as WorkspaceStoreState

const mockWorkspaceStore = createStore<WorkspaceStoreState>()(() => mockWorkspaceStoreStateData)

describe('Comments Renderer', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  describe('commentsApplication', () => {
    it('has correct application properties', () => {
      expect(commentsApplication.id).toBe('comments')
      expect(commentsApplication.name).toBe('Comments')
      expect(commentsApplication.icon).toBe('\u{1F4AC}')
      expect(commentsApplication.canClose).toBe(true)
      expect(commentsApplication.showInNewTabMenu).toBe(true)
      expect(commentsApplication.displayStyle).toBe('flex')
      expect(commentsApplication.isDefault).toBe(false)
    })

    describe('createInitialState', () => {
      it('returns empty object', () => {
        expect(commentsApplication.createInitialState()).toEqual({})
      })

      it('returns a fresh object on each call', () => {
        const s1 = commentsApplication.createInitialState()
        const s2 = commentsApplication.createInitialState()
        expect(s1).toEqual(s2)
        expect(s1).not.toBe(s2)
      })
    })

    describe('onWorkspaceLoad', () => {
      it('returns disposable ref', () => {
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'comments',
          title: 'Comments',
          state: {}
        }

        const ref = commentsApplication.onWorkspaceLoad(tab, mockWorkspaceStore)
        expect(typeof ref.dispose).toBe('function')
        ref.dispose()
      })
    })

    describe('render', () => {
      it('renders CommentsList with correct props for valid state', () => {
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'comments',
          title: 'Comments',
          state: {}
        }

        const result = commentsApplication.render({
          tab,
          workspace: mockWorkspaceStore,
          isVisible: true,
        })

        expect(result).toEqual({
          // eslint-disable-next-line @typescript-eslint/no-unsafe-assignment
          component: expect.any(Function),
          // eslint-disable-next-line @typescript-eslint/no-unsafe-assignment
          props: expect.objectContaining({
            key: 'tab-1',
            workspace: mockWorkspaceStore,
          })
        })
      })

      it('returns null for invalid state (null)', () => {
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'comments',
          title: 'Comments',
          state: null
        }

        const result = commentsApplication.render({
          tab,
          workspace: mockWorkspaceStore,
          isVisible: true,
        })

        expect(result).toBeNull()
      })
    })
  })
})
