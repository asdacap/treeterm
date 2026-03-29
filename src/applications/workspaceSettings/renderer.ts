import type { Application } from '../../renderer/types'
import WorkspaceSettings from '../../renderer/components/WorkspaceSettings'
import { createElement } from 'react'

export const workspaceSettingsApplication: Application = {
  id: 'workspace-settings',
  name: 'Settings',
  icon: '\u2699',

  createInitialState: () => ({}),

  onWorkspaceLoad: () => ({ dispose: () => {} }),

  render: ({ tab, workspace }) =>
    createElement(WorkspaceSettings, { key: tab.id, workspace }),

  canClose: true,
  showInNewTabMenu: false,
  displayStyle: 'flex',
  isDefault: false
}
