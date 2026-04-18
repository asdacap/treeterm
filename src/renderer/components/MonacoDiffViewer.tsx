/* eslint-disable custom/no-string-literal-comparison -- TODO: migrate existing string-literal comparisons to enums */
import { DiffEditor, DiffOnMount } from '@monaco-editor/react'
import React, { useRef, useState, useEffect } from 'react'
import { createPortal } from 'react-dom'
import type { editor } from 'monaco-editor'
import type { ReviewComment } from '../types'
import { CommentInput } from './CommentInput'
import { CommentDisplay } from './CommentDisplay'

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
  onCommentDelete?: (commentId: string) => void
  // Scroll position persistence
  initialScrollTop?: number
  onScrollPositionChange?: (scrollTop: number) => void
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
  onCommentCancel,
  onCommentDelete,
  initialScrollTop,
  onScrollPositionChange
}: MonacoDiffViewerProps): React.JSX.Element {
  const diffEditorRef = useRef<editor.IStandaloneDiffEditor | null>(null)
  const lastScrollTopRef = useRef(initialScrollTop ?? 0)
  let shouldRestoreScroll = !!initialScrollTop && initialScrollTop > 0
  const [lineChanges, setLineChanges] = useState<editor.ILineChange[] | null>(null)
  const [currentChangeIndex, setCurrentChangeIndex] = useState(-1)
  const [isSplitView, setIsSplitView] = useState(false)
  const [isWordWrap, setIsWordWrap] = useState(true)
  const decorationsRef = useRef<{ original: editor.IEditorDecorationsCollection | null; modified: editor.IEditorDecorationsCollection | null }>({ original: null, modified: null })
  const changeHighlightRef = useRef<{ original: editor.IEditorDecorationsCollection | null; modified: editor.IEditorDecorationsCollection | null }>({ original: null, modified: null })
  const viewZoneIdRef = useRef<string | null>(null)
  const [commentContainer, setCommentContainer] = useState<HTMLDivElement | null>(null)
  // Inline comment display zones
  const commentDisplayZonesRef = useRef(new Map<string, { zoneId: string; container: HTMLDivElement; editor: editor.IStandaloneCodeEditor }>())
  const [commentDisplayContainers, setCommentDisplayContainers] = useState(new Map<string, { container: HTMLDivElement; comments: ReviewComment[] }>())

  // Reset when content changes
  const [prevOriginalContent, setPrevOriginalContent] = useState(originalContent)
  const [prevModifiedContent, setPrevModifiedContent] = useState(modifiedContent)
  if (originalContent !== prevOriginalContent || modifiedContent !== prevModifiedContent) {
    setPrevOriginalContent(originalContent)
    setPrevModifiedContent(modifiedContent)
    setLineChanges(null)
    setCurrentChangeIndex(-1)
  }

  // Persist scroll position on unmount
  useEffect(() => {
    return () => {
      onScrollPositionChange?.(lastScrollTopRef.current)
    }
  }, [onScrollPositionChange])

  const updateLineChanges = () => {
    if (!diffEditorRef.current) return

    const changes = diffEditorRef.current.getLineChanges()
    setLineChanges(changes || [])

    // Restore saved scroll position instead of auto-scrolling to first change
    if (shouldRestoreScroll) {
      shouldRestoreScroll = false
      if (changes && changes.length > 0) {
        setCurrentChangeIndex(0)
      }
      const modEditor = diffEditorRef.current.getModifiedEditor()
      modEditor.setScrollTop(initialScrollTop ?? 0)
      return
    }

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

    // eslint-disable-next-line @typescript-eslint/no-non-null-assertion -- index is bounds-checked by caller
    const change = changesToUse[index]!
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

    // Track scroll position on the modified editor
    const modEditor = editor.getModifiedEditor()
    modEditor.onDidScrollChange((e) => {
      lastScrollTopRef.current = e.scrollTop
    })

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
        // eslint-disable-next-line @typescript-eslint/no-unsafe-member-access
        if (e.target.type === monaco.editor.MouseTargetType.GUTTER_LINE_NUMBERS) {
          const lineNumber = e.target.position?.lineNumber
          if (lineNumber) {
            onLineClick(lineNumber, 'modified')
          }
        }
      })

      originalEditor.onMouseDown((e) => {
        // eslint-disable-next-line @typescript-eslint/no-unsafe-member-access
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
    return `${String(currentChangeIndex + 1)}/${String(lineChanges.length)}`
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
    decorationsRef.current.modified?.clear()
    decorationsRef.current.original?.clear()

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

    decorationsRef.current.modified = modifiedEditor.createDecorationsCollection(modifiedDecorations)
    decorationsRef.current.original = originalEditor.createDecorationsCollection(originalDecorations)
  }, [comments])

  // Manage inline comment display view zones
  useEffect(() => {
    if (!diffEditorRef.current) return

    const modifiedEditor = diffEditorRef.current.getModifiedEditor()
    const originalEditor = diffEditorRef.current.getOriginalEditor()

    // Group comments by side:lineNumber
    const groups = new Map<string, ReviewComment[]>()
    for (const comment of comments) {
      const key = `${comment.side}:${String(comment.lineNumber)}`
      const group = groups.get(key)
      if (group) {
        group.push(comment)
      } else {
        groups.set(key, [comment])
      }
    }

    // Remove all existing display zones
    const existingZones = commentDisplayZonesRef.current
    Array.from(existingZones.values()).forEach(zone => {
      zone.editor.changeViewZones((accessor: editor.IViewZoneChangeAccessor) => {
        accessor.removeZone(zone.zoneId)
      })
    })
    existingZones.clear()

    // Create new zones for each group
    const newContainers = new Map<string, { container: HTMLDivElement; comments: ReviewComment[] }>()

    Array.from(groups.entries()).forEach(([key, groupComments]) => {
      const [side, lineStr] = key.split(':') as [string, string]
      const lineNumber = parseInt(lineStr, 10)
      const targetEditor = side === 'modified' ? modifiedEditor : originalEditor

      const container = document.createElement('div')
      container.className = 'inline-comment-zone inline-comment-display-zone'
      container.addEventListener('mousedown', (e) => { e.stopPropagation(); })

      targetEditor.changeViewZones((accessor: editor.IViewZoneChangeAccessor) => {
        const zoneId = accessor.addZone({
          afterLineNumber: lineNumber,
          heightInPx: groupComments.length * 44 + 12,
          domNode: container,
          suppressMouseDown: true
        })
        existingZones.set(key, { zoneId, container, editor: targetEditor })
      })

      newContainers.set(key, { container, comments: groupComments })
    })

    setCommentDisplayContainers(newContainers)

    const currentZones = commentDisplayZonesRef.current
    return () => {
      Array.from(currentZones.values()).forEach(zone => {
        try {
          zone.editor.changeViewZones((accessor: editor.IViewZoneChangeAccessor) => {
            accessor.removeZone(zone.zoneId)
          })
        } catch {
          // Editor may already be disposed
        }
      })
      currentZones.clear()
    }
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
        setCommentContainer(null)
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
    // Prevent Monaco from stealing focus when clicking inside the comment zone
    container.addEventListener('mousedown', (e) => { e.stopPropagation(); })
    setCommentContainer(container)

    editor.changeViewZones((accessor) => {
      // Remove previous zone if exists
      if (viewZoneIdRef.current) {
        accessor.removeZone(viewZoneIdRef.current)
      }

      // Add new zone
      viewZoneIdRef.current = accessor.addZone({
        afterLineNumber: lineNumber,
        heightInPx: 180,
        domNode: container,
        suppressMouseDown: true
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

  // Highlight current change range
  useEffect(() => {
    if (!diffEditorRef.current) return

    const modifiedEditor = diffEditorRef.current.getModifiedEditor()
    const originalEditor = diffEditorRef.current.getOriginalEditor()

    if (!lineChanges || lineChanges.length === 0 || currentChangeIndex < 0) {
      changeHighlightRef.current.modified?.clear()
      changeHighlightRef.current.original?.clear()
      return
    }

    // eslint-disable-next-line @typescript-eslint/no-non-null-assertion -- currentChangeIndex is bounds-checked above
    const change = lineChanges[currentChangeIndex]!
    const highlightOpts = { isWholeLine: true, className: 'current-change-highlight' }

    const modifiedDecs = change.modifiedStartLineNumber > 0 ? [{
      range: { startLineNumber: change.modifiedStartLineNumber, startColumn: 1, endLineNumber: change.modifiedEndLineNumber || change.modifiedStartLineNumber, endColumn: 1 },
      options: highlightOpts
    }] : []

    const originalDecs = change.originalStartLineNumber > 0 ? [{
      range: { startLineNumber: change.originalStartLineNumber, startColumn: 1, endLineNumber: change.originalEndLineNumber || change.originalStartLineNumber, endColumn: 1 },
      options: highlightOpts
    }] : []

    changeHighlightRef.current.modified?.clear()
    changeHighlightRef.current.modified = modifiedEditor.createDecorationsCollection(modifiedDecs)
    changeHighlightRef.current.original?.clear()
    changeHighlightRef.current.original = originalEditor.createDecorationsCollection(originalDecs)
  }, [currentChangeIndex, lineChanges])

  // Update editor options when split view changes
  useEffect(() => {
    if (diffEditorRef.current) {
      diffEditorRef.current.updateOptions({ renderSideBySide: isSplitView })
    }
  }, [isSplitView])

  // Update editor options when word wrap changes
  useEffect(() => {
    if (diffEditorRef.current) {
      diffEditorRef.current.updateOptions({ diffWordWrap: isWordWrap ? 'on' : 'off' } as Record<string, unknown>)
    }
  }, [isWordWrap])

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
        <button
          className={`monaco-diff-nav-btn monaco-diff-view-toggle${isSplitView ? ' active' : ''}`}
          onClick={() => { setIsSplitView(prev => !prev); }}
          title={isSplitView ? 'Switch to unified view' : 'Switch to split view'}
        >
          {isSplitView ? '⇔' : '⇕'}
        </button>
        <button
          className={`monaco-diff-nav-btn monaco-diff-view-toggle${isWordWrap ? ' active' : ''}`}
          onClick={() => { setIsWordWrap(prev => !prev); }}
          title={isWordWrap ? 'Disable word wrap' : 'Enable word wrap'}
        >
          ↩
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
            renderSideBySide: false,
            ...({ renderSideBySideMinWidthOverride: 0 } as Record<string, unknown>),
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
            diffWordWrap: 'on',
            enableSplitViewResizing: true,
            renderIndicators: true,
            originalEditable: false,
            ignoreTrimWhitespace: false
          }}
        />
      </div>
      {commentContainer && inlineCommentInput && onCommentSubmit && onCommentCancel && createPortal(
        <CommentInput
          lineNumber={inlineCommentInput.lineNumber}
          side={inlineCommentInput.side}
          onSubmit={onCommentSubmit}
          onCancel={onCommentCancel}
        />,
        commentContainer
      )}
      {Array.from(commentDisplayContainers.entries()).map(([key, { container, comments: groupComments }]) =>
        createPortal(
          <div className="inline-comment-display-group" key={key}>
            {groupComments.map(comment => (
              <CommentDisplay
                key={comment.id}
                comment={comment}
                onDelete={onCommentDelete ?? (() => {})}
                hideLineRef
              />
            ))}
          </div>,
          container
        )
      )}
    </div>
  )
}
