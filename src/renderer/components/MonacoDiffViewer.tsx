import { DiffEditor, DiffOnMount } from '@monaco-editor/react'
import { useRef, useState, useEffect } from 'react'
import type { editor } from 'monaco-editor'

interface MonacoDiffViewerProps {
  originalContent: string
  modifiedContent: string
  language: string
  originalLabel?: string
  modifiedLabel?: string
  // Navigation props
  onPreviousFile?: () => void
  onNextFile?: () => void
  hasPreviousFile?: boolean
  hasNextFile?: boolean
}

export function MonacoDiffViewer({
  originalContent,
  modifiedContent,
  language,
  originalLabel = 'Original',
  modifiedLabel = 'Modified',
  onPreviousFile,
  onNextFile,
  hasPreviousFile = false,
  hasNextFile = false
}: MonacoDiffViewerProps): JSX.Element {
  const diffEditorRef = useRef<editor.IStandaloneDiffEditor | null>(null)
  const [lineChanges, setLineChanges] = useState<editor.ILineChange[] | null>(null)
  const [currentChangeIndex, setCurrentChangeIndex] = useState<number>(-1)

  // Reset when content changes
  useEffect(() => {
    setLineChanges(null)
    setCurrentChangeIndex(-1)
  }, [originalContent, modifiedContent])

  const updateLineChanges = () => {
    if (!diffEditorRef.current) return

    const changes = diffEditorRef.current.getLineChanges()
    setLineChanges(changes || [])

    // Auto-scroll to first change
    if (changes && changes.length > 0) {
      setCurrentChangeIndex(0)
      scrollToChange(0, changes)
    }
  }

  const scrollToChange = (index: number, changes?: editor.ILineChange[]) => {
    if (!diffEditorRef.current) return
    const changesToUse = changes || lineChanges
    if (!changesToUse || changesToUse.length === 0) return

    const change = changesToUse[index]
    const lineNumber = change.modifiedStartLineNumber || change.originalStartLineNumber
    const modifiedEditor = diffEditorRef.current.getModifiedEditor()
    modifiedEditor.revealLineInCenter(lineNumber)
  }

  const goToNextChange = () => {
    if (!lineChanges || lineChanges.length === 0) {
      // No changes in this file, go to next file
      if (onNextFile) onNextFile()
      return
    }

    const nextIndex = currentChangeIndex + 1
    if (nextIndex >= lineChanges.length) {
      // Past last change, go to next file
      if (onNextFile) onNextFile()
    } else {
      setCurrentChangeIndex(nextIndex)
      scrollToChange(nextIndex)
    }
  }

  const goToPreviousChange = () => {
    if (!lineChanges || lineChanges.length === 0) {
      // No changes in this file, go to previous file
      if (onPreviousFile) onPreviousFile()
      return
    }

    if (currentChangeIndex <= 0) {
      // At or before first change, go to previous file
      if (onPreviousFile) onPreviousFile()
    } else {
      const prevIndex = currentChangeIndex - 1
      setCurrentChangeIndex(prevIndex)
      scrollToChange(prevIndex)
    }
  }

  const handleEditorMount: DiffOnMount = (editor) => {
    diffEditorRef.current = editor

    // Listen for diff updates
    editor.onDidUpdateDiff(() => {
      updateLineChanges()
    })

    // Initial update (may already be computed)
    setTimeout(() => {
      updateLineChanges()
    }, 100)
  }

  const getNavInfo = () => {
    if (!lineChanges || lineChanges.length === 0) {
      return 'No changes'
    }
    return `${currentChangeIndex + 1}/${lineChanges.length}`
  }

  const isPrevDisabled = () => {
    if (!lineChanges || lineChanges.length === 0) {
      return !hasPreviousFile
    }
    return currentChangeIndex <= 0 && !hasPreviousFile
  }

  const isNextDisabled = () => {
    if (!lineChanges || lineChanges.length === 0) {
      return !hasNextFile
    }
    return currentChangeIndex >= lineChanges.length - 1 && !hasNextFile
  }

  useEffect(() => {
    return () => {
      if (diffEditorRef.current) {
        diffEditorRef.current.dispose()
        diffEditorRef.current = null
      }
    }
  }, [])

  return (
    <div className="monaco-diff-viewer">
      <div className="monaco-diff-labels">
        <span className="monaco-diff-label original">{originalLabel}</span>
        <span className="monaco-diff-label modified">{modifiedLabel}</span>
      </div>
      <div className="monaco-diff-nav">
        <button
          className="monaco-diff-nav-btn"
          onClick={goToPreviousChange}
          disabled={isPrevDisabled()}
          title="Previous change (or previous file)"
        >
          ▲
        </button>
        <span className="monaco-diff-nav-info">{getNavInfo()}</span>
        <button
          className="monaco-diff-nav-btn"
          onClick={goToNextChange}
          disabled={isNextDisabled()}
          title="Next change (or next file)"
        >
          ▼
        </button>
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
