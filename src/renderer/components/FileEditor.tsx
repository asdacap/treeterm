import { useEffect, useRef, useCallback, useState } from 'react'
import Editor, { OnMount, OnChange } from '@monaco-editor/react'
import type { editor } from 'monaco-editor'
import { useWorkspaceStore } from '../store/workspace'
import type { EditorState } from '../types'
import { MarkdownPreview } from './MarkdownPreview'

interface FileEditorProps {
  workspaceId: string
  workspacePath: string
  tabId: string
}

function mapLanguageToMonaco(language: string): string {
  const languageMap: Record<string, string> = {
    bash: 'shell'
  }
  return languageMap[language] || language
}

function getFilename(filePath: string): string {
  return filePath.split('/').pop() || filePath
}

export function FileEditor({ workspaceId, workspacePath, tabId }: FileEditorProps): JSX.Element {
  const { workspaces, updateTabState, updateTabTitle } = useWorkspaceStore()
  const workspace = workspaces[workspaceId]
  const tab = workspace?.tabs.find((t) => t.id === tabId)
  const state = tab?.state as EditorState | undefined

  const editorRef = useRef<editor.IStandaloneCodeEditor | null>(null)
  const [saving, setSaving] = useState(false)

  // Load file content on mount
  useEffect(() => {
    if (!state?.filePath || state.isLoading) return

    const loadFile = async () => {
      updateTabState<EditorState>(workspaceId, tabId, (s) => ({
        ...s,
        isLoading: true,
        error: null
      }))

      try {
        const result = await window.electron.filesystem.readFile(workspacePath, state.filePath)

        if (result.success && result.file) {
          const language = mapLanguageToMonaco(result.file.language)
          const isMarkdown = language === 'markdown'

          updateTabState<EditorState>(workspaceId, tabId, (s) => ({
            ...s,
            originalContent: result.file!.content,
            currentContent: result.file!.content,
            language,
            viewMode: isMarkdown ? 'preview' : 'editor',
            isLoading: false,
            isDirty: false
          }))

          updateTabTitle(workspaceId, tabId, getFilename(state.filePath))
        } else {
          updateTabState<EditorState>(workspaceId, tabId, (s) => ({
            ...s,
            isLoading: false,
            error: result.error || 'Failed to load file'
          }))
        }
      } catch (err) {
        updateTabState<EditorState>(workspaceId, tabId, (s) => ({
          ...s,
          isLoading: false,
          error: `Error loading file: ${err}`
        }))
      }
    }

    if (!state.originalContent && !state.error) {
      loadFile()
    }
  }, [state?.filePath, workspacePath, workspaceId, tabId, updateTabState, updateTabTitle])

  // Update tab title with dirty indicator
  useEffect(() => {
    if (!state?.filePath) return
    const filename = getFilename(state.filePath)
    const title = state.isDirty ? `${filename} \u2022` : filename
    updateTabTitle(workspaceId, tabId, title)
  }, [state?.isDirty, state?.filePath, workspaceId, tabId, updateTabTitle])

  const handleSave = useCallback(async () => {
    if (!state || !state.isDirty || saving) return

    setSaving(true)
    try {
      const result = await window.electron.filesystem.writeFile(
        workspacePath,
        state.filePath,
        state.currentContent
      )

      if (result.success) {
        updateTabState<EditorState>(workspaceId, tabId, (s) => ({
          ...s,
          originalContent: s.currentContent,
          isDirty: false
        }))
      } else {
        alert(`Failed to save: ${result.error}`)
      }
    } catch (err) {
      alert(`Error saving file: ${err}`)
    } finally {
      setSaving(false)
    }
  }, [state, saving, workspacePath, workspaceId, tabId, updateTabState])

  const handleEditorMount: OnMount = useCallback(
    (editor) => {
      editorRef.current = editor

      // Add Cmd/Ctrl+S keybinding for save
      editor.addCommand(
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        (window as any).monaco.KeyMod.CtrlCmd | (window as any).monaco.KeyCode.KeyS,
        () => handleSave()
      )
    },
    [handleSave]
  )

  const handleEditorChange: OnChange = useCallback(
    (value) => {
      if (value === undefined) return

      updateTabState<EditorState>(workspaceId, tabId, (s) => ({
        ...s,
        currentContent: value,
        isDirty: value !== s.originalContent
      }))
    },
    [workspaceId, tabId, updateTabState]
  )

  const toggleViewMode = useCallback(() => {
    updateTabState<EditorState>(workspaceId, tabId, (s) => ({
      ...s,
      viewMode: s.viewMode === 'preview' ? 'editor' : 'preview'
    }))
  }, [workspaceId, tabId, updateTabState])

  if (!state) {
    return <div className="file-editor-error">Invalid tab</div>
  }

  if (state.isLoading) {
    return (
      <div className="file-editor">
        <div className="file-editor-placeholder">Loading...</div>
      </div>
    )
  }

  if (state.error) {
    return (
      <div className="file-editor">
        <div className="file-editor-error">{state.error}</div>
      </div>
    )
  }

  const filename = getFilename(state.filePath)
  const isMarkdown = state.language === 'markdown'

  return (
    <div className="file-editor">
      <div className="file-editor-header">
        <span className="file-editor-filename">
          {filename}
          {state.isDirty && <span className="file-editor-dirty"> \u2022</span>}
        </span>
        <div className="file-editor-actions">
          {isMarkdown && (
            <button
              className="file-editor-toggle-btn"
              onClick={toggleViewMode}
              title={state.viewMode === 'preview' ? 'Edit' : 'Preview'}
            >
              {state.viewMode === 'preview' ? '\u270F Edit' : '\uD83D\uDC41 Preview'}
            </button>
          )}
          <button
            className="file-editor-save-btn"
            onClick={handleSave}
            disabled={!state.isDirty || saving}
            title="Save (Cmd+S)"
          >
            {saving ? 'Saving...' : 'Save'}
          </button>
          <span className="file-editor-language">{state.language}</span>
        </div>
      </div>
      <div className="file-editor-content">
        {isMarkdown && state.viewMode === 'preview' ? (
          <MarkdownPreview content={state.currentContent} />
        ) : (
          <Editor
          height="100%"
          language={state.language}
          value={state.currentContent}
          theme="vs-dark"
          onMount={handleEditorMount}
          onChange={handleEditorChange}
          options={{
            readOnly: false,
            minimap: { enabled: true },
            lineNumbers: 'on',
            scrollBeyondLastLine: false,
            wordWrap: 'on',
            fontSize: 14,
            folding: true,
            renderLineHighlight: 'line',
            scrollbar: {
              vertical: 'auto',
              horizontal: 'auto'
            },
            padding: { top: 8 }
          }}
        />
        )}
      </div>
    </div>
  )
}
