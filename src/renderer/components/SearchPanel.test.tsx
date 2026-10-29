// @vitest-environment jsdom
import { act, fireEvent, render, screen } from '@testing-library/react'
import { createStore } from 'zustand/vanilla'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { WorkspaceStore } from '../types'
import { RepoSearchEventType, RepoSearchTool, startRepoSearch } from '../utils/repoSearch'
import { REPO_SEARCH_DEBOUNCE_MS, SearchPanel, openInFileBrowser } from './SearchPanel'

const cancels = vi.hoisted(() => [] as ReturnType<typeof vi.fn>[])
const repoSearch = vi.hoisted(() => ({ onEvent: null as ((event: unknown) => void) | null }))
vi.mock('../utils/repoSearch', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../utils/repoSearch')>()
  return {
    ...actual,
    startRepoSearch: vi.fn((_exec: unknown, _conn: unknown, _cwd: unknown, _query: unknown, onEvent: (event: unknown) => void) => {
      repoSearch.onEvent = onEvent
      const cancel = vi.fn()
      cancels.push(cancel)
      return cancel
    }),
  }
})

const execApi = { start: vi.fn(), kill: vi.fn(), onEvent: vi.fn() }
const addTab = vi.fn()
const workspace = createStore(() => ({
  workspace: { path: '/repo' },
  connectionId: 'conn',
  execApi,
  addTab,
})) as unknown as WorkspaceStore

function renderPanel(props: Partial<React.ComponentProps<typeof SearchPanel>> = {}): ReturnType<typeof render> {
  return render(
    <SearchPanel
      workspace={workspace}
      query="needle"
      onQueryChange={vi.fn()}
      focusNonce={1}
      onClose={vi.fn()}
      inViewSections={[{ title: 'In this file', hits: [] }]}
      activeIndex={null}
      inViewPending={false}
      onSelectInView={vi.fn()}
      onNext={vi.fn()}
      onPrev={vi.fn()}
      excludeFilePaths={[]}
      onSelectRepo={vi.fn()}
      {...props}
    />
  )
}

function emit(event: unknown): void {
  act(() => { repoSearch.onEvent?.(event) })
}

