import { useEffect, useRef, useCallback, useState } from 'react'
import Editor, { OnMount, OnChange } from '@monaco-editor/react'
import { editor, KeyMod, KeyCode } from 'monaco-editor'
import { useStore } from 'zustand'
import type { EditorState, WorkspaceStore } from '../types'
import { useFilesystemApi } from '../hooks/useWorkspaceApis'
import { MarkdownPreview } from './MarkdownPreview'

interface FileEditorProps {
  workspace: WorkspaceStore
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

export function FileEditor({ workspace, tabId }: FileEditorProps): JSX.Element {
  const { workspace: wsData, updateTabState, updateTabTitle } = useStore(workspace)
  const filesystem = useFilesystemApi(workspace)
  const appState = wsData?.appStates[tabId]
  const state = appState?.state as EditorState | undefined

  const scrollTop = state?.status === 'ready' ? state.scrollTop ?? 0 : 0
  const lastScrollTopRef = useRef<number>(scrollTop)
  const [saving, setSaving] = useState(false)

  // Load file content on mount
  useEffect(() => {
    if (!state?.filePath || state.status === 'loading') return

    const loadFile = async () => {
      updateTabState<EditorState>(tabId, () => ({
        status: 'loading',
        filePath: state.filePath
      }))

      try {
        const result = await filesystem.readFile(state.filePath)

        if (result.success) {
          const language = mapLanguageToMonaco(result.file.language)

          updateTabState<EditorState>(tabId, () => ({
            status: 'ready',
            filePath: state.filePath,
            originalContent: result.file.content,
            currentContent: result.file.content,
            language,
            viewMode: language === 'markdown' ? 'preview' : 'editor',
            isDirty: false,
          }))

          updateTabTitle(tabId, getFilename(state.filePath))
        } else {
          updateTabState<EditorState>(tabId, () => ({
            status: 'error',
            filePath: state.filePath,
            error: result.error
          }))
        }
      } catch (err) {
        updateTabState<EditorState>(tabId, () => ({
          status: 'error',
          filePath: state.filePath,
          error: `Error loading file: ${err}`
        }))
      }
    }

    if (state.status !== 'ready' || !state.originalContent) {
      loadFile()
    }
  }, [state?.filePath, wsData.path, tabId, filesystem, updateTabState, updateTabTitle, state?.status])

  // Update tab title with dirty indicator
  const isDirty = state?.status === 'ready' ? state.isDirty : false
  useEffect(() => {
    if (!state?.filePath) return
    const filename = getFilename(state.filePath)
    const title = isDirty ? `${filename} \u2022` : filename
    updateTabTitle(tabId, title)
  }, [isDirty, state?.filePath, tabId, updateTabTitle])

  // Persist scroll position on unmount
  useEffect(() => {
    return () => {
      updateTabState<EditorState>(tabId, (s) => {
        if (s.status !== 'ready') return s
        return { ...s, scrollTop: lastScrollTopRef.current }
      })
    }
  }, [tabId, updateTabState])

  const handleSave = useCallback(async () => {
    if (!state || state.status !== 'ready' || !state.isDirty || saving) return

    setSaving(true)
    try {
      const result = await filesystem.writeFile(
        state.filePath,
        state.currentContent
      )

      if (result.success) {
        updateTabState<EditorState>(tabId, (s) => {
          if (s.status !== 'ready') return s
          return { ...s, originalContent: s.currentContent, isDirty: false }
        })
      } else {
        alert(`Failed to save: ${result.error}`)
      }
    } catch (err) {
      alert(`Error saving file: ${err}`)
    } finally {
      setSaving(false)
    }
  }, [state, saving, tabId, filesystem, updateTabState])

  const handleEditorMount: OnMount = useCallback(
    (editor) => {
      editorRef.current = editor

      editor.onDidScrollChange((e) => {
        lastScrollTopRef.current = e.scrollTop
      })

      // Restore scroll position after content is ready
      if (lastScrollTopRef.current > 0) {
        editor.setScrollTop(lastScrollTopRef.current)
      }

      // Add Cmd/Ctrl+S keybinding for save
      editor.addCommand(KeyMod.CtrlCmd | KeyCode.KeyS, () => handleSave())
    },
    [handleSave]
  )

  const handleEditorChange: OnChange = useCallback(
    (value) => {
      if (value === undefined) return

      updateTabState<EditorState>(tabId, (s) => {
        if (s.status !== 'ready') return s
        return { ...s, currentContent: value, isDirty: value !== s.originalContent }
      })
    },
    [tabId, updateTabState]
  )

  const toggleViewMode = useCallback(() => {
    updateTabState<EditorState>(tabId, (s) => {
      if (s.status !== 'ready') return s
      return { ...s, viewMode: s.viewMode === 'preview' ? 'editor' : 'preview' }
    })
  }, [tabId, updateTabState])

  const editorRef = useRef<editor.IStandaloneCodeEditor | null>(null)

  if (!state) {
    return <div className="file-editor-error">Invalid tab</div>
  }

  if (state.status === 'loading') {
    return (
      <div className="file-editor">
        <div className="file-editor-placeholder">Loading...</div>
      </div>
    )
  }

  if (state.status === 'error') {
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
