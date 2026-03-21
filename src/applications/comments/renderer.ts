import type { Application, CommentsState } from '../../renderer/types'
import { isCommentsState } from '../../renderer/types'
import CommentsList from '../../renderer/components/CommentsList'
import { createElement } from 'react'

export const commentsApplication: Application<CommentsState> = {
  id: 'comments',
  name: 'Comments',
  icon: '\u{1F4AC}',

  createInitialState: () => ({}),

  render: ({ tab, workspace }) => {
    if (!isCommentsState(tab.state)) {
      return null
    }
    return createElement(CommentsList, {
      key: tab.id,
      workspace,
    })
  },

  canClose: true,
  canHaveMultiple: false,
  showInNewTabMenu: true,
  keepAlive: false,
  displayStyle: 'flex',
  isDefault: false
}
