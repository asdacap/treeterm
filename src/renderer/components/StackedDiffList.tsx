import React, { useState, useEffect, useRef, useCallback } from 'react'
import type { DiffFile, UncommittedFile, FileDiffContents, ReviewComment } from '../types'
import { FileChangeStatus } from '../types'
import { FileDiffSection } from './FileDiffSection'

interface FileLoadState {
  contents: FileDiffContents | null
  loading: boolean
  error: string | null
}

interface StagingAction {
  label: string
  onAction: () => void
  disabled: boolean
}

interface StackedDiffListProps {
  files: (DiffFile | UncommittedFile)[]
  loadFileContents: (filePath: string) => Promise<FileDiffContents>
  diffStyle: 'split' | 'unified'
  expandUnchanged: boolean
  getStatusIcon: (status: FileChangeStatus) => React.JSX.Element
  reviews: ReviewComment[]
  onLineClick: (filePath: string, lineNumber: number, side: 'original' | 'modified') => void
  commentInput: { filePath: string; lineNumber: number; side: 'original' | 'modified' } | null
  onCommentSubmit?: (text: string) => void
  onCommentCancel?: () => void
  onCommentDelete?: (commentId: string) => void
  getStagingAction?: (file: DiffFile | UncommittedFile) => StagingAction | undefined
  scrollToFile: string | null
  onActiveFileChange?: (filePath: string) => void
  onScrollToFileHandled?: () => void
  isFileViewed?: (filePath: string) => boolean
  onToggleViewed?: (file: DiffFile | UncommittedFile) => void
}

export function StackedDiffList({
  files,
  loadFileContents,
  diffStyle,
  expandUnchanged,
  getStatusIcon,
  reviews,
  onLineClick,
  commentInput,
  onCommentSubmit,
  onCommentCancel,
  onCommentDelete,
  getStagingAction,
  scrollToFile,
  onActiveFileChange,
  onScrollToFileHandled,
  isFileViewed,
  onToggleViewed,
}: StackedDiffListProps): React.JSX.Element {
  const [loadStates, setLoadStates] = useState<Map<string, FileLoadState>>(new Map())
  const sectionRefsRef = useRef<Map<string, HTMLElement>>(new Map())
  const scrollContainerRef = useRef<HTMLDivElement>(null)

  // Scroll to file when scrollToFile changes
  useEffect(() => {
    if (!scrollToFile) return
    const el = sectionRefsRef.current.get(scrollToFile)
    if (el) {
      el.scrollIntoView({ behavior: 'smooth', block: 'start' })
    }
    onScrollToFileHandled?.()
  }, [scrollToFile, onScrollToFileHandled])

  // Active file tracking via IntersectionObserver on section headers
  useEffect(() => {
    const container = scrollContainerRef.current
    if (!container || !onActiveFileChange) return

    const observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          if (entry.isIntersecting) {
            const filePath = (entry.target as HTMLElement).dataset.filePath
            if (filePath) {
              onActiveFileChange(filePath)
            }
          }
        }
      },
      { root: container, rootMargin: '0px 0px -90% 0px', threshold: 0 }
    )

    // Observe all section elements
    sectionRefsRef.current.forEach((el) => {
      observer.observe(el)
    })

    return () => { observer.disconnect() }
  }, [files, onActiveFileChange])

  const handleRequestLoad = useCallback(async (filePath: string) => {
    const existing = loadStates.get(filePath)
    if (existing?.contents || existing?.loading) return

    setLoadStates(prev => {
      const next = new Map(prev)
      next.set(filePath, { contents: null, loading: true, error: null })
      return next
    })

    try {
      const contents = await loadFileContents(filePath)
      setLoadStates(prev => {
        const next = new Map(prev)
        next.set(filePath, { contents, loading: false, error: null })
        return next
      })
    } catch (err) {
      setLoadStates(prev => {
        const next = new Map(prev)
        next.set(filePath, {
          contents: null,
          loading: false,
          error: err instanceof Error ? err.message : 'Failed to load diff',
        })
        return next
      })
    }
  }, [loadFileContents, loadStates])

  const registerRef = useCallback((filePath: string, el: HTMLElement | null) => {
    if (el) {
      sectionRefsRef.current.set(filePath, el)
    } else {
      sectionRefsRef.current.delete(filePath)
    }
  }, [])

  return (
    <div className="stacked-diff-list" ref={scrollContainerRef}>
      {files.map(file => {
        const state = loadStates.get(file.path)
        const fileComments = reviews.filter(c => c.filePath === file.path)
        const fileCommentInput = commentInput?.filePath === file.path
          ? { lineNumber: commentInput.lineNumber, side: commentInput.side }
          : null
        const stagingAction = getStagingAction?.(file)

        return (
          <div key={file.path} ref={(el) => { registerRef(file.path, el) }} data-file-path={file.path}>
            <FileDiffSection
              file={file}
              contents={state?.contents ?? null}
              loading={state?.loading ?? false}
              error={state?.error ?? null}
              onRequestLoad={() => { void handleRequestLoad(file.path) }}
              diffStyle={diffStyle}
              expandUnchanged={expandUnchanged}
              getStatusIcon={getStatusIcon}
              comments={fileComments}
              onLineClick={(lineNumber, side) => { onLineClick(file.path, lineNumber, side) }}
              inlineCommentInput={fileCommentInput}
              onCommentSubmit={onCommentSubmit}
              onCommentCancel={onCommentCancel}
              onCommentDelete={onCommentDelete}
              stagingAction={stagingAction}
              isViewed={isFileViewed?.(file.path) ?? false}
              onToggleViewed={onToggleViewed ? () => { onToggleViewed(file) } : undefined}
            />
          </div>
        )
      })}
    </div>
  )
}
