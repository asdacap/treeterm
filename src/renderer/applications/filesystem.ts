import type { Application, FilesystemState } from '../types'
import { FilesystemBrowser } from '../components/FilesystemBrowser'
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

  canClose: false,
  canHaveMultiple: false,
  showInNewTabMenu: false,
  keepAlive: false,
  displayStyle: 'flex'
}
