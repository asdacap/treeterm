import React, { useEffect, useRef, useCallback, useState } from 'react'
import Editor, { OnMount, OnChange } from '@monaco-editor/react'
import { editor, KeyMod, KeyCode } from 'monaco-editor'
import { useStore } from 'zustand'
import type { EditorState, WorkspaceStore } from '../types'
import { EditorStatus, EditorViewMode } from '../types'
import { useFilesystemApi, useExecApi } from '../hooks/useWorkspaceApis'
import { monacoNavigationBridge } from '../monaco-config'
import { searchDefinition } from '../utils/definitionSearch'
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

export function FileEditor({ workspace, tabId }: FileEditorProps): React.JSX.Element {
  const { workspace: wsData, updateTabState, updateTabTitle, addTab, connectionId } = useStore(workspace)
  const filesystem = useFilesystemApi(workspace)
  const execApi = useExecApi(workspace)
  // eslint-disable-next-line @typescript-eslint/no-non-null-assertion -- tabId guaranteed to exist in appStates
  const appState = wsData.appStates[tabId]!
  const state = appState.state as EditorState | undefined

  const scrollTop = state?.status === EditorStatus.Ready ? state.scrollTop ?? 0 : 0
  const lastScrollTopRef = useRef(scrollTop)
  const [saving, setSaving] = useState(false)

  const pendingScrollToLine = state && state.status !== EditorStatus.Error ? state.scrollToLine : undefined

  // Load file content on mount
  useEffect(() => {
    if (!state?.filePath || state.status === EditorStatus.Loading) return

    const loadFile = async () => {
      updateTabState<EditorState>(tabId, () => ({
        status: EditorStatus.Loading,
        filePath: state.filePath,
        scrollToLine: pendingScrollToLine,
      }))

      try {
        const result = await filesystem.readFile(state.filePath)

        if (result.success) {
          const language = mapLanguageToMonaco(result.file.language)

          updateTabState<EditorState>(tabId, () => ({
            status: EditorStatus.Ready,
            filePath: state.filePath,
            originalContent: result.file.content,
            currentContent: result.file.content,
            language,
            viewMode: language === 'markdown' ? EditorViewMode.Preview : EditorViewMode.Editor,
            isDirty: false,
            scrollToLine: pendingScrollToLine,
          }))

          updateTabTitle(tabId, getFilename(state.filePath))
        } else {
          updateTabState<EditorState>(tabId, () => ({
            status: EditorStatus.Error,
            filePath: state.filePath,
            error: result.error
          }))
        }
      } catch (err) {
        updateTabState<EditorState>(tabId, () => ({
          status: EditorStatus.Error,
          filePath: state.filePath,
          error: `Error loading file: ${err instanceof Error ? err.message : String(err)}`
        }))
      }
    }

    if (state.status !== EditorStatus.Ready) {
      void loadFile()
    }
  }, [state?.filePath, wsData.path, tabId, filesystem, updateTabState, updateTabTitle, state?.status, pendingScrollToLine])

  // Update tab title with dirty indicator
  const isDirty = state?.status === EditorStatus.Ready ? state.isDirty : false
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
        if (s.status !== EditorStatus.Ready) return s
        return { ...s, scrollTop: lastScrollTopRef.current }
      })
    }
  }, [tabId, updateTabState])

  const handleSave = useCallback(async () => {
    if (!state || state.status !== EditorStatus.Ready || !state.isDirty || saving) return

    setSaving(true)
    try {
      const result = await filesystem.writeFile(
        state.filePath,
        state.currentContent
      )

      if (result.success) {
        updateTabState<EditorState>(tabId, (s) => {
          if (s.status !== EditorStatus.Ready) return s
          return { ...s, originalContent: s.currentContent, isDirty: false }
        })
      } else {
        alert(`Failed to save: ${result.error}`)
      }
    } catch (err) {
      alert(`Error saving file: ${err instanceof Error ? err.message : String(err)}`)
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

      // Scroll to specific line if requested (from go-to-definition)
      if (state?.status === EditorStatus.Ready && state.scrollToLine) {
        editor.revealLineInCenter(state.scrollToLine)
        updateTabState<EditorState>(tabId, (s) => {
          if (s.status !== EditorStatus.Ready) return s
          return { ...s, scrollToLine: undefined }
        })
      } else if (lastScrollTopRef.current > 0) {
        // Restore scroll position after content is ready
        editor.setScrollTop(lastScrollTopRef.current)
      }

      // Add Cmd/Ctrl+S keybinding for save
      editor.addCommand(KeyMod.CtrlCmd | KeyCode.KeyS, () => { void handleSave(); })

      // Wire navigation bridge for go-to-definition
      monacoNavigationBridge.searchDefinition = (symbol, language) =>
        searchDefinition(execApi, connectionId, wsData.path, symbol, language)
      monacoNavigationBridge.openFileAtLine = (targetFilePath, line) => {
        addTab<EditorState>('editor', { status: EditorStatus.Loading, filePath: targetFilePath, scrollToLine: line })
      }
      monacoNavigationBridge.getWorkspacePath = () => wsData.path
    },
    [handleSave, state, tabId, updateTabState, execApi, connectionId, wsData.path, addTab]
  )

  const handleEditorChange: OnChange = useCallback(
    (value) => {
      if (value === undefined) return

      updateTabState<EditorState>(tabId, (s) => {
        if (s.status !== EditorStatus.Ready) return s
        return { ...s, currentContent: value, isDirty: value !== s.originalContent }
      })
    },
    [tabId, updateTabState]
  )

  const toggleViewMode = useCallback(() => {
    updateTabState<EditorState>(tabId, (s) => {
      if (s.status !== EditorStatus.Ready) return s
      return { ...s, viewMode: s.viewMode === EditorViewMode.Preview ? EditorViewMode.Editor : EditorViewMode.Preview }
    })
  }, [tabId, updateTabState])

  const editorRef = useRef<editor.IStandaloneCodeEditor | null>(null)

  if (!state) {
    return <div className="file-editor-error">Invalid tab</div>
  }

  if (state.status === EditorStatus.Loading) {
    return (
      <div className="file-editor">
        <div className="file-editor-placeholder">Loading...</div>
      </div>
    )
  }

  if (state.status === EditorStatus.Error) {
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
              title={state.viewMode === EditorViewMode.Preview ? 'Edit' : 'Preview'}
            >
              {state.viewMode === EditorViewMode.Preview ? '\u270F Edit' : '\uD83D\uDC41 Preview'}
            </button>
          )}
          <button
            className="file-editor-save-btn"
            onClick={() => { void handleSave(); }}
            disabled={!state.isDirty || saving}
            title="Save (Cmd+S)"
          >
            {saving ? 'Saving...' : 'Save'}
          </button>
          <span className="file-editor-language">{state.language}</span>
        </div>
      </div>
      <div className="file-editor-content">
        {isMarkdown && state.viewMode === EditorViewMode.Preview ? (
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
