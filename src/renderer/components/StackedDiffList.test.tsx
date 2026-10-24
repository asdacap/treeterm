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

// The real diffs-container element (registered by @pierre/diffs) adopts a constructed
// stylesheet, which jsdom can't fill.
CSSStyleSheet.prototype.replaceSync = function () {}

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
    const { oldFile, selectedLines } = props as {
      oldFile: { name: string }
      selectedLines: { start: number; side: string } | null
    }
    // The real viewer renders inside a diffs-container host element.
    return React.createElement('diffs-container', null, (
      <div
        data-testid={`diff-${oldFile.name}`}
        data-selected={selectedLines ? `${selectedLines.side}:${String(selectedLines.start)}` : ''}
      />
    ))
  },
  WorkerPoolContextProvider: ({ children }: { children: React.ReactNode }) => <>{children}</>,
}))

// Find in diff parses with the real parser so its matches mirror the viewer.
vi.mock('@pierre/diffs', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@pierre/diffs')>()
  return {
    parseDiffFromFile: actual.parseDiffFromFile,
    DEFAULT_COLLAPSED_CONTEXT_THRESHOLD: actual.DEFAULT_COLLAPSED_CONTEXT_THRESHOLD,
  }
})
vi.mock('../pierre-diffs-config', () => ({
  createDiffsWorker: () => ({} as Worker),
}))

// The repo search is driven by hand: the test grabs the onEvent callback.
const repoSearch = vi.hoisted(() => ({
  onEvent: null as ((event: unknown) => void) | null,
}))
vi.mock('../utils/repoSearch', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../utils/repoSearch')>()
  return {
    ...actual,
    startRepoSearch: vi.fn((_exec: unknown, _conn: unknown, _cwd: unknown, _query: unknown, onEvent: (event: unknown) => void) => {
      repoSearch.onEvent = onEvent
      return () => { repoSearch.onEvent = null }
    }),
  }
})

import { StackedDiffList, SETTLE_FRAMES, MAX_ALIGN_FRAMES, MAX_SEARCH_LOADS } from './StackedDiffList'
import type { DiffFile, FileDiffContents, ReviewComment } from '../types'
import type { WorkspaceStore } from '../types'
import { DiffContentKind, FileChangeStatus, FileStatKind } from '../types'
import { createStore } from 'zustand/vanilla'
import { RepoSearchEventType, RepoSearchTool } from '../utils/repoSearch'
import { REPO_SEARCH_DEBOUNCE_MS } from './SearchPanel'

const addTab = vi.fn()
const workspace = createStore(() => ({
  workspace: { path: '/repo' },
  connectionId: 'conn',
  execApi: { start: vi.fn(), kill: vi.fn(), onEvent: vi.fn() },
  addTab,
})) as unknown as WorkspaceStore

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
    kind: DiffContentKind.Text,
    originalContent: 'old',
    modifiedContent: 'new',
    language: 'typescript',
  }
}

