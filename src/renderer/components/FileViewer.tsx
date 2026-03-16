import { useState, useEffect, useRef, useCallback } from 'react'
import Editor, { OnMount } from '@monaco-editor/react'
import type { editor } from 'monaco-editor'
import { useWorkspaceStore } from '../store/workspace'
import { useElectron } from '../store/ElectronContext'
import type { EditorState } from '../types'
import { MarkdownPreview } from './MarkdownPreview'

interface FileViewerProps {
  workspacePath: string
  workspaceId: string
  filePath: string | null
}

interface FileState {
  content: string
  language: string
  loading: boolean
  error: string | null
}

// Map backend language IDs to Monaco language IDs
function mapLanguageToMonaco(language: string): string {
  const languageMap: Record<string, string> = {
    bash: 'shell',
    // Most others are directly compatible
  }
  return languageMap[language] || language
}

export function FileViewer({ workspacePath, workspaceId, filePath }: FileViewerProps): JSX.Element {
  const { filesystem } = useElectron()
  const { addTabWithState } = useWorkspaceStore()
  const [fileState, setFileState] = useState<FileState>({
    content: '',
    language: 'plaintext',
    loading: false,
    error: null
  })
  const editorRef = useRef<editor.IStandaloneCodeEditor | null>(null)

  useEffect(() => {
    if (!filePath) {
      setFileState({ content: '', language: 'plaintext', loading: false, error: null })
      return
    }

    const loadFile = async () => {
      setFileState((prev) => ({ ...prev, loading: true, error: null }))

      try {
        const result = await filesystem.readFile(workspacePath, filePath)

        if (result.success && result.file) {
          setFileState({
            content: result.file.content,
            language: mapLanguageToMonaco(result.file.language),
            loading: false,
            error: null
          })
        } else {
          setFileState({
            content: '',
            language: 'plaintext',
            loading: false,
            error: result.error || 'Failed to load file'
          })
        }
      } catch (err) {
        setFileState({
          content: '',
          language: 'plaintext',
          loading: false,
          error: `Error loading file: ${err}`
        })
      }
    }

    loadFile()
  }, [workspacePath, filePath])

  const handleEditorMount: OnMount = useCallback((editor) => {
    editorRef.current = editor
  }, [])

  const handleOpenInTab = useCallback(() => {
    if (!filePath) return

    addTabWithState<EditorState>(workspaceId, 'editor', {
      filePath: filePath,
      originalContent: fileState.content,
      currentContent: fileState.content,
      language: fileState.language,
      isDirty: false,
      viewMode: fileState.language === 'markdown' ? 'preview' : 'editor',
      isLoading: false,
      error: null
    })
  }, [filePath, fileState, workspaceId, addTabWithState])

  if (!filePath) {
    return (
      <div className="file-viewer">
        <div className="file-viewer-placeholder">Select a file to view its contents</div>
      </div>
    )
  }

  if (fileState.loading) {
    return (
      <div className="file-viewer">
        <div className="file-viewer-placeholder">Loading...</div>
      </div>
    )
  }

  if (fileState.error) {
    return (
      <div className="file-viewer">
        <div className="file-viewer-error">{fileState.error}</div>
      </div>
    )
  }

  const fileName = filePath.split('/').pop() || filePath
  const isMarkdown = fileState.language === 'markdown'

  return (
    <div className="file-viewer">
      <div className="file-viewer-header">
        <span className="file-viewer-filename">{fileName}</span>
        <div className="file-viewer-header-actions">
          <button
            className="file-viewer-open-btn"
            onClick={handleOpenInTab}
            title="Open in new tab for editing"
          >
            \u21D7 Open in Tab
          </button>
          <span className="file-viewer-language">{fileState.language}</span>
        </div>
      </div>
      <div className="file-viewer-content">
        {isMarkdown ? (
          <MarkdownPreview content={fileState.content} />
        ) : (
          <Editor
            height="100%"
            language={fileState.language}
            value={fileState.content}
            theme="vs-dark"
            onMount={handleEditorMount}
            options={{
              readOnly: true,
              minimap: { enabled: false },
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
