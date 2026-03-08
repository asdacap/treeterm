import { useState, useEffect, useRef, useCallback } from 'react'
import Editor, { OnMount } from '@monaco-editor/react'
import type { editor } from 'monaco-editor'

interface FileViewerProps {
  workspacePath: string
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

export function FileViewer({ workspacePath, filePath }: FileViewerProps): JSX.Element {
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
        const result = await window.electron.filesystem.readFile(workspacePath, filePath)

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

  return (
    <div className="file-viewer">
      <div className="file-viewer-header">
        <span className="file-viewer-filename">{fileName}</span>
        <span className="file-viewer-language">{fileState.language}</span>
      </div>
      <div className="file-viewer-content">
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
      </div>
    </div>
  )
}
