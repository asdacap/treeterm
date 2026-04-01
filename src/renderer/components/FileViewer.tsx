import { useState, useEffect, useRef, useCallback } from 'react'
import { createPortal } from 'react-dom'
import Editor, { OnMount } from '@monaco-editor/react'
import type { editor } from 'monaco-editor'
import { useStore } from 'zustand'
import type { EditorState, ReviewComment, WorkspaceStore } from '../types'
import { useFilesystemApi } from '../hooks/useWorkspaceApis'
import { MarkdownPreview } from './MarkdownPreview'
import { CommentInput } from './CommentInput'
import { CommentDisplay } from './CommentDisplay'

interface FileViewerProps {
  workspace: WorkspaceStore
  filePath: string | null
  // Comment props
  comments?: ReviewComment[]
  onLineClick?: (lineNumber: number) => void
  inlineCommentInput?: { lineNumber: number } | null
  onCommentSubmit?: (text: string) => void
  onCommentCancel?: () => void
  onCommentDelete?: (commentId: string) => void
  // Scroll to a specific line after file loads
  scrollToLine?: number
  onScrollToLineUsed?: () => void
  // Scroll position persistence
  initialScrollTop?: number
  onScrollPositionChange?: (scrollTop: number) => void
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

function getMimeType(filePath: string): string {
  const ext = filePath.split('.').pop()?.toLowerCase() || ''
  const mimeMap: Record<string, string> = {
    png: 'image/png',
    jpg: 'image/jpeg',
    jpeg: 'image/jpeg',
    gif: 'image/gif',
    webp: 'image/webp',
    bmp: 'image/bmp',
    ico: 'image/x-icon',
    avif: 'image/avif',
    svg: 'image/svg+xml',
  }
  return mimeMap[ext] || 'image/png'
}

export function FileViewer({
  workspace,
  filePath,
  comments = [],
  onLineClick,
  inlineCommentInput,
  onCommentSubmit,
  onCommentCancel,
  onCommentDelete,
  scrollToLine,
  onScrollToLineUsed,
  initialScrollTop,
  onScrollPositionChange
}: FileViewerProps): JSX.Element {
  const { workspace: wsData, addTab } = useStore(workspace)
  const filesystem = useFilesystemApi(workspace)
  const [fileState, setFileState] = useState<FileState>({
    content: '',
    language: 'plaintext',
    loading: false,
    error: null
  })
  const editorRef = useRef<editor.IStandaloneCodeEditor | null>(null)
  const lastScrollTopRef = useRef<number>(initialScrollTop ?? 0)
  const decorationsRef = useRef<string[]>([])
  const viewZoneIdRef = useRef<string | null>(null)
  const [commentContainer, setCommentContainer] = useState<HTMLDivElement | null>(null)
  const [viewMode, setViewMode] = useState<'source' | 'preview'>(onLineClick ? 'source' : 'preview')
  // Inline comment display zones
  const commentDisplayZonesRef = useRef<Map<string, { zoneId: string; container: HTMLDivElement }>>(new Map())
  const [commentDisplayContainers, setCommentDisplayContainers] = useState<Map<string, { container: HTMLDivElement; comments: ReviewComment[] }>>(new Map())

  useEffect(() => {
    if (!filePath) {
      setFileState({ content: '', language: 'plaintext', loading: false, error: null })
      return
    }

    const loadFile = async () => {
      setFileState((prev) => ({ ...prev, loading: true, error: null }))

      try {
        const result = await filesystem.readFile(filePath)

        if (result.success) {
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
  }, [wsData.path, filePath, filesystem])

  useEffect(() => {
    setViewMode(onLineClick ? 'source' : 'preview')
  }, [filePath, onLineClick])

  const handleEditorMount: OnMount = useCallback((editor, monaco) => {
    editorRef.current = editor

    editor.onDidScrollChange((e) => {
      lastScrollTopRef.current = e.scrollTop
    })

    if (onLineClick) {
      editor.onMouseDown((e) => {
        if (e.target.type === monaco.editor.MouseTargetType.GUTTER_LINE_NUMBERS) {
          const lineNumber = e.target.position?.lineNumber
          if (lineNumber) {
            onLineClick(lineNumber)
          }
        }
      })
    }
  }, [onLineClick])

  // Persist scroll position on unmount
  useEffect(() => {
    return () => {
      onScrollPositionChange?.(lastScrollTopRef.current)
    }
  }, [onScrollPositionChange])

  // Scroll to a specific line when requested, or restore scroll position
  useEffect(() => {
    if (!editorRef.current || fileState.loading) return
    if (scrollToLine) {
      editorRef.current.revealLineInCenter(scrollToLine)
      onScrollToLineUsed?.()
    } else if (initialScrollTop) {
      editorRef.current.setScrollTop(initialScrollTop)
    }
  }, [scrollToLine, fileState.loading, initialScrollTop, onScrollToLineUsed])

  // Add decorations for lines with comments
  useEffect(() => {
    if (!editorRef.current) return

    // Clear old decorations
    decorationsRef.current = editorRef.current.deltaDecorations(
      decorationsRef.current,
      []
    )

    if (!comments.length) return

    const decorations = comments.map(comment => ({
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

    decorationsRef.current = editorRef.current.deltaDecorations(
      [],
      decorations
    )
  }, [comments])

  // Manage inline comment display view zones
  useEffect(() => {
    if (!editorRef.current) return

    const ed = editorRef.current

    // Group comments by lineNumber
    const groups = new Map<string, ReviewComment[]>()
    for (const comment of comments) {
      const key = String(comment.lineNumber)
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
      ed.changeViewZones((accessor: editor.IViewZoneChangeAccessor) => {
        accessor.removeZone(zone.zoneId)
      })
    })
    existingZones.clear()

    // Create new zones for each group
    const newContainers = new Map<string, { container: HTMLDivElement; comments: ReviewComment[] }>()

    Array.from(groups.entries()).forEach(([key, groupComments]) => {
      const lineNumber = parseInt(key, 10)

      const container = document.createElement('div')
      container.className = 'inline-comment-zone inline-comment-display-zone'
      container.addEventListener('mousedown', (e) => e.stopPropagation())

      ed.changeViewZones((accessor: editor.IViewZoneChangeAccessor) => {
        const zoneId = accessor.addZone({
          afterLineNumber: lineNumber,
          heightInPx: groupComments.length * 44 + 12,
          domNode: container,
          suppressMouseDown: true
        })
        existingZones.set(key, { zoneId, container })
      })

      newContainers.set(key, { container, comments: groupComments })
    })

    setCommentDisplayContainers(newContainers)

    const currentZones = commentDisplayZonesRef.current
    return () => {
      Array.from(currentZones.values()).forEach(zone => {
        try {
          ed.changeViewZones((accessor: editor.IViewZoneChangeAccessor) => {
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
    if (!editorRef.current) return

    if (!inlineCommentInput) {
      if (viewZoneIdRef.current) {
        editorRef.current.changeViewZones((accessor) => {
          if (viewZoneIdRef.current) {
            accessor.removeZone(viewZoneIdRef.current)
          }
        })
        viewZoneIdRef.current = null
        setCommentContainer(null)
      }
      return
    }

    const { lineNumber } = inlineCommentInput

    const container = document.createElement('div')
    container.className = 'inline-comment-zone'
    container.addEventListener('mousedown', (e) => e.stopPropagation())
    setCommentContainer(container)

    editorRef.current.changeViewZones((accessor) => {
      if (viewZoneIdRef.current) {
        accessor.removeZone(viewZoneIdRef.current)
      }

      viewZoneIdRef.current = accessor.addZone({
        afterLineNumber: lineNumber,
        heightInPx: 180,
        domNode: container,
        suppressMouseDown: true
      })
    })

    return () => {
      if (viewZoneIdRef.current && editorRef.current) {
        editorRef.current.changeViewZones((accessor) => {
          if (viewZoneIdRef.current) {
            accessor.removeZone(viewZoneIdRef.current)
          }
        })
        viewZoneIdRef.current = null
      }
    }
  }, [inlineCommentInput])

  const handleOpenInTab = useCallback(() => {
    if (!filePath) return

    addTab<EditorState>('editor', {
      status: 'ready',
      filePath: filePath,
      originalContent: fileState.content,
      currentContent: fileState.content,
      language: fileState.language,
      isDirty: false,
      viewMode: fileState.language === 'markdown' ? 'preview' : 'editor',
    })
  }, [filePath, fileState, addTab])

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
  const isImage = fileState.language === 'image'

  return (
    <div className="file-viewer">
      <div className="file-viewer-header">
        <span className="file-viewer-filename">{fileName}</span>
        <div className="file-viewer-header-actions">
          {isMarkdown && (
            <button
              className="file-viewer-toggle-btn"
              onClick={() => setViewMode(v => v === 'preview' ? 'source' : 'preview')}
              title={viewMode === 'preview' ? 'View source' : 'View preview'}
            >
              {viewMode === 'preview' ? '</> Source' : 'Preview'}
            </button>
          )}
          {!isImage && (
            <button
              className="file-viewer-open-btn"
              onClick={handleOpenInTab}
              title="Open in new tab for editing"
            >
              ⇗ Open in Tab
            </button>
          )}
          <span className="file-viewer-language">{fileState.language}</span>
        </div>
      </div>
      <div className="file-viewer-content">
        {isImage ? (
          <div className="file-viewer-image-container">
            <img
              src={`data:${getMimeType(filePath)};base64,${fileState.content}`}
              alt={fileName}
            />
          </div>
        ) : isMarkdown && viewMode === 'preview' ? (
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
              padding: { top: 8 },
              glyphMargin: !!onLineClick
            }}
          />
        )}
      </div>
      {commentContainer && inlineCommentInput && onCommentSubmit && onCommentCancel && createPortal(
        <CommentInput
          lineNumber={inlineCommentInput.lineNumber}
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
