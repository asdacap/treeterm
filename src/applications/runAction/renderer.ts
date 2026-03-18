import type { Application } from '../../renderer/types'
import { createElement } from 'react'
import RunActionPanel from '../../renderer/components/RunActionPanel'

export const runActionApplication: Application = {
  id: 'run-action',
  name: 'Run Actions',
  icon: '▶',

  createInitialState: () => ({}),

  render: ({ workspacePath, isVisible }) => {
    return createElement(RunActionPanel, {
      workspacePath,
      isVisible
    })
  },

  canClose: true,
  canHaveMultiple: false,
  showInNewTabMenu: true,
  keepAlive: false,
  displayStyle: 'flex',
  isDefault: false
}
