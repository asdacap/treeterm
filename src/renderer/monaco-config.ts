/* eslint-disable custom/no-string-literal-comparison -- TODO: migrate existing string-literal comparisons to enums */
import { loader } from '@monaco-editor/react'
import * as monaco from 'monaco-editor'
import editorWorker from 'monaco-editor/esm/vs/editor/editor.worker?worker'
import jsonWorker from 'monaco-editor/esm/vs/language/json/json.worker?worker'
import cssWorker from 'monaco-editor/esm/vs/language/css/css.worker?worker'
import htmlWorker from 'monaco-editor/esm/vs/language/html/html.worker?worker'
import tsWorker from 'monaco-editor/esm/vs/language/typescript/ts.worker?worker'

// Configure Monaco worker environment
self.MonacoEnvironment = {
  getWorker(_: unknown, label: string) {
    if (label === 'json') {
      return new jsonWorker()
    }
    if (label === 'css' || label === 'scss' || label === 'less') {
      return new cssWorker()
    }
    if (label === 'html' || label === 'handlebars' || label === 'razor') {
      return new htmlWorker()
    }
    if (label === 'typescript' || label === 'javascript') {
      return new tsWorker()
    }
    return new editorWorker()
  }
}

// Configure Monaco to use the locally installed package instead of CDN
loader.config({ monaco })

// === Navigation bridge for go-to-definition across files ===
// Set by the active editor component (FileViewer/FileEditor) on mount.

export interface MonacoNavigationBridge {
  searchDefinition: ((symbol: string, language: string) => Promise<{ filePath: string; lineNumber: number }[]>) | null
  openFileAtLine: ((filePath: string, lineNumber: number) => void) | null
  getWorkspacePath: (() => string) | null
}

export const monacoNavigationBridge: MonacoNavigationBridge = {
  searchDefinition: null,
  openFileAtLine: null,
  getWorkspacePath: null,
}

// Register DefinitionProvider for common languages
const SUPPORTED_LANGUAGES = [
  'typescript', 'javascript', 'python', 'rust', 'go',
  'java', 'kotlin', 'ruby', 'php', 'c', 'cpp',
  'css', 'scss', 'less', 'html', 'shell',
]

const definitionProvider: monaco.languages.DefinitionProvider = {
  provideDefinition: async (model, position) => {
    if (!monacoNavigationBridge.searchDefinition || !monacoNavigationBridge.getWorkspacePath) {
      return null
    }

    const wordInfo = model.getWordAtPosition(position)
    if (!wordInfo) return null

    const language = model.getLanguageId()
    const results = await monacoNavigationBridge.searchDefinition(wordInfo.word, language)
    if (!results.length) return null

    const workspacePath = monacoNavigationBridge.getWorkspacePath()
    return results.map((result) => ({
      uri: monaco.Uri.file(workspacePath + '/' + result.filePath),
      range: new monaco.Range(result.lineNumber, 1, result.lineNumber, 1),
    }))
  },
}

for (const lang of SUPPORTED_LANGUAGES) {
  monaco.languages.registerDefinitionProvider(lang, definitionProvider)
}

// Register EditorOpener to intercept cross-file navigation (go-to-definition in another file)
monaco.editor.registerEditorOpener({
  openCodeEditor(_source, resource, selectionOrPosition) {
    if (!monacoNavigationBridge.openFileAtLine) return false

    const filePath = resource.path
    const line = selectionOrPosition
      ? ('startLineNumber' in selectionOrPosition ? selectionOrPosition.startLineNumber : selectionOrPosition.lineNumber)
      : 1
    monacoNavigationBridge.openFileAtLine(filePath, line)
    return true
  },
})
