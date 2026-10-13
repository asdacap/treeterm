import React, { useState, useEffect, useRef, useCallback, useMemo } from 'react'
import type { DiffFile, UncommittedFile, FileDiffContents, ReviewComment } from '../types'
import { FileChangeStatus } from '../types'
import { FileDiffSection } from './FileDiffSection'
import { DiffFindBar } from './DiffFindBar'
import { findLinesInDiff, type DiffSearchHit } from '../utils/diffSearch'

interface FileLoadState {
  contents: FileDiffContents | null
  loading: boolean
  error: string | null
}

// Consecutive frames the target must sit still at the top before a scroll counts
// as landed, and the hard cap that stops us fighting a section whose neighbours
// never stop resizing (or a user who scrolled away).
export const SETTLE_FRAMES = 3
export const MAX_ALIGN_FRAMES = 90

// Files fetched at once while a search loads every file's contents.
export const MAX_SEARCH_LOADS = 4

interface SearchMatch extends DiffSearchHit {
  filePath: string
}

// The diff viewer renders into the open shadow root of this element, and marks
// the line passed as selectedLines with data-selected-line.
const DIFFS_HOST_SELECTOR = 'diffs-container'

function findSelectedLineElement(section: HTMLElement): HTMLElement | null {
  const hosts = section.querySelectorAll(DIFFS_HOST_SELECTOR)
  for (let i = 0; i < hosts.length; i++) {
    const line = hosts.item(i).shadowRoot?.querySelector<HTMLElement>('[data-line][data-selected-line]')
    if (line) return line
  }
  return null
}

function isFindShortcut(e: KeyboardEvent): boolean {
  // eslint-disable-next-line custom/no-string-literal-comparison -- DOM key names are external
  return (e.metaKey || e.ctrlKey) && !e.altKey && !e.shiftKey && e.key.toLowerCase() === 'f'
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
  ignoreWhitespace: boolean
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
  onMarkViewedAbove?: (files: (DiffFile | UncommittedFile)[]) => void
}

