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

  render: ({ tab, workspace }) => {
    return createElement(FilesystemBrowser, {
      key: tab.id,
      workspace,
      tabId: tab.id,
    })
  },

  canClose: true,
  canHaveMultiple: true,
  showInNewTabMenu: true,
  displayStyle: 'flex',
  isDefault: true
}
