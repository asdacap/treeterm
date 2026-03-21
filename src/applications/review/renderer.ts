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

  render: ({ tab, workspace }) => {
    if (!isReviewState(tab.state)) {
      return null
    }
    return createElement(ReviewBrowser, {
      key: tab.id,
      workspace,
      tabId: tab.id,
      parentWorkspaceId: tab.state.parentWorkspaceId,
    })
  },

  canClose: true,
  canHaveMultiple: false,
  showInNewTabMenu: true,
  keepAlive: false,
  displayStyle: 'flex',
  isDefault: false
}
