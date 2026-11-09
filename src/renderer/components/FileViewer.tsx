/* eslint-disable custom/no-string-literal-comparison -- TODO: migrate existing string-literal comparisons to enums */
import React, { useState, useEffect, useRef, useCallback, lazy, Suspense } from 'react'
import { createPortal } from 'react-dom'
import Editor, { OnMount } from '@monaco-editor/react'
import type { editor } from 'monaco-editor'
import { useStore } from 'zustand'
import type { EditorState, ReviewComment, WorkspaceStore } from '../types'
import { EditorStatus, EditorViewMode } from '../types'
import { useFilesystemApi, useExecApi } from '../hooks/useWorkspaceApis'
import { maxReadBytesFor } from '../lib/fileLimits'
import { formatMegabytes } from '../../shared/formatBytes'
import { monacoNavigationBridge } from '../monaco-config'
import { searchDefinition } from '../utils/definitionSearch'
import { detectMonacoLanguage } from '../utils/detectMonacoLanguage'
import { MarkdownPreview } from './MarkdownPreview'
import { IMAGE_LANGUAGE, MODEL_LANGUAGE, imageMimeType, isBinaryContent, modelFormat } from '../../shared/fileContentKind'
import { FileDownloadButton } from './FileDownloadButton'
import { CommentInput } from './CommentInput'
import { CommentDisplay } from './CommentDisplay'
import { SearchPanel, type InViewHit } from './SearchPanel'
import { findInText } from '../utils/diffSearch'
import { toWorkspaceRelative } from '../utils/repoSearch'

// three.js is large; it loads the first time a model is opened.
const ModelViewer = lazy(() => import('./ModelViewer'))

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
  // Open a search result in another file (workspace-relative path)
  onOpenLocation: (relativePath: string, lineNumber: number) => void
}

enum SearchStatus {
  Closed = 'closed',
  Open = 'open',
}

type SearchState =
  | { status: SearchStatus.Closed }
  | { status: SearchStatus.Open; query: string; focusNonce: number; activeIndex: number | null }

enum FileErrorKind {
  Failed = 'failed',
  /** Over the transfer budget: `size` lets the user raise the budget to admit it. */
  TooLarge = 'too-large',
}

type FileError =
  | { kind: FileErrorKind.Failed; message: string }
  | { kind: FileErrorKind.TooLarge; message: string; size: number }

/** `onOpenAnyway` re-reads the file with a budget of the given byte count. */
function renderFileError(error: FileError, onOpenAnyway: (bytes: number) => void): React.ReactNode {
  const views: Record<FileErrorKind, () => React.ReactNode> = {
    [FileErrorKind.Failed]: () => <div className="file-viewer-error">{error.message}</div>,
    [FileErrorKind.TooLarge]: () => error.kind === FileErrorKind.TooLarge && (
      <div className="file-viewer-error">
        <div>{error.message}</div>
        <button
          className="file-viewer-open-btn file-viewer-override-btn"
          onClick={() => { onOpenAnyway(error.size) }}
          title="Load the whole file despite the size limit"
        >
          Open anyway ({formatMegabytes(error.size)})
        </button>
      </div>
    ),
  }
  return views[error.kind]()
}

/** The transfer budget for reading `filePath`: the default for its kind, or more if the user opened it anyway. */
function readBudget(filePath: string, raisedBudget: number): number {
  return Math.max(maxReadBytesFor(filePath), raisedBudget)
}

