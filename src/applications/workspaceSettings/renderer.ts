import type { Application } from '../../renderer/types'
import WorkspaceSettings from '../../renderer/components/WorkspaceSettings'
import { createElement } from 'react'

export const workspaceSettingsApplication: Application = {
  id: 'workspace-settings',
  name: 'Settings',
  icon: '\u2699',

  createInitialState: () => ({}),

  onWorkspaceLoad: () => {},

  render: ({ tab, workspace }) =>
    createElement(WorkspaceSettings, { key: tab.id, workspace }),

  canClose: true,
  canHaveMultiple: false,
  showInNewTabMenu: true,
  displayStyle: 'flex',
  isDefault: false
}
