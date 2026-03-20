import { describe, it, expect, vi, beforeEach } from 'vitest'
import { createStore } from 'zustand/vanilla'
import { commentsApplication } from './renderer'
import type { Tab } from '../../renderer/types'

// Mock React
vi.mock('react', () => ({
  createElement: vi.fn((component: unknown, props: unknown) => ({ component, props }))
}))

// Mock CommentsList component
vi.mock('../../renderer/components/CommentsList', () => ({
  default: vi.fn(() => null)
}))

import type { WorkspaceState } from '../../renderer/store/createWorkspaceStore'
const mockWorkspaceStore = createStore(() => ({} as WorkspaceState))

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
      expect(commentsApplication.canHaveMultiple).toBe(false)
      expect(commentsApplication.showInNewTabMenu).toBe(true)
      expect(commentsApplication.keepAlive).toBe(false)
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
          workspaceId: 'ws-1',
          workspacePath: '/test',
          isVisible: true,
          workspaceStore: mockWorkspaceStore
        })

        expect(result).toEqual({
          component: expect.any(Function),
          props: expect.objectContaining({
            key: 'tab-1',
            workspacePath: '/test',
            workspaceId: 'ws-1',
            workspaceStore: mockWorkspaceStore
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
          workspaceId: 'ws-1',
          workspacePath: '/test',
          isVisible: true,
          workspaceStore: mockWorkspaceStore
        })

        expect(result).toBeNull()
      })
    })
  })
})
