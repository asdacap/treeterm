import type { Application, EditorState, Tab, Workspace } from '../../renderer/types'
import { FileEditor } from '../../renderer/components/FileEditor'
import { createElement } from 'react'

export const editorApplication: Application<EditorState> = {
  id: 'editor',
  name: 'Editor',
  icon: '\u270F',

  createInitialState: () => ({
    filePath: '',
    originalContent: '',
    currentContent: '',
    language: 'plaintext',
    isDirty: false,
    viewMode: 'editor',
    isLoading: false,
    error: null
  }),

  cleanup: async (tab: Tab, _workspace: Workspace) => {
    const state = tab.state as EditorState
    if (state.isDirty) {
      console.warn('Editor tab closed with unsaved changes')
    }
  },

  render: ({ tab, workspaceId, workspacePath }) => {
    return createElement(FileEditor, {
      key: tab.id,
      workspaceId,
      workspacePath,
      tabId: tab.id
    })
  },

  canClose: true,
  canHaveMultiple: true,
  showInNewTabMenu: false,
  keepAlive: false,
  displayStyle: 'flex',
  isDefault: false
}
