import type { Application, FilesystemState } from '../../renderer/types'
import { FilesystemBrowser } from '../../renderer/components/FilesystemBrowser'
import { createElement } from 'react'

export const filesystemApplication: Application<FilesystemState> = {
  id: 'filesystem',
  name: 'Files',
  icon: '\uD83D\uDCC2',

  createInitialState: () => ({
    selectedPath: null,
    expandedDirs: []
  }),

  render: ({ tab, workspaceId, workspacePath }) => {
    return createElement(FilesystemBrowser, {
      key: tab.id,
      workspacePath,
      workspaceId,
      tabId: tab.id
    })
  },

  canClose: true,
  canHaveMultiple: true,
  showInNewTabMenu: true,
  keepAlive: false,
  displayStyle: 'flex',
  isDefault: true
}
