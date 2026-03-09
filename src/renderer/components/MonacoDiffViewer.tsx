import { DiffEditor, DiffOnMount } from '@monaco-editor/react'
import { useRef } from 'react'
import type { editor } from 'monaco-editor'

interface MonacoDiffViewerProps {
  originalContent: string
  modifiedContent: string
  language: string
  originalLabel?: string
  modifiedLabel?: string
}

export function MonacoDiffViewer({
  originalContent,
  modifiedContent,
  language,
  originalLabel = 'Original',
  modifiedLabel = 'Modified'
}: MonacoDiffViewerProps): JSX.Element {
  const diffEditorRef = useRef<editor.IStandaloneDiffEditor | null>(null)

  const handleEditorMount: DiffOnMount = (editor) => {
    diffEditorRef.current = editor
  }

  return (
    <div className="monaco-diff-viewer">
      <div className="monaco-diff-labels">
        <span className="monaco-diff-label original">{originalLabel}</span>
        <span className="monaco-diff-label modified">{modifiedLabel}</span>
      </div>
      <div className="monaco-diff-editor-container">
        <DiffEditor
          height="100%"
          language={language}
          original={originalContent}
          modified={modifiedContent}
          theme="vs-dark"
          onMount={handleEditorMount}
          options={{
            readOnly: true,
            renderSideBySide: true,
            minimap: { enabled: false },
            lineNumbers: 'on',
            scrollBeyondLastLine: false,
            fontSize: 12,
            folding: true,
            renderLineHighlight: 'none',
            scrollbar: {
              vertical: 'auto',
              horizontal: 'auto'
            },
            diffWordWrap: 'off',
            enableSplitViewResizing: true,
            renderIndicators: true,
            originalEditable: false,
            ignoreTrimWhitespace: false
          }}
        />
      </div>
    </div>
  )
}
