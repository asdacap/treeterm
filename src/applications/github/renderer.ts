import type { Application, GitHubAppState } from '../../renderer/types'
import GitHubBrowser from '../../renderer/components/GitHubBrowser'
import { createElement } from 'react'

export const githubApplication: Application<GitHubAppState> = {
  id: 'github',
  name: 'GitHub',
  icon: '🐙',

  createInitialState: () => ({}),

  onWorkspaceLoad: () => ({ dispose: () => {} }),

  render: ({ tab, workspace, isVisible }) => {
    return createElement(GitHubBrowser, {
      key: tab.id,
      workspace,
      isVisible,
    })
  },

  canClose: true,
  showInNewTabMenu: true,
  displayStyle: 'flex',
  isDefault: false
}
