import type { Application, ReviewState } from '../../renderer/types'
import { isReviewState } from '../../renderer/types'
import ReviewBrowser from '../../renderer/components/ReviewBrowser'
import { createElement } from 'react'

export const reviewApplication: Application<ReviewState> = {
  id: 'review',
  name: 'Review',
  icon: '📋',

  createInitialState: () => ({
    // parentWorkspaceId is optional - null means top-level worktree (no merge parent)
  }),

  render: ({ tab, workspaceId, workspacePath, workspaceStore }) => {
    if (!isReviewState(tab.state)) {
      return null
    }
    return createElement(ReviewBrowser, {
      key: tab.id,
      workspaceId,
      workspacePath,
      tabId: tab.id,
      parentWorkspaceId: tab.state.parentWorkspaceId,
      workspaceStore
    })
  },

  canClose: true,
  canHaveMultiple: false,
  showInNewTabMenu: true,
  keepAlive: false,
  displayStyle: 'flex',
  isDefault: false
}
