import type { Application, EditorState } from '../../renderer/types'
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

  onWorkspaceLoad: () => ({ dispose: () => {} }),

  render: ({ tab, workspace }) => createElement(FileEditor, {
    key: tab.id,
    workspace,
    tabId: tab.id,
  }),

  canClose: true,
  showInNewTabMenu: false,
  displayStyle: 'flex',
  isDefault: false
}
