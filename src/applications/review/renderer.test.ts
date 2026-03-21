import { describe, it, expect, vi, beforeEach } from 'vitest'
import { reviewApplication } from './renderer'
import type { Tab, Workspace, ReviewState } from '../../renderer/types'
import type { WorkspaceHandle } from '../../renderer/store/createWorkspaceStore'

// Mock React
vi.mock('react', () => ({
  createElement: vi.fn((component: any, props: any) => ({ component, props }))
}))

// Mock ReviewBrowser component
vi.mock('../../renderer/components/ReviewBrowser', () => ({
  default: vi.fn(() => null)
}))

const mockWorkspaceHandle = {
  id: 'ws-1',
  data: { path: '/test' } as Workspace,
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
} satisfies WorkspaceHandle

describe('Review Renderer', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  describe('reviewApplication', () => {
    it('has correct application properties', () => {
      expect(reviewApplication.id).toBe('review')
      expect(reviewApplication.name).toBe('Review')
      expect(reviewApplication.icon).toBe('📋')
      expect(reviewApplication.canClose).toBe(true)
      expect(reviewApplication.canHaveMultiple).toBe(false)
      expect(reviewApplication.showInNewTabMenu).toBe(true)
      expect(reviewApplication.keepAlive).toBe(false)
      expect(reviewApplication.displayStyle).toBe('flex')
      expect(reviewApplication.isDefault).toBe(false)
    })

    describe('createInitialState', () => {
      it('returns empty object as initial state', () => {
        const state = reviewApplication.createInitialState()

        expect(state).toEqual({})
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
          workspace: mockWorkspaceHandle,
          isVisible: true,
        })

        expect(result).toEqual({
          component: expect.any(Function),
          props: expect.objectContaining({
            key: 'tab-1',
            workspace: mockWorkspaceHandle,
            tabId: 'tab-1',
            parentWorkspaceId: undefined
          })
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
          workspace: mockWorkspaceHandle,
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
          workspace: mockWorkspaceHandle,
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
          workspace: mockWorkspaceHandle,
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
          workspace: mockWorkspaceHandle,
          isVisible: true,
        }) as { props: { parentWorkspaceId: string | undefined } }

        expect(result.props.parentWorkspaceId).toBeUndefined()
      })

      it('passes workspace correctly', () => {
        const tab: Tab = {
          id: 'review-tab',
          applicationId: 'review',
          title: 'Review Changes',
          state: {
            parentWorkspaceId: 'main-branch'
          }
        }

        const wsHandle = { ...mockWorkspaceHandle, id: 'feature-ws', data: { path: '/workspace/feature' } as Workspace }

        const result = reviewApplication.render({
          tab,
          workspace: wsHandle,
          isVisible: true,
        }) as { props: { workspace: WorkspaceHandle } }

        expect(result.props.workspace.id).toBe('feature-ws')
        expect(result.props.workspace.data.path).toBe('/workspace/feature')
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
          workspace: mockWorkspaceHandle,
          isVisible: true,
        })

        const hiddenResult = reviewApplication.render({
          tab,
          workspace: mockWorkspaceHandle,
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
            parentWorkspaceId: 123 as any
          }
        }

        const result = reviewApplication.render({
          tab,
          workspace: mockWorkspaceHandle,
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
          workspace: mockWorkspaceHandle,
          isVisible: true,
        }) as { props: { parentWorkspaceId: string } }

        expect(result.props.parentWorkspaceId).toBe('parent-123')
      })
    })

    describe('cleanup', () => {
      it('cleanup is undefined for review application', () => {
        // Review application doesn't need cleanup
        expect(reviewApplication.cleanup).toBeUndefined()
      })
    })
  })
})
