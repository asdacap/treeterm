import type { Application, ReviewState } from '../../renderer/types'
import { isReviewState } from '../../renderer/types'
import ReviewBrowser from '../../renderer/components/ReviewBrowser'
import { createElement } from 'react'

export const reviewApplication: Application<ReviewState> = {
  id: 'review',
  name: 'Review',
  icon: '📋',

  createInitialState: () => ({
    parentWorkspaceId: ''
  }),

  render: ({ tab, workspaceId, workspacePath }) => {
    if (!isReviewState(tab.state)) {
      return null
    }
    return createElement(ReviewBrowser, {
      key: tab.id,
      workspaceId,
      workspacePath,
      tabId: tab.id,
      parentWorkspaceId: tab.state.parentWorkspaceId
    })
  },

  canClose: true,
  canHaveMultiple: false,
  showInNewTabMenu: false,
  keepAlive: false,
  displayStyle: 'flex',
  isDefault: false
}