export function StackedDiffList({
  files,
  loadFileContents,
  diffStyle,
  expandUnchanged,
  ignoreWhitespace,
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
  onMarkViewedAbove,
}: StackedDiffListProps): React.JSX.Element {
  const [loadStates, setLoadStates] = useState<Map<string, FileLoadState>>(new Map())
  const sectionRefsRef = useRef<Map<string, HTMLElement>>(new Map())
  const scrollContainerRef = useRef<HTMLDivElement>(null)
  const wrapperRef = useRef<HTMLDivElement>(null)

  // Find in diff. The browser's own find can't work here: lines live in shadow
  // DOM and off-screen files aren't mounted, so this searches the loaded file
  // contents instead and scrolls the match into view.
  const [findOpen, setFindOpen] = useState(false)
  const [findQuery, setFindQuery] = useState('')
  const [findFocusNonce, setFindFocusNonce] = useState(0)
  const [activeMatch, setActiveMatch] = useState<(SearchMatch & { nonce: number }) | null>(null)
  const matchNonceRef = useRef(0)

  // Scroll to file when scrollToFile changes.
  //
  // A smooth scrollIntoView animates toward a position computed once, up front.
  // Every section the animation sweeps past crosses FileDiffSection's lazy-load
  // margin, mounts its viewer, and grows past the stale spacer it was standing
  // in for — which pushes the target away from the position we're animating to.
  // The animation still stops there, so it lands on whichever file drifted into
  // that spot: a file before the target, varying with how many loaded in time.
  //
  // Jump instantly instead, so no section in between is ever swept past, then
  // re-assert the position each frame until the target's own neighbours have
  // finished mounting and stopped moving it.
  useEffect(() => {
    if (!scrollToFile) return
    const container = scrollContainerRef.current
    if (!container) return

    let frames = 0
    let settled = 0
    let rafId = 0

    const align = (): void => {
      const el = sectionRefsRef.current.get(scrollToFile)
      if (!el) {
        onScrollToFileHandled?.()
        return
      }
      const delta = el.getBoundingClientRect().top - container.getBoundingClientRect().top
      if (Math.abs(delta) < 1) {
        settled++
      } else {
        settled = 0
        container.scrollTop += delta
      }
      frames++
      if (settled < SETTLE_FRAMES && frames < MAX_ALIGN_FRAMES) {
        rafId = requestAnimationFrame(align)
        return
      }
      onScrollToFileHandled?.()
    }

    align()
    return () => { cancelAnimationFrame(rafId) }
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

  const handleRequestLoad = useCallback(async (filePath: string, force = false) => {
    const existing = loadStates.get(filePath)
    if (!force && (existing?.contents || existing?.loading)) return

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

  // Cmd/Ctrl+F opens the find bar when focus is within this review (or nowhere in
  // particular, e.g. after clicking into the diff) and the list is on screen.
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent): void => {
      if (!isFindShortcut(e)) return
      const wrapper = wrapperRef.current
      if (!wrapper || wrapper.getClientRects().length === 0) return
      const scope = wrapper.closest('.review-browser') ?? wrapper
      const focused = document.activeElement
      if (focused && focused !== document.body && !scope.contains(focused)) return
      e.preventDefault()
      setFindOpen(true)
      setFindFocusNonce(n => n + 1)
    }
    document.addEventListener('keydown', onKeyDown, true)
    return () => { document.removeEventListener('keydown', onKeyDown, true) }
  }, [])

  const searching = findOpen && findQuery.length > 0

  const matches = useMemo(() => {
    if (!searching) return []
    const opts = { diffStyle, expandUnchanged, ignoreWhitespace }
    const found: SearchMatch[] = []
    for (const file of files) {
      const contents = loadStates.get(file.path)?.contents
      if (!contents) continue
      for (const hit of findLinesInDiff(contents, file.path, findQuery, opts)) {
        found.push({ filePath: file.path, ...hit })
      }
    }
    return found
  }, [searching, findQuery, files, loadStates, diffStyle, expandUnchanged, ignoreWhitespace])

  const pendingSearch = searching && files.some(f => {
    const state = loadStates.get(f.path)
    return !state || state.loading
  })

  // Searching needs every file's contents, not just the ones scrolled past.
  // Fetch the rest a few at a time; each finished load re-runs this effect.
  useEffect(() => {
    if (!searching) return
    let inFlight = 0
    loadStates.forEach(state => { if (state.loading) inFlight++ })
    for (const file of files) {
      if (inFlight >= MAX_SEARCH_LOADS) break
      if (loadStates.has(file.path)) continue
      inFlight++
      void handleRequestLoad(file.path)
    }
  }, [searching, files, loadStates, handleRequestLoad])

  const activeIndex = activeMatch
    ? matches.findIndex(m =>
        m.filePath === activeMatch.filePath && m.side === activeMatch.side && m.lineNumber === activeMatch.lineNumber)
    : -1

  const stepMatch = (dir: 1 | -1): void => {
    if (matches.length === 0) return
    const next = activeIndex >= 0
      ? (activeIndex + dir + matches.length) % matches.length
      : dir === 1 ? 0 : matches.length - 1
    const match = matches[next]
    if (match) setActiveMatch({ ...match, nonce: ++matchNonceRef.current })
  }

  const closeFind = (): void => {
    setFindOpen(false)
    setActiveMatch(null)
  }

  // Bring the current match into view. Its section expands and mounts its viewer
  // in response to the new match, and the viewer renders asynchronously, so head
  // for the section first and re-aim at the highlighted line once it exists,
  // until it stops moving (same settling as the scroll-to-file effect).
  useEffect(() => {
    if (!activeMatch) return
    const container = scrollContainerRef.current
    if (!container) return

    let frames = 0
    let settled = 0
    let rafId = 0

    const align = (): void => {
      const section = sectionRefsRef.current.get(activeMatch.filePath)
      if (!section) return
      const line = findSelectedLineElement(section)
      const containerTop = container.getBoundingClientRect().top
      // Park the line a third of the way down, clear of the sticky file header.
      const delta = line
        ? line.getBoundingClientRect().top - containerTop - container.clientHeight / 3
        : section.getBoundingClientRect().top - containerTop
      const before = container.scrollTop
      if (Math.abs(delta) >= 1) container.scrollTop += delta
      // Settled once the line is in place, or can't move further (end of list).
      if (line && container.scrollTop === before) settled++
      else settled = 0
      frames++
      if (settled < SETTLE_FRAMES && frames < MAX_ALIGN_FRAMES) rafId = requestAnimationFrame(align)
    }

    align()
    return () => { cancelAnimationFrame(rafId) }
  }, [activeMatch])

  const registerRef = useCallback((filePath: string, el: HTMLElement | null) => {
    if (el) {
      sectionRefsRef.current.set(filePath, el)
    } else {
      sectionRefsRef.current.delete(filePath)
    }
  }, [])

  return (
    <div className="stacked-diff-list-wrapper" ref={wrapperRef}>
      {findOpen && (
        <DiffFindBar
          query={findQuery}
          onQueryChange={(q) => {
            setFindQuery(q)
            setActiveMatch(null)
          }}
          onNext={() => { stepMatch(1) }}
          onPrev={() => { stepMatch(-1) }}
          onClose={closeFind}
          current={activeIndex >= 0 ? activeIndex + 1 : null}
          total={matches.length}
          pending={pendingSearch}
          focusNonce={findFocusNonce}
        />
      )}
      <div className="stacked-diff-list" ref={scrollContainerRef}>
        {files.map((file, index) => {
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
                onRefresh={() => { void handleRequestLoad(file.path, true) }}
                diffStyle={diffStyle}
                expandUnchanged={expandUnchanged}
                ignoreWhitespace={ignoreWhitespace}
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
                onMarkViewedAbove={onMarkViewedAbove ? () => { onMarkViewedAbove(files.slice(0, index)) } : undefined}
                isFirstFile={index === 0}
                searchHit={activeMatch?.filePath === file.path ? activeMatch : null}
              />
            </div>
          )
        })}
      </div>
    </div>
  )
}
