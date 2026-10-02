// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, act, fireEvent } from '@testing-library/react'
import React from 'react'

// Track all IntersectionObserver instances for testing
const mockObservers: Array<{
  callback: IntersectionObserverCallback
  observe: ReturnType<typeof vi.fn>
  disconnect: ReturnType<typeof vi.fn>
}> = []

class MockIntersectionObserver {
  callback: IntersectionObserverCallback
  observe = vi.fn()
  disconnect = vi.fn()
  unobserve = vi.fn()
  constructor(callback: IntersectionObserverCallback) {
    this.callback = callback
    mockObservers.push(this)
  }
}

vi.stubGlobal('IntersectionObserver', MockIntersectionObserver)

// Hand-driven animation frames, so the scroll-settling loop can be stepped one
// frame at a time.
const pendingFrames = new Map<number, FrameRequestCallback>()
let nextFrameId = 1
vi.stubGlobal('requestAnimationFrame', (cb: FrameRequestCallback) => {
  const id = nextFrameId++
  pendingFrames.set(id, cb)
  return id
})
vi.stubGlobal('cancelAnimationFrame', (id: number) => { pendingFrames.delete(id) })

function runFrames(count: number): void {
  for (let i = 0; i < count; i++) {
    const due: FrameRequestCallback[] = Array.from(pendingFrames.values())
    pendingFrames.clear()
    act(() => { for (const cb of due) { cb(0) } })
  }
}

// jsdom has no layout, so getBoundingClientRect is stubbed: the scroll container
// sits at top 0 and the named file's wrapper reports whatever topOf() says.
function stubRects(filePath: string, topOf: () => number): void {
  vi.spyOn(Element.prototype, 'getBoundingClientRect').mockImplementation(function (this: Element) {
    const isTarget =
      this instanceof HTMLElement &&
      this.dataset.filePath === filePath &&
      !this.classList.contains('file-diff-section')
    return { top: isTarget ? topOf() : 0, height: 0 } as DOMRect
  })
}

vi.mock('@pierre/diffs/react', () => ({
  MultiFileDiff: (props: Record<string, unknown>) => {
    const { oldFile } = props as { oldFile: { name: string } }
    return <div data-testid={`diff-${oldFile.name}`} />
  },
  WorkerPoolContextProvider: ({ children }: { children: React.ReactNode }) => <>{children}</>,
}))

vi.mock('@pierre/diffs', () => ({}))
vi.mock('../pierre-diffs-config', () => ({
  createDiffsWorker: () => ({} as Worker),
}))

import { StackedDiffList, SETTLE_FRAMES, MAX_ALIGN_FRAMES } from './StackedDiffList'
import type { DiffFile, FileDiffContents, ReviewComment } from '../types'
import { FileChangeStatus, FileStatKind } from '../types'

function makeDiffFile(path: string): DiffFile {
  return {
    path,
    status: FileChangeStatus.Modified,
    kind: FileStatKind.Text,
    additions: 10,
    deletions: 5,
  }
}

function makeContents(): FileDiffContents {
  return {
    originalContent: 'old',
    modifiedContent: 'new',
    language: 'typescript',
  }
}

const defaultProps = {
  files: [makeDiffFile('src/a.ts'), makeDiffFile('src/b.ts')],
  loadFileContents: vi.fn(() => Promise.resolve(makeContents())),
  diffStyle: 'split' as const,
  expandUnchanged: true,
  ignoreWhitespace: false,
  getStatusIcon: vi.fn(() => <span>M</span>),
  reviews: [] as ReviewComment[],
  onLineClick: vi.fn(),
  commentInput: null,
  scrollToFile: null,
  onActiveFileChange: vi.fn(),
  onScrollToFileHandled: vi.fn(),
}

