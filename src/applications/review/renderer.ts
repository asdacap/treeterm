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

  onWorkspaceLoad: () => {},

  render: ({ tab, workspace, isVisible }) => {
    if (!isReviewState(tab.state)) {
      return null
    }
    return createElement(ReviewBrowser, {
      key: tab.id,
      workspace,
      tabId: tab.id,
      parentWorkspaceId: tab.state.parentWorkspaceId,
      isVisible,
    })
  },

  canClose: true,
  canHaveMultiple: false,
  showInNewTabMenu: true,
  displayStyle: 'flex',
  isDefault: false
}
