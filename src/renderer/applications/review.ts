import type { Application, ReviewState } from '../types'
import ReviewBrowser from '../components/ReviewBrowser'
import { createElement } from 'react'

export const reviewApplication: Application<ReviewState> = {
  id: 'review',
  name: 'Review',
  icon: '📋',

  createInitialState: () => ({
    parentWorkspaceId: ''
  }),

  render: ({ tab, workspaceId, workspacePath }) => {
    const state = tab.state as ReviewState
    return createElement(ReviewBrowser, {
      key: tab.id,
      workspaceId,
      workspacePath,
      tabId: tab.id,
      parentWorkspaceId: state.parentWorkspaceId
    })
  },

  canClose: true,
  canHaveMultiple: false,
  showInNewTabMenu: false,
  keepAlive: false,
  displayStyle: 'flex',
  isDefault: false
}