describe('StackedDiffList', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.restoreAllMocks()
    mockObservers.length = 0
    pendingFrames.clear()
  })

  it('renders a section for each file', () => {
    render(<StackedDiffList {...defaultProps} />)
    expect(screen.getByText('src/a.ts')).toBeDefined()
    expect(screen.getByText('src/b.ts')).toBeDefined()
  })

  it('calls loadFileContents when IntersectionObserver fires', async () => {
    const loadFileContents = vi.fn(() => Promise.resolve(makeContents()))
    render(<StackedDiffList {...defaultProps} loadFileContents={loadFileContents} />)

    // Find the FileDiffSection observer (the first observer for each section)
    // FileDiffSection creates its own IntersectionObserver
    const fileSectionObserver = mockObservers.find(o => o.observe.mock.calls.length > 0)
    expect(fileSectionObserver).toBeDefined()

    // Simulate intersection - use async act to wait for promise-based state updates
    // eslint-disable-next-line @typescript-eslint/require-await -- act needs async to flush promise-based state updates
    await act(async () => {
      fileSectionObserver!.callback(
        [{ isIntersecting: true } as IntersectionObserverEntry],
        {} as IntersectionObserver
      )
    })

    expect(loadFileContents).toHaveBeenCalled()
  })

  it('shows diff after contents loaded', async () => {
    const loadFileContents = vi.fn(() => Promise.resolve(makeContents()))
    render(<StackedDiffList {...defaultProps} files={[makeDiffFile('test.ts')]} loadFileContents={loadFileContents} />)

    // Find the FileDiffSection's IntersectionObserver
    const fileSectionObserver = mockObservers[0]
    expect(fileSectionObserver).toBeDefined()

    // eslint-disable-next-line @typescript-eslint/require-await -- act needs async to flush promise-based state updates
    await act(async () => {
      fileSectionObserver!.callback(
        [{ isIntersecting: true } as IntersectionObserverEntry],
        {} as IntersectionObserver
      )
    })

    expect(screen.getByTestId('diff-test.ts')).toBeDefined()
  })

  it('filters comments per file', () => {
    const reviews: ReviewComment[] = [
      { id: '1', filePath: 'src/a.ts', lineNumber: 1, text: 'comment-a', commitHash: null, createdAt: Date.now(), isOutdated: false, addressed: false, side: 'modified' },
      { id: '2', filePath: 'src/b.ts', lineNumber: 2, text: 'comment-b', commitHash: null, createdAt: Date.now(), isOutdated: false, addressed: false, side: 'modified' },
    ]
    render(<StackedDiffList {...defaultProps} reviews={reviews} />)
    // Component renders but comments are passed to FileDiffSection internally
    // We verify the component renders without error with reviews
    expect(screen.getByText('src/a.ts')).toBeDefined()
    expect(screen.getByText('src/b.ts')).toBeDefined()
  })

  it('shows error state when loadFileContents rejects', async () => {
    const loadFileContents = vi.fn(() => Promise.reject(new Error('Network error')))
    render(<StackedDiffList {...defaultProps} files={[makeDiffFile('fail.ts')]} loadFileContents={loadFileContents} />)

    const fileSectionObserver = mockObservers[0]
    // eslint-disable-next-line @typescript-eslint/require-await -- act needs async to flush promise-based state updates
    await act(async () => {
      fileSectionObserver!.callback(
        [{ isIntersecting: true } as IntersectionObserverEntry],
        {} as IntersectionObserver
      )
    })

    expect(screen.getByText('Network error')).toBeDefined()
  })

  it('calls onScrollToFileHandled once the target settles at the top', () => {
    const onScrollToFileHandled = vi.fn()

    render(<StackedDiffList {...defaultProps} scrollToFile="src/a.ts" onScrollToFileHandled={onScrollToFileHandled} />)

    // Already aligned, so the loop only has to burn its settle frames.
    expect(onScrollToFileHandled).not.toHaveBeenCalled()
    runFrames(SETTLE_FRAMES)
    expect(onScrollToFileHandled).toHaveBeenCalled()
  })

  it('re-aligns the scroll position while a section above the target keeps growing', () => {
    const onScrollToFileHandled = vi.fn()

    // Target starts 500px below the container top.
    let targetTop = 500
    stubRects('src/b.ts', () => targetTop)

    const { container } = render(
      <StackedDiffList {...defaultProps} scrollToFile="src/b.ts" onScrollToFileHandled={onScrollToFileHandled} />
    )
    const list = container.querySelector('.stacked-diff-list') as HTMLElement

    // The jump lands on mount, before any frame runs — a smooth animation would
    // still be in flight here.
    expect(list.scrollTop).toBe(500)

    // A neighbour mounts its viewer and shoves the target 100px further down.
    targetTop = 100
    runFrames(1)
    expect(list.scrollTop).toBe(600)
    expect(onScrollToFileHandled).not.toHaveBeenCalled()

    // Growth stops: the target holds still and the loop settles.
    targetTop = 0
    runFrames(SETTLE_FRAMES)
    expect(list.scrollTop).toBe(600)
    expect(onScrollToFileHandled).toHaveBeenCalled()
  })

  it('stops re-aligning after the frame cap when the target never settles', () => {
    const onScrollToFileHandled = vi.fn()

    // Never stops moving — the loop must give up rather than pin the scroll forever.
    stubRects('src/b.ts', () => 10)

    render(<StackedDiffList {...defaultProps} scrollToFile="src/b.ts" onScrollToFileHandled={onScrollToFileHandled} />)

    runFrames(MAX_ALIGN_FRAMES)
    expect(onScrollToFileHandled).toHaveBeenCalled()
    expect(pendingFrames.size).toBe(0)
  })

  it('reports the scroll handled when the target file is not in the list', () => {
    const onScrollToFileHandled = vi.fn()

    render(<StackedDiffList {...defaultProps} scrollToFile="src/gone.ts" onScrollToFileHandled={onScrollToFileHandled} />)

    expect(onScrollToFileHandled).toHaveBeenCalled()
  })

  it('calls onMarkViewedAbove with files above on right-click of viewed label', () => {
    const files = [makeDiffFile('src/a.ts'), makeDiffFile('src/b.ts'), makeDiffFile('src/c.ts')]
    const onMarkViewedAbove = vi.fn()
    render(
      <StackedDiffList
        {...defaultProps}
        files={files}
        onToggleViewed={vi.fn()}
        onMarkViewedAbove={onMarkViewedAbove}
      />
    )
    const viewedLabels = screen.getAllByText('Viewed')
    // Right-click on third file's viewed label (index 2) - should mark files at index 0 and 1
    fireEvent.contextMenu(viewedLabels[2]!)
    expect(onMarkViewedAbove).toHaveBeenCalledWith([files[0], files[1]])
  })

  it('does not call onMarkViewedAbove on right-click of first file viewed label', () => {
    const files = [makeDiffFile('src/a.ts'), makeDiffFile('src/b.ts')]
    const onMarkViewedAbove = vi.fn()
    render(
      <StackedDiffList
        {...defaultProps}
        files={files}
        onToggleViewed={vi.fn()}
        onMarkViewedAbove={onMarkViewedAbove}
      />
    )
    const viewedLabels = screen.getAllByText('Viewed')
    // Right-click on first file's viewed label - should not trigger
    fireEvent.contextMenu(viewedLabels[0]!)
    expect(onMarkViewedAbove).not.toHaveBeenCalled()
  })
})