describe('SearchPanel', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    vi.clearAllMocks()
    cancels.length = 0
    repoSearch.onEvent = null
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('focuses the input and shows nothing below it for an empty query', () => {
    renderPanel({ query: '' })
    expect(document.activeElement).toBe(screen.getByPlaceholderText('Search'))
    expect(screen.queryByText('In repo')).toBeNull()
    act(() => { vi.advanceTimersByTime(REPO_SEARCH_DEBOUNCE_MS) })
    expect(startRepoSearch).not.toHaveBeenCalled()
  })

  it('groups in-view hits by file and marks the active one', () => {
    const onSelectInView = vi.fn()
    renderPanel({
      inViewSections: [{
        title: 'In this file',
        hits: [
          { filePath: 'a.ts', label: '1', preview: 'first' },
          { filePath: 'a.ts', label: '−4', preview: 'second' },
          { filePath: 'b.ts', label: '2', preview: 'third' },
        ],
      }],
      activeIndex: 1,
      onSelectInView,
    })
    expect(screen.getByText('2/3')).toBeTruthy()
    expect(screen.getAllByText(/^[ab]\.ts$/)).toHaveLength(2)
    expect(screen.getByText('second').closest('button')?.className).toContain('active')
    fireEvent.click(screen.getByText('third'))
    expect(onSelectInView).toHaveBeenCalledWith(2)
  })

  it('lists sections in order, numbering hits across them', () => {
    const onSelectInView = vi.fn()
    renderPanel({
      inViewSections: [
        { title: 'In this file', hits: [{ filePath: 'a.ts', label: '1', preview: 'here' }] },
        { title: 'In diff', hits: [{ filePath: 'b.ts', label: '2', preview: 'there' }] },
      ],
      activeIndex: 1,
      onSelectInView,
    })
    const titles = screen.getAllByText(/^In (this file|diff|repo)$/).map(el => el.textContent)
    expect(titles).toEqual(['In this file', 'In diff', 'In repo'])
    expect(screen.getByText('2/2')).toBeTruthy()
    expect(screen.getByText('there').closest('button')?.className).toContain('active')
    expect(screen.getByText('here').closest('button')?.className).not.toContain('active')
    fireEvent.click(screen.getByText('there'))
    expect(onSelectInView).toHaveBeenCalledWith(1)
  })

  it('says when one section has no matches', () => {
    renderPanel({
      inViewSections: [
        { title: 'In this file', hits: [] },
        { title: 'In diff', hits: [{ filePath: 'b.ts', label: '2', preview: 'there' }] },
      ],
    })
    expect(screen.getAllByText('No matches')).toHaveLength(1)
  })

  it('shows a pending count while in-view content is still loading', () => {
    const { rerender } = renderPanel({ inViewPending: true })
    expect(screen.getByText('Searching…')).toBeTruthy()
    rerender(
      <SearchPanel
        workspace={workspace} query="needle" onQueryChange={vi.fn()} focusNonce={1} onClose={vi.fn()}
        inViewSections={[{ title: 'In diff', hits: [{ filePath: 'a.ts', label: '1', preview: 'x' }] }]} activeIndex={null}
        inViewPending onSelectInView={vi.fn()} onNext={vi.fn()} onPrev={vi.fn()} excludeFilePaths={[]} onSelectRepo={vi.fn()}
      />
    )
    expect(screen.getByText('1 found…')).toBeTruthy()
  })

  it('wires the input keys and buttons', () => {
    const onNext = vi.fn()
    const onPrev = vi.fn()
    const onClose = vi.fn()
    const onQueryChange = vi.fn()
    renderPanel({ onNext, onPrev, onClose, onQueryChange, inViewSections: [{ title: 'In this file', hits: [{ filePath: 'a.ts', label: '1', preview: 'x' }] }] })
    const input = screen.getByPlaceholderText('Search')
    fireEvent.change(input, { target: { value: 'other' } })
    expect(onQueryChange).toHaveBeenCalledWith('other')
    fireEvent.keyDown(input, { key: 'Enter' })
    fireEvent.keyDown(input, { key: 'Enter', shiftKey: true })
    fireEvent.keyDown(input, { key: 'a' })
    fireEvent.click(screen.getByTitle('Next match (Enter)'))
    fireEvent.click(screen.getByTitle('Previous match (Shift+Enter)'))
    expect(onNext).toHaveBeenCalledTimes(2)
    expect(onPrev).toHaveBeenCalledTimes(2)
    fireEvent.keyDown(input, { key: 'Escape' })
    fireEvent.click(screen.getByTitle('Close (Escape)'))
    expect(onClose).toHaveBeenCalledTimes(2)
  })

  it('starts the repo search after a pause and streams its results in', () => {
    const onSelectRepo = vi.fn()
    renderPanel({ onSelectRepo, excludeFilePaths: ['skip.ts'] })
    expect(screen.getByText('Searching repo…')).toBeTruthy()
    expect(startRepoSearch).not.toHaveBeenCalled()

    act(() => { vi.advanceTimersByTime(REPO_SEARCH_DEBOUNCE_MS) })
    expect(startRepoSearch).toHaveBeenCalledWith(execApi, 'conn', '/repo', 'needle', expect.any(Function))

    emit({ type: RepoSearchEventType.Hits, hits: [{ filePath: 'x.ts', lineNumber: 3, column: 1, preview: 'first hit' }] })
    expect(screen.getByText('first hit')).toBeTruthy()
    expect(screen.getByText('Searching repo…')).toBeTruthy()

    emit({
      type: RepoSearchEventType.Hits,
      hits: [
        { filePath: 'skip.ts', lineNumber: 1, column: 1, preview: 'excluded hit' },
        { filePath: 'y.ts', lineNumber: 5, column: 1, preview: 'second hit' },
      ],
    })
    emit({ type: RepoSearchEventType.Done, tool: RepoSearchTool.Ripgrep, truncated: false })

    expect(screen.queryByText('excluded hit')).toBeNull()
    expect(screen.getByText('2 found · rg')).toBeTruthy()
    fireEvent.click(screen.getByText('second hit'))
    expect(onSelectRepo).toHaveBeenCalledWith({ filePath: 'y.ts', lineNumber: 5, column: 1, preview: 'second hit' })
  })

  it('says when the repo search found nothing', () => {
    renderPanel()
    act(() => { vi.advanceTimersByTime(REPO_SEARCH_DEBOUNCE_MS) })
    emit({ type: RepoSearchEventType.Done, tool: RepoSearchTool.GitGrep, truncated: false })
    expect(screen.getByText('No matches · git grep')).toBeTruthy()
  })

  it('shows a repo search error, keeping the hits found so far', () => {
    renderPanel()
    act(() => { vi.advanceTimersByTime(REPO_SEARCH_DEBOUNCE_MS) })
    emit({ type: RepoSearchEventType.Hits, hits: [{ filePath: 'x.ts', lineNumber: 1, column: 1, preview: 'kept' }] })
    emit({ type: RepoSearchEventType.Error, message: 'rg exited with code 2: boom' })
    expect(screen.getByText('kept')).toBeTruthy()
    expect(screen.getByText('Search failed: rg exited with code 2: boom')).toBeTruthy()
  })

  it('a new query cancels the running search and starts over', () => {
    const { rerender } = renderPanel()
    act(() => { vi.advanceTimersByTime(REPO_SEARCH_DEBOUNCE_MS) })
    emit({ type: RepoSearchEventType.Hits, hits: [{ filePath: 'x.ts', lineNumber: 1, column: 1, preview: 'old hit' }] })

    rerender(
      <SearchPanel
        workspace={workspace} query="other" onQueryChange={vi.fn()} focusNonce={1} onClose={vi.fn()}
        inViewSections={[{ title: 'In this file', hits: [] }]} activeIndex={null} inViewPending={false}
        onSelectInView={vi.fn()} onNext={vi.fn()} onPrev={vi.fn()} excludeFilePaths={[]} onSelectRepo={vi.fn()}
      />
    )

    expect(cancels[0]).toHaveBeenCalled()
    expect(screen.queryByText('old hit')).toBeNull()
    act(() => { vi.advanceTimersByTime(REPO_SEARCH_DEBOUNCE_MS) })
    expect(startRepoSearch).toHaveBeenLastCalledWith(execApi, 'conn', '/repo', 'other', expect.any(Function))
  })

  it('a query changed during the pause never starts the stale search', () => {
    const { unmount } = renderPanel()
    act(() => { vi.advanceTimersByTime(REPO_SEARCH_DEBOUNCE_MS - 1) })
    unmount()
    act(() => { vi.advanceTimersByTime(REPO_SEARCH_DEBOUNCE_MS) })
    expect(startRepoSearch).not.toHaveBeenCalled()
  })

  it('closing the panel cancels the running search', () => {
    const { unmount } = renderPanel()
    act(() => { vi.advanceTimersByTime(REPO_SEARCH_DEBOUNCE_MS) })
    unmount()
    expect(cancels[0]).toHaveBeenCalled()
  })
})

describe('openInFileBrowser', () => {
  it('opens a file browser tab at the line', () => {
    openInFileBrowser(workspace, 'src/a.ts', 7)
    expect(addTab).toHaveBeenCalledWith('filesystem', { selectedPath: '/repo/src/a.ts', scrollToLine: 7 })
  })
})
