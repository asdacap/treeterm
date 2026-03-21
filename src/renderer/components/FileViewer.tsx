import { useState, useEffect, useRef, useCallback } from 'react'
import { createPortal } from 'react-dom'
import Editor, { OnMount } from '@monaco-editor/react'
import type { editor } from 'monaco-editor'
import { useFilesystemApi } from '../contexts/FilesystemApiContext'
import type { EditorState, ReviewComment, WorkspaceHandle } from '../types'
import { MarkdownPreview } from './MarkdownPreview'
import { CommentInput } from './CommentInput'

interface FileViewerProps {
  workspace: WorkspaceHandle
  filePath: string | null
  // Comment props
  comments?: ReviewComment[]
  onLineClick?: (lineNumber: number) => void
  inlineCommentInput?: { lineNumber: number } | null
  onCommentSubmit?: (text: string) => void
  onCommentCancel?: () => void
  // Scroll to a specific line after file loads
  scrollToLine?: number
  onScrollToLineUsed?: () => void
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

export function FileViewer({
  workspace,
  filePath,
  comments = [],
  onLineClick,
  inlineCommentInput,
  onCommentSubmit,
  onCommentCancel,
  scrollToLine,
  onScrollToLineUsed
}: FileViewerProps): JSX.Element {
  const filesystem = useFilesystemApi()
  const workspacePath = workspace.data.path
  const [fileState, setFileState] = useState<FileState>({
    content: '',
    language: 'plaintext',
    loading: false,
    error: null
  })
  const editorRef = useRef<editor.IStandaloneCodeEditor | null>(null)
  const decorationsRef = useRef<string[]>([])
  const viewZoneIdRef = useRef<string | null>(null)
  const [commentContainer, setCommentContainer] = useState<HTMLDivElement | null>(null)

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

  const handleEditorMount: OnMount = useCallback((editor, monaco) => {
    editorRef.current = editor

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

  // Scroll to a specific line when requested
  useEffect(() => {
    if (!editorRef.current || !scrollToLine || fileState.loading) return
    editorRef.current.revealLineInCenter(scrollToLine)
    onScrollToLineUsed?.()
  }, [scrollToLine, fileState.loading])

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

    workspace.addTab<EditorState>('editor', {
      filePath: filePath,
      originalContent: fileState.content,
      currentContent: fileState.content,
      language: fileState.language,
      isDirty: false,
      viewMode: fileState.language === 'markdown' ? 'preview' : 'editor',
      isLoading: false,
      error: null
    })
  }, [filePath, fileState, workspace])

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
            ⇗ Open in Tab
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
    </div>
  )
}