const defaultProps = {
  workspace,
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

  describe('find in diff', () => {
    function contentsWith(modified: string): FileDiffContents {
      return { kind: DiffContentKind.Text, originalContent: 'shared\n', modifiedContent: `shared\n${modified}\n`, language: 'typescript' }
    }

    // jsdom lays nothing out; the shortcut only fires for a list that's on screen.
    function markVisible(): void {
      vi.spyOn(Element.prototype, 'getClientRects').mockReturnValue([{}] as unknown as DOMRectList)
    }

    function pressFind(target: Element | Document = document): void {
      fireEvent.keyDown(target, { key: 'f', metaKey: true })
    }

    async function typeQuery(query: string): Promise<void> {
      // eslint-disable-next-line @typescript-eslint/require-await -- act needs async to flush promise-based state updates
      await act(async () => {
        fireEvent.change(screen.getByPlaceholderText('Search'), { target: { value: query } })
      })
    }

    it('opens the find bar on Cmd+F', () => {
      markVisible()
      render(<StackedDiffList {...defaultProps} />)
      expect(screen.queryByPlaceholderText('Search')).toBeNull()

      pressFind()

      const input = screen.getByPlaceholderText<HTMLInputElement>('Search')
      expect(document.activeElement).toBe(input)
    })

    it('ignores Cmd+F while focus is outside the review', () => {
      markVisible()
      render(<StackedDiffList {...defaultProps} />)
      const outside = document.createElement('input')
      document.body.appendChild(outside)
      outside.focus()

      pressFind(outside)

      expect(screen.queryByPlaceholderText('Search')).toBeNull()
      outside.remove()
    })

    it('ignores Cmd+F while the list is hidden', () => {
      render(<StackedDiffList {...defaultProps} />)
      pressFind()
      expect(screen.queryByPlaceholderText('Search')).toBeNull()
    })

    it('loads files that were never scrolled to, a few at a time', async () => {
      markVisible()
      const files = Array.from({ length: MAX_SEARCH_LOADS + 2 }, (_, i) => makeDiffFile(`f${String(i)}.ts`))
      const resolvers: Array<() => void> = []
      const loadFileContents = vi.fn(() => new Promise<FileDiffContents>((resolve) => {
        resolvers.push(() => { resolve(contentsWith('needle')) })
      }))
      render(<StackedDiffList {...defaultProps} files={files} loadFileContents={loadFileContents} />)
      pressFind()
      await typeQuery('needle')

      expect(loadFileContents).toHaveBeenCalledTimes(MAX_SEARCH_LOADS)
      expect(screen.getAllByText('Searching…')).toHaveLength(1)

      // eslint-disable-next-line @typescript-eslint/require-await -- act needs async to flush promise-based state updates
      await act(async () => { resolvers.splice(0).forEach(r => { r() }) })
      expect(loadFileContents).toHaveBeenCalledTimes(files.length)

      // eslint-disable-next-line @typescript-eslint/require-await -- act needs async to flush promise-based state updates
      await act(async () => { resolvers.splice(0).forEach(r => { r() }) })
      expect(screen.getByText(`${String(files.length)} found`)).toBeDefined()
    })

    it('steps through matches across files and highlights the current line', async () => {
      markVisible()
      const byPath: Record<string, FileDiffContents> = {
        'src/a.ts': contentsWith('const needle = 1'),
        'src/b.ts': contentsWith('NEEDLE()'),
      }
      const loadFileContents = vi.fn((path: string) => Promise.resolve(byPath[path]!))
      render(<StackedDiffList {...defaultProps} loadFileContents={loadFileContents} />)
      pressFind()
      await typeQuery('needle')
      expect(screen.getByText('2 found')).toBeDefined()

      const input = screen.getByPlaceholderText<HTMLInputElement>('Search')
      fireEvent.keyDown(input, { key: 'Enter' })
      expect(screen.getByText('1/2')).toBeDefined()
      expect(screen.getByTestId('diff-src/a.ts').dataset.selected).toBe('additions:2')
      expect(screen.getByTestId('diff-src/b.ts').dataset.selected).toBe('')

      fireEvent.keyDown(input, { key: 'Enter' })
      expect(screen.getByText('2/2')).toBeDefined()
      expect(screen.getByTestId('diff-src/a.ts').dataset.selected).toBe('')
      expect(screen.getByTestId('diff-src/b.ts').dataset.selected).toBe('additions:2')

      // Wraps around, both ways.
      fireEvent.keyDown(input, { key: 'Enter' })
      expect(screen.getByText('1/2')).toBeDefined()
      fireEvent.keyDown(input, { key: 'Enter', shiftKey: true })
      expect(screen.getByText('2/2')).toBeDefined()
    })

    it('expands a collapsed file when navigating to a match in it', async () => {
      markVisible()
      const loadFileContents = vi.fn(() => Promise.resolve(contentsWith('needle')))
      render(
        <StackedDiffList
          {...defaultProps}
          files={[makeDiffFile('src/a.ts')]}
          loadFileContents={loadFileContents}
          isFileViewed={() => true}
          onToggleViewed={vi.fn()}
        />
      )
      pressFind()
      await typeQuery('needle')
      expect(screen.queryByTestId('diff-src/a.ts')).toBeNull()

      fireEvent.keyDown(screen.getByPlaceholderText('Search'), { key: 'Enter' })

      expect(screen.getByTestId('diff-src/a.ts').dataset.selected).toBe('additions:2')
    })

    it('clears the highlight when closed with Escape', async () => {
      markVisible()
      render(<StackedDiffList {...defaultProps} loadFileContents={() => Promise.resolve(contentsWith('needle'))} />)
      pressFind()
      await typeQuery('needle')
      const input = screen.getByPlaceholderText<HTMLInputElement>('Search')
      fireEvent.keyDown(input, { key: 'Enter' })
      expect(screen.getByTestId('diff-src/a.ts').dataset.selected).toBe('additions:2')

      fireEvent.keyDown(input, { key: 'Escape' })

      expect(screen.queryByPlaceholderText('Search')).toBeNull()
      expect(screen.getByTestId('diff-src/a.ts').dataset.selected).toBe('')
    })

    async function renderLoaded(): Promise<void> {
      const byPath: Record<string, FileDiffContents> = {
        'src/a.ts': contentsWith('const needle = 1'),
        'src/b.ts': contentsWith('NEEDLE()'),
      }
      render(<StackedDiffList {...defaultProps} loadFileContents={(path: string) => Promise.resolve(byPath[path]!)} />)
      // Sections load as they scroll into view.
      // eslint-disable-next-line @typescript-eslint/require-await -- act needs async to flush promise-based state updates
      await act(async () => {
        for (const observer of mockObservers) {
          observer.callback([{ isIntersecting: true, target: document.body } as unknown as IntersectionObserverEntry], {} as IntersectionObserver)
        }
      })
    }

    function selectText(text: string): void {
      vi.spyOn(document, 'getSelection').mockReturnValue({ toString: () => text } as Selection)
    }

    it('double-clicking a word in the diff searches for it', async () => {
      await renderLoaded()
      selectText('needle')

      // eslint-disable-next-line @typescript-eslint/require-await -- act needs async to flush promise-based state updates
      await act(async () => { fireEvent.doubleClick(screen.getByTestId('diff-src/a.ts')) })

      const input = screen.getByPlaceholderText<HTMLInputElement>('Search')
      expect(input.value).toBe('needle')
      expect(screen.getByText('2 found')).toBeDefined()
      expect(screen.getByText('const needle = 1')).toBeDefined()
    })

    it.each([
      ['a multi-word selection', 'two words', 'diff-src/a.ts'],
      ['an empty selection', '  ', 'diff-src/a.ts'],
    ])('ignores a double-click that selected %s', async (_label, text, testId) => {
      await renderLoaded()
      selectText(text)
      fireEvent.doubleClick(screen.getByTestId(testId))
      expect(screen.queryByPlaceholderText('Search')).toBeNull()
    })

    it('ignores a double-click outside the diff viewer', async () => {
      await renderLoaded()
      selectText('needle')
      fireEvent.doubleClick(screen.getAllByText('M')[0]!)
      expect(screen.queryByPlaceholderText('Search')).toBeNull()
    })

    it('ignores a double-click inside an input', async () => {
      await renderLoaded()
      selectText('needle')
      const container = screen.getByTestId('diff-src/a.ts')
      const input = document.createElement('input')
      container.appendChild(input)
      fireEvent.doubleClick(input)
      expect(screen.queryByPlaceholderText('Search')).toBeNull()
    })

    it('clicking an in-diff result highlights that line', async () => {
      await renderLoaded()
      selectText('needle')
      // eslint-disable-next-line @typescript-eslint/require-await -- act needs async to flush promise-based state updates
      await act(async () => { fireEvent.doubleClick(screen.getByTestId('diff-src/a.ts')) })

      fireEvent.click(screen.getByText('NEEDLE()'))

      expect(screen.getByText('2/2')).toBeDefined()
      expect(screen.getByTestId('diff-src/b.ts').dataset.selected).toBe('additions:2')
    })

    it('lists repo results outside the diff and opens them in a file browser tab', async () => {
      vi.useFakeTimers()
      try {
        await renderLoaded()
        selectText('needle')
        act(() => { fireEvent.doubleClick(screen.getByTestId('diff-src/a.ts')) })
        act(() => { vi.advanceTimersByTime(REPO_SEARCH_DEBOUNCE_MS) })

        act(() => {
          repoSearch.onEvent?.({
            type: RepoSearchEventType.Hits,
            hits: [
              { filePath: 'src/a.ts', lineNumber: 2, column: 7, preview: 'in the diff already' },
              { filePath: 'lib/other.ts', lineNumber: 9, column: 1, preview: 'needle elsewhere' },
            ],
          })
          repoSearch.onEvent?.({ type: RepoSearchEventType.Done, tool: RepoSearchTool.Ripgrep, truncated: false })
        })

        expect(screen.queryByText('in the diff already')).toBeNull()
        expect(screen.getByText('1 found · rg')).toBeDefined()
        fireEvent.click(screen.getByText('needle elsewhere'))
        expect(addTab).toHaveBeenCalledWith('filesystem', { selectedPath: '/repo/lib/other.ts', scrollToLine: 9 })
      } finally {
        vi.useRealTimers()
      }
    })
  })
})
