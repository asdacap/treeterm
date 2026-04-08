import type { Application, WorkspaceStore } from '../../renderer/types'
import WorkspaceSettings from '../../renderer/components/WorkspaceSettings'
import { useAppStore } from '../../renderer/store/app'
import { createElement } from 'react'

function WorkspaceSettingsConnected({ tab, workspace }: { tab: { id: string }; workspace: WorkspaceStore }) {
  const applications = useAppStore((s) => s.applications)
  return createElement(WorkspaceSettings, { key: tab.id, workspace, applications })
}

export const workspaceSettingsApplication: Application = {
  id: 'workspace-settings',
  name: 'Settings',
  icon: '\u2699',

  createInitialState: () => ({}),

  onWorkspaceLoad: () => ({ close: () => {}, dispose: () => {} }),

  render: ({ tab, workspace }) =>
    createElement(WorkspaceSettingsConnected, { key: tab.id, tab, workspace }),

  canClose: true,
  showInNewTabMenu: false,
  displayStyle: 'flex',
  isDefault: false
}
