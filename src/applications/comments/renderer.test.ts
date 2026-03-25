import { describe, it, expect, vi, beforeEach } from 'vitest'
import { commentsApplication } from './renderer'
import type { Tab, Workspace } from '../../renderer/types'
import { createStore } from 'zustand/vanilla'
import type { WorkspaceStoreState } from '../../renderer/store/createWorkspaceStore'

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
  addTab: vi.fn(),
  removeTab: vi.fn(),
  setActiveTab: vi.fn(),
  updateTabTitle: vi.fn(),
  updateTabState: vi.fn(),
  getReviewComments: vi.fn(),
  addReviewComment: vi.fn(),
  deleteReviewComment: vi.fn(),
  toggleReviewCommentAddressed: vi.fn(),
  updateOutdatedReviewComments: vi.fn(),
  clearReviewComments: vi.fn(),
  markAllReviewCommentsAddressed: vi.fn(),
  promptHarness: vi.fn(),
  quickForkWorkspace: vi.fn(),
  updateMetadata: vi.fn(),
  updateStatus: vi.fn(),
  refreshGitInfo: vi.fn(),
  mergeAndRemove: vi.fn(),
  closeAndClean: vi.fn(),
  lookupWorkspace: vi.fn(),
  remove: vi.fn(),
  removeKeepBranch: vi.fn(),
  removeKeepWorktree: vi.fn(),
  removeKeepBoth: vi.fn(),
  getGitApi: vi.fn(),
  getFilesystemApi: vi.fn(),
  initTab: vi.fn(),
  getTabRef: vi.fn().mockReturnValue(null),
  initAnalyzer: vi.fn(),
  createTty: vi.fn().mockResolvedValue('pty-1'),
  connectionId: 'local',
  updateSettings: vi.fn(),
  hasUncommittedChanges: false,
  isDiffCleanFromParent: false,
  hasConflictsWithParent: false,
  disposeGitController: vi.fn(),
  focusTabId: null,
  requestFocus: vi.fn(),
  clearFocusRequest: vi.fn(),
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
          component: expect.any(Function),
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