interface FileState {
  /**
   * The path `content` was read from. Until it matches the `filePath` prop the new file is
   * still loading: the previous file's content must not render under the new path, where it
   * would be taken for the new file's kind (a model's bytes handed to another format's parser).
   */
  path: string | null
  content: string
  language: string
  loading: boolean
  error: FileError | null
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
  onScrollPositionChange,
  onOpenLocation
}: FileViewerProps): React.JSX.Element {
  const wsData = useStore(workspace, s => s.workspace)
  const addTab = useStore(workspace, s => s.addTab)
  const connectionId = useStore(workspace, s => s.connectionId)
  const filesystem = useFilesystemApi(workspace)
  const execApi = useExecApi(workspace)
  const [fileState, setFileState] = useState<FileState>({
    path: null,
    content: '',
    language: 'plaintext',
    loading: false,
    error: null
  })
  const editorRef = useRef<editor.IStandaloneCodeEditor | null>(null)
  const lastScrollTopRef = useRef(initialScrollTop ?? 0)
  const decorationsRef = useRef<editor.IEditorDecorationsCollection | null>(null)
  const viewZoneIdRef = useRef<string | null>(null)
  const [commentContainer, setCommentContainer] = useState<HTMLDivElement | null>(null)
  const [viewMode, setViewMode] = useState<'source' | 'preview'>(onLineClick ? 'source' : 'preview')
  // Path the persisted scroll offset has already been applied for, so it is restored
  // once per file rather than re-asserted on every render.
  const [scrollRestoredFor, setScrollRestoredFor] = useState<string | null>(null)
  // Inline comment display zones
  const commentDisplayZonesRef = useRef(new Map<string, { zoneId: string; container: HTMLDivElement }>())
  const [commentDisplayContainers, setCommentDisplayContainers] = useState(new Map<string, { container: HTMLDivElement; comments: ReviewComment[] }>())
  // Search panel: Cmd/Ctrl+F or right-click > Search opens it
  const [search, setSearch] = useState<SearchState>({ status: SearchStatus.Closed })
  // Bytes the user agreed to pull down for an oversize file ("Open anyway"). The read takes the
  // larger of this and the default budget; it resets with the path, so the override is per file.
  const [raisedBudget, setRaisedBudget] = useState(0)

  // Callers rebuild the comments array on every render (they filter a store selection), so its
  // identity is useless as an effect dependency — the view-zone effect below would re-run forever,
  // since it sets state unconditionally. Re-key off the comment contents instead.
  const commentsSignature = comments
    .map(c => `${c.id}:${String(c.lineNumber)}:${String(c.isOutdated)}`)
    .join('|')
  const [stableComments, setStableComments] = useState(comments)
  const [prevCommentsSignature, setPrevCommentsSignature] = useState(commentsSignature)
  if (commentsSignature !== prevCommentsSignature) {
    setPrevCommentsSignature(commentsSignature)
    setStableComments(comments)
  }

  // Reset file state and view mode when file path or onLineClick changes
  const [prevFilePath, setPrevFilePath] = useState(filePath)
  const [prevHasLineClick, setPrevHasLineClick] = useState(!!onLineClick)
  if (filePath !== prevFilePath || !!onLineClick !== prevHasLineClick) {
    if (filePath !== prevFilePath) setRaisedBudget(0)
    setPrevFilePath(filePath)
    setPrevHasLineClick(!!onLineClick)
    if (!filePath) {
      setFileState({ path: null, content: '', language: 'plaintext', loading: false, error: null })
    }
    setViewMode(onLineClick ? 'source' : 'preview')
    setScrollRestoredFor(null)
    setSearch(s => s.status === SearchStatus.Open ? { ...s, activeIndex: null } : s)
  }

  useEffect(() => {
    if (!filePath) return
    // A superseded read must not land: an oversize file opened anyway can still be streaming
    // after the user has moved on, and its result would replace the next file's.
    let cancelled = false
    const maxBytes = readBudget(filePath, raisedBudget)

    const loadFile = async () => {
      setFileState((prev) => ({ ...prev, loading: true, error: null }))

      try {
        const result = await filesystem.readFile(filePath, maxBytes)
        if (cancelled) return

        if (result.success) {
          setFileState({
            path: filePath,
            content: result.file.content,
            // Raw-bytes files (base64 content) keep the kind main tagged them with.
            language: result.file.language === IMAGE_LANGUAGE || result.file.language === MODEL_LANGUAGE
              ? result.file.language
              : detectMonacoLanguage(filePath),
            loading: false,
            error: null
          })
        } else {
          setFileState({
            path: filePath,
            content: '',
            language: 'plaintext',
            loading: false,
            error: 'tooLarge' in result
              ? { kind: FileErrorKind.TooLarge, message: result.error, size: result.size }
              : { kind: FileErrorKind.Failed, message: result.error || 'Failed to load file' }
          })
        }
      } catch (err) {
        if (cancelled) return
        setFileState({
          path: filePath,
          content: '',
          language: 'plaintext',
          loading: false,
          error: { kind: FileErrorKind.Failed, message: `Error loading file: ${err instanceof Error ? err.message : String(err)}` }
        })
      }
    }

    void loadFile()
    return () => { cancelled = true }
  }, [wsData.path, filePath, filesystem, raisedBudget])

  const handleEditorMount: OnMount = useCallback((editor, monaco) => {
    editorRef.current = editor

    editor.onDidScrollChange((e) => {
      lastScrollTopRef.current = e.scrollTop
    })

    if (onLineClick) {
      editor.onMouseDown((e: { target: { type: number; position?: { lineNumber: number } | null } }) => {
        // eslint-disable-next-line @typescript-eslint/no-unsafe-member-access
        if (e.target.type === monaco.editor.MouseTargetType.GUTTER_LINE_NUMBERS) {
          const lineNumber = e.target.position?.lineNumber
          if (lineNumber) {
            onLineClick(lineNumber)
          }
        }
      })
    }

    const openSearch = (query: string): void => {
      setSearch(s => ({
        status: SearchStatus.Open,
        query,
        focusNonce: (s.status === SearchStatus.Open ? s.focusNonce : 0) + 1,
        activeIndex: null,
      }))
    }

    // Right-click > Search, next to Monaco's own Copy: searches for the selection,
    // or the word under the cursor (right-clicking outside a selection moves the cursor there).
    editor.addAction({
      id: 'treeterm.search',
      label: 'Search',
      contextMenuGroupId: '9_cutcopypaste',
      contextMenuOrder: 2,
      run: () => {
        const model = editor.getModel()
        const selection = editor.getSelection()
        if (!model || !selection) return
        const selected = model.getValueInRange(selection)
        if (selected && !selected.includes('\n')) {
          openSearch(selected)
          return
        }
        const word = model.getWordAtPosition(selection.getPosition())
        if (word) openSearch(word.word)
      },
    })

    // Cmd/Ctrl+F opens the search panel (seeded with the selection) instead of Monaco's find widget
    // eslint-disable-next-line @typescript-eslint/no-unsafe-member-access
    editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyF, () => {
      const selection = editor.getSelection()
      const selected = selection ? editor.getModel()?.getValueInRange(selection) ?? '' : ''
      if (selected && !selected.includes('\n')) {
        openSearch(selected)
      } else {
        setSearch(s => s.status === SearchStatus.Open
          ? { ...s, focusNonce: s.focusNonce + 1 }
          : { status: SearchStatus.Open, query: '', focusNonce: 1, activeIndex: null })
      }
    })

    // Wire navigation bridge for go-to-definition
    monacoNavigationBridge.searchDefinition = (symbol, language) =>
      searchDefinition(execApi, connectionId, wsData.path, symbol, language)
    monacoNavigationBridge.openFileAtLine = (targetFilePath, line) => {
      addTab<EditorState>('editor', { status: EditorStatus.Loading, filePath: targetFilePath, scrollToLine: line })
    }
    monacoNavigationBridge.getWorkspacePath = () => wsData.path
  }, [onLineClick, execApi, connectionId, wsData.path, addTab])

  // Persist scroll position on unmount
  useEffect(() => {
    return () => {
      onScrollPositionChange?.(lastScrollTopRef.current)
    }
  }, [onScrollPositionChange])

  // Scroll to a specific line when requested
  useEffect(() => {
    if (!editorRef.current || fileState.loading || !scrollToLine) return
    editorRef.current.revealLineInCenter(scrollToLine)
    onScrollToLineUsed?.()
  }, [scrollToLine, fileState.loading, onScrollToLineUsed])

  // Restore the persisted scroll offset once per file. This must not re-run on unrelated
  // renders: the offset is only written back on unmount, so a later render (opening the
  // inline comment input, for example) would otherwise snap the editor back to a stale
  // position — clamped to the bottom when the current file is shorter than the last one.
  useEffect(() => {
    if (!editorRef.current || fileState.loading) return
    if (scrollRestoredFor === filePath) return
    // eslint-disable-next-line react-hooks/set-state-in-effect -- one-shot guard; the scroll can only be applied once the editor exists
    setScrollRestoredFor(filePath)
    if (!scrollToLine && initialScrollTop) {
      editorRef.current.setScrollTop(initialScrollTop)
    }
  }, [filePath, fileState.loading, initialScrollTop, scrollToLine, scrollRestoredFor])

  // Add decorations for lines with comments
  useEffect(() => {
    if (!editorRef.current) return

    // Clear old decorations
    if (decorationsRef.current) {
      decorationsRef.current.clear()
    }

    if (!stableComments.length) return

    const decorations = stableComments.map(comment => ({
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

    decorationsRef.current = editorRef.current.createDecorationsCollection(decorations)
  }, [stableComments])

  // Manage inline comment display view zones
  useEffect(() => {
    if (!editorRef.current) return

    const ed = editorRef.current

    // Group comments by lineNumber
    const groups = new Map<string, ReviewComment[]>()
    for (const comment of stableComments) {
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
      container.addEventListener('mousedown', (e) => { e.stopPropagation(); })

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

    // eslint-disable-next-line react-hooks/set-state-in-effect -- sync with Monaco view zones for React portals
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
  }, [stableComments])

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
        // eslint-disable-next-line react-hooks/set-state-in-effect -- sync with Monaco view zones for React portals
        setCommentContainer(null)
      }
      return
    }

    const { lineNumber } = inlineCommentInput

    const container = document.createElement('div')
    container.className = 'inline-comment-zone'
    container.addEventListener('mousedown', (e) => { e.stopPropagation(); })
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
      status: EditorStatus.Ready,
      filePath: filePath,
      originalContent: fileState.content,
      currentContent: fileState.content,
      language: fileState.language,
      isDirty: false,
      viewMode: fileState.language === 'markdown' ? EditorViewMode.Preview : EditorViewMode.Editor,
    })
  }, [filePath, fileState, addTab])

  const isImage = fileState.language === IMAGE_LANGUAGE
  const isModel = fileState.language === MODEL_LANGUAGE
  const isBinary = !isImage && !isModel && isBinaryContent(fileState.content)
  const pending = fileState.loading || fileState.path !== filePath
  const searchable = !!filePath && !pending && !fileState.error && !isImage && !isModel && !isBinary
  const relativePath = filePath ? toWorkspaceRelative(wsData.path, filePath) : ''

  const renderViewer = (): React.JSX.Element => {
    if (!filePath) {
      return (
        <div className="file-viewer">
          <div className="file-viewer-placeholder">Select a file to view its contents</div>
        </div>
      )
    }

    if (pending) {
      return (
        <div className="file-viewer">
          <div className="file-viewer-placeholder">Loading...</div>
        </div>
      )
    }

    if (fileState.error) {
      return (
        <div className="file-viewer">
          {renderFileError(fileState.error, setRaisedBudget)}
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
            {isMarkdown && (
              <button
                className="file-viewer-toggle-btn"
                onClick={() => { setViewMode(v => v === 'preview' ? 'source' : 'preview'); }}
                title={viewMode === 'preview' ? 'View source' : 'View preview'}
              >
                {viewMode === 'preview' ? '</> Source' : 'Preview'}
              </button>
            )}
            <FileDownloadButton
              filesystem={filesystem}
              filePath={filePath}
              maxBytes={readBudget(filePath, raisedBudget)}
            />
            {!isImage && !isModel && !isBinary && (
              <button
                className="file-viewer-open-btn"
                onClick={handleOpenInTab}
                title="Open in new tab for editing"
              >
                ⇗ Open in Tab
              </button>
            )}
            <span className="file-viewer-language">{isBinary ? 'binary' : isModel ? modelFormat(filePath) : fileState.language}</span>
          </div>
        </div>
        <div className="file-viewer-content">
          {isImage ? (
            <div className="file-viewer-image-container">
              <img
                src={`data:${imageMimeType(filePath)};base64,${fileState.content}`}
                alt={fileName}
              />
            </div>
          ) : isModel ? (
            <Suspense fallback={<div className="file-viewer-placeholder">Loading 3D viewer…</div>}>
              <ModelViewer base64={fileState.content} format={modelFormat(filePath)} />
            </Suspense>
          ) : isBinary ? (
            <div className="file-viewer-placeholder">Binary file — not shown as text</div>
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

  const renderSearchPanel = (s: Extract<SearchState, { status: SearchStatus.Open }>): React.JSX.Element => {
    const textHits = searchable ? findInText(fileState.content, s.query) : []
    const lines = fileState.content.split('\n')
    const inViewHits: InViewHit[] = textHits.map(hit => ({
      filePath: relativePath,
      label: String(hit.lineNumber),
      preview: lines[hit.lineNumber - 1] ?? '',
    }))
    const select = (index: number): void => {
      const hit = textHits[index]
      if (!hit) return
      setSearch({ ...s, activeIndex: index })
      const ed = editorRef.current
      if (!ed) return
      ed.revealLineInCenter(hit.lineNumber)
      ed.setSelection({
        startLineNumber: hit.lineNumber,
        startColumn: hit.column,
        endLineNumber: hit.lineNumber,
        endColumn: hit.column + s.query.length,
      })
    }
    const step = (dir: 1 | -1): void => {
      if (textHits.length === 0) return
      const next = s.activeIndex === null
        ? (dir === 1 ? 0 : textHits.length - 1)
        : (s.activeIndex + dir + textHits.length) % textHits.length
      select(next)
    }
    return (
      <SearchPanel
        workspace={workspace}
        query={s.query}
        onQueryChange={(query) => { setSearch({ ...s, query, activeIndex: null }) }}
        focusNonce={s.focusNonce}
        onClose={() => { setSearch({ status: SearchStatus.Closed }) }}
        inViewSections={[{ title: 'In this file', hits: inViewHits }]}
        activeIndex={s.activeIndex}
        inViewPending={pending}
        onSelectInView={select}
        onNext={() => { step(1) }}
        onPrev={() => { step(-1) }}
        excludeFilePaths={[relativePath]}
        onSelectRepo={(hit) => { onOpenLocation(hit.filePath, hit.lineNumber) }}
      />
    )
  }

  return (
    <div className="file-viewer-with-search">
      {renderViewer()}
      {search.status === SearchStatus.Open && renderSearchPanel(search)}
    </div>
  )
}
