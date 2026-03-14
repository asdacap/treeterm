import { DiffEditor, DiffOnMount } from '@monaco-editor/react'
import { useRef, useState, useEffect } from 'react'
import { createPortal } from 'react-dom'
import type { editor } from 'monaco-editor'
import type { ReviewComment } from '../types'
import { CommentInput } from './CommentInput'

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
  // Comments props
  comments?: ReviewComment[]
  onLineClick?: (lineNumber: number, side: 'original' | 'modified') => void
  // Inline comment input
  inlineCommentInput?: {
    lineNumber: number
    side: 'original' | 'modified'
  } | null
  onCommentSubmit?: (text: string) => void
  onCommentCancel?: () => void
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
  hasNextFile = false,
  comments = [],
  onLineClick,
  inlineCommentInput,
  onCommentSubmit,
  onCommentCancel
}: MonacoDiffViewerProps): JSX.Element {
  const diffEditorRef = useRef<editor.IStandaloneDiffEditor | null>(null)
  const [lineChanges, setLineChanges] = useState<editor.ILineChange[] | null>(null)
  const [currentChangeIndex, setCurrentChangeIndex] = useState<number>(-1)
  const decorationsRef = useRef<{ original: string[]; modified: string[] }>({ original: [], modified: [] })
  const viewZoneIdRef = useRef<string | null>(null)
  const commentContainerRef = useRef<HTMLDivElement | null>(null)

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

  const handleEditorMount: DiffOnMount = (editor, monaco) => {
    diffEditorRef.current = editor

    // Listen for diff updates
    editor.onDidUpdateDiff(() => {
      updateLineChanges()
    })

    // Initial update (may already be computed)
    setTimeout(() => {
      updateLineChanges()
    }, 100)

    // Add line click handlers if callback is provided
    if (onLineClick) {
      const modifiedEditor = editor.getModifiedEditor()
      const originalEditor = editor.getOriginalEditor()

      modifiedEditor.onMouseDown((e) => {
        if (e.target.type === monaco.editor.MouseTargetType.GUTTER_LINE_NUMBERS) {
          const lineNumber = e.target.position?.lineNumber
          if (lineNumber) {
            onLineClick(lineNumber, 'modified')
          }
        }
      })

      originalEditor.onMouseDown((e) => {
        if (e.target.type === monaco.editor.MouseTargetType.GUTTER_LINE_NUMBERS) {
          const lineNumber = e.target.position?.lineNumber
          if (lineNumber) {
            onLineClick(lineNumber, 'original')
          }
        }
      })
    }
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

  // Add decorations for lines with comments
  useEffect(() => {
    if (!diffEditorRef.current || !comments.length) return

    const modifiedEditor = diffEditorRef.current.getModifiedEditor()
    const originalEditor = diffEditorRef.current.getOriginalEditor()

    // Clear old decorations
    decorationsRef.current.modified = modifiedEditor.deltaDecorations(
      decorationsRef.current.modified,
      []
    )
    decorationsRef.current.original = originalEditor.deltaDecorations(
      decorationsRef.current.original,
      []
    )

    // Add decorations for comments on modified side
    const modifiedDecorations = comments
      .filter(c => c.side === 'modified')
      .map(comment => ({
        range: {
          startLineNumber: comment.lineNumber,
          startColumn: 1,
          endLineNumber: comment.lineNumber,
          endColumn: 1
        },
        options: {
          isWholeLine: true,
          className: comment.isOutdated ? 'comment-line-outdated' : 'comment-line',
          glyphMarginClassName: comment.isOutdated ? 'comment-glyph-outdated' : 'comment-glyph'
        }
      }))

    // Add decorations for comments on original side
    const originalDecorations = comments
      .filter(c => c.side === 'original')
      .map(comment => ({
        range: {
          startLineNumber: comment.lineNumber,
          startColumn: 1,
          endLineNumber: comment.lineNumber,
          endColumn: 1
        },
        options: {
          isWholeLine: true,
          className: comment.isOutdated ? 'comment-line-outdated' : 'comment-line',
          glyphMarginClassName: comment.isOutdated ? 'comment-glyph-outdated' : 'comment-glyph'
        }
      }))

    decorationsRef.current.modified = modifiedEditor.deltaDecorations(
      [],
      modifiedDecorations
    )
    decorationsRef.current.original = originalEditor.deltaDecorations(
      [],
      originalDecorations
    )
  }, [comments])

  // Manage inline comment view zone
  useEffect(() => {
    if (!diffEditorRef.current) return

    // Clear existing view zone if no inline comment input
    if (!inlineCommentInput) {
      if (viewZoneIdRef.current) {
        const modifiedEditor = diffEditorRef.current.getModifiedEditor()
        const originalEditor = diffEditorRef.current.getOriginalEditor()

        modifiedEditor.changeViewZones((accessor) => {
          if (viewZoneIdRef.current) {
            accessor.removeZone(viewZoneIdRef.current)
          }
        })
        originalEditor.changeViewZones((accessor) => {
          if (viewZoneIdRef.current) {
            accessor.removeZone(viewZoneIdRef.current)
          }
        })

        viewZoneIdRef.current = null
        commentContainerRef.current = null
      }
      return
    }

    const { lineNumber, side } = inlineCommentInput
    const editor = side === 'modified'
      ? diffEditorRef.current.getModifiedEditor()
      : diffEditorRef.current.getOriginalEditor()

    // Create DOM container for inline comment
    const container = document.createElement('div')
    container.className = 'inline-comment-zone'
    commentContainerRef.current = container

    editor.changeViewZones((accessor) => {
      // Remove previous zone if exists
      if (viewZoneIdRef.current) {
        accessor.removeZone(viewZoneIdRef.current)
      }

      // Add new zone
      viewZoneIdRef.current = accessor.addZone({
        afterLineNumber: lineNumber,
        heightInPx: 140,
        domNode: container,
        suppressMouseDown: false
      })
    })

    return () => {
      // Cleanup view zone on unmount or change
      if (viewZoneIdRef.current && diffEditorRef.current) {
        editor.changeViewZones((accessor) => {
          if (viewZoneIdRef.current) {
            accessor.removeZone(viewZoneIdRef.current)
          }
        })
        viewZoneIdRef.current = null
      }
    }
  }, [inlineCommentInput])

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
      {commentContainerRef.current && inlineCommentInput && onCommentSubmit && onCommentCancel && createPortal(
        <CommentInput
          lineNumber={inlineCommentInput.lineNumber}
          side={inlineCommentInput.side}
          onSubmit={onCommentSubmit}
          onCancel={onCommentCancel}
        />,
        commentContainerRef.current
      )}
    </div>
  )
}
