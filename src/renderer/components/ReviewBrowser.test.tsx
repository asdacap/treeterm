// @vitest-environment jsdom
/* eslint-disable custom/no-string-literal-comparison -- tests compare DOM text content against literal branch names */
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, act, fireEvent, waitFor, screen } from '@testing-library/react'
import React from 'react'
import type { UncommittedFile, WorkspaceFilesystemApi, WorkspaceGitApi, WorkspaceStore } from '../types'
import { FileChangeStatus } from '../types'
import type { ReviewCommentState } from '../store/createReviewCommentStore'
import type { ReviewViewedFilesState } from '../store/createReviewViewedFilesStore'
import type { GitControllerState } from '../store/createGitControllerStore'
import { createStore } from 'zustand/vanilla'
import { makeWorkspace } from '../../shared/test-fixtures/workspace'

const mockObservers: Array<{
  callback: IntersectionObserverCallback
  observe: ReturnType<typeof vi.fn>
  disconnect: ReturnType<typeof vi.fn>
  unobserve: ReturnType<typeof vi.fn>
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

// jsdom has no layout, so StackedDiffList's scroll-to-file effect needs a stub.
const scrollIntoView = vi.fn()
Element.prototype.scrollIntoView = scrollIntoView

// Stub pierre-diffs imports pulled in transitively via ReviewBrowser.
vi.mock('@pierre/diffs/react', () => ({
  MultiFileDiff: () => <div />,
  WorkerPoolContextProvider: ({ children }: { children: React.ReactNode }) => <>{children}</>,
}))
vi.mock('@pierre/diffs', () => ({}))
vi.mock('../pierre-diffs-config', () => ({
  createDiffsWorker: () => ({} as Worker),
}))
vi.mock('./FileViewer', () => ({
  FileViewer: () => <div data-testid="file-viewer" />,
}))
// DiffFileTree pulls in the context-menu store (→ app store → monaco) transitively;
// mock it so this lightweight suite stays free of the editor stack.
vi.mock('../store/contextMenu', async () => {
  const { create } = await import('zustand')
  const store = create<{
    activeMenuId: string | null
    position: { x: number; y: number }
    open: (menuId: string, x: number, y: number) => void
    close: () => void
  }>()((set) => ({
    activeMenuId: null,
    position: { x: 0, y: 0 },
    open: (menuId, x, y) => { set({ activeMenuId: menuId, position: { x, y } }); },
    close: () => { set({ activeMenuId: null }); },
  }))
  return {
    useContextMenuStore: store,
    handleClickOutside: vi.fn(),
    installClickListener: vi.fn(),
  }
})

import ReviewBrowser, { CommitsLoadMoreSentinel, BaseBranchSelector } from './ReviewBrowser'

function fireIntersection(index: number, isIntersecting: boolean) {
  const observer = mockObservers[index]
  if (!observer) throw new Error(`No observer at index ${String(index)}`)
  act(() => {
    observer.callback(
      [{ isIntersecting } as IntersectionObserverEntry],
      observer as unknown as IntersectionObserver
    )
  })
}

describe('CommitsLoadMoreSentinel', () => {
  beforeEach(() => {
    mockObservers.length = 0
    vi.clearAllMocks()
  })

  it('calls onLoadMore when sentinel scrolls into view', () => {
    const onLoadMore = vi.fn()
    render(<CommitsLoadMoreSentinel loading={false} onLoadMore={onLoadMore} />)
    fireIntersection(0, true)
    expect(onLoadMore).toHaveBeenCalledTimes(1)
  })

  it('does not call onLoadMore when sentinel is not intersecting', () => {
    const onLoadMore = vi.fn()
    render(<CommitsLoadMoreSentinel loading={false} onLoadMore={onLoadMore} />)
    fireIntersection(0, false)
    expect(onLoadMore).not.toHaveBeenCalled()
  })

  it('does not observe while loading — observer is skipped so no callback fires', () => {
    const onLoadMore = vi.fn()
    const { rerender } = render(<CommitsLoadMoreSentinel loading={true} onLoadMore={onLoadMore} />)
    // No observer should have been created while loading
    expect(mockObservers.length).toBe(0)
    // Once loading ends, a fresh observer attaches
    rerender(<CommitsLoadMoreSentinel loading={false} onLoadMore={onLoadMore} />)
    expect(mockObservers.length).toBe(1)
    fireIntersection(0, true)
    expect(onLoadMore).toHaveBeenCalledTimes(1)
  })

  it('re-attaches observer after a load cycle so it can fire again if still in view', () => {
    const onLoadMore = vi.fn()
    const { rerender } = render(<CommitsLoadMoreSentinel loading={false} onLoadMore={onLoadMore} />)
    // First intersection → fires
    fireIntersection(0, true)
    expect(onLoadMore).toHaveBeenCalledTimes(1)
    // Simulate the parent flipping loading on, then back off after fetch completes
    rerender(<CommitsLoadMoreSentinel loading={true} onLoadMore={onLoadMore} />)
    rerender(<CommitsLoadMoreSentinel loading={false} onLoadMore={onLoadMore} />)
    // A new observer is attached — if sentinel is still in view, it can fire again
    const latestObserverIndex = mockObservers.length - 1
    fireIntersection(latestObserverIndex, true)
    expect(onLoadMore).toHaveBeenCalledTimes(2)
  })

  it('shows loading text when loading is true', () => {
    const { container } = render(<CommitsLoadMoreSentinel loading={true} onLoadMore={vi.fn()} />)
    expect(container.textContent).toContain('Loading')
  })
})

/** Build a minimal WorkspaceGitApi with only the methods BaseBranchSelector calls.
 *  Other methods throw so the test fails loudly if the component ever depends on them. */
function makeGitApiStub(overrides: Partial<WorkspaceGitApi> = {}): WorkspaceGitApi {
  const notImplemented = (name: string) => () => { throw new Error(`${name} not stubbed`) }
  return new Proxy({
    listLocalBranches: vi.fn().mockResolvedValue(['main', 'feature-a']),
    listRemoteBranches: vi.fn().mockResolvedValue(['origin/main', 'origin/release']),
    ...overrides,
  } as unknown as WorkspaceGitApi, {
    get(target, prop: string) {
      if (prop in target) return (target as unknown as Record<string, unknown>)[prop]
      return notImplemented(prop)
    },
  })
}

describe('BaseBranchSelector', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('shows the current base in the button label', () => {
    const git = makeGitApiStub()
    const { container } = render(
      <BaseBranchSelector
        git={git}
        currentBase="main"
        defaultBase="main"
        isOverridden={false}
        onChange={vi.fn()}
      />
    )
    const button = container.querySelector('.review-branch-selector')
    expect(button).not.toBeNull()
    expect(button!.textContent).toContain('main')
    expect(button!.className).not.toContain('overridden')
  })

  it('falls back to a placeholder label and marks button empty when currentBase is undefined', () => {
    const git = makeGitApiStub()
    const { container } = render(
      <BaseBranchSelector
        git={git}
        currentBase={undefined}
        defaultBase={undefined}
        isOverridden={false}
        onChange={vi.fn()}
      />
    )
    const button = container.querySelector('.review-branch-selector')
    expect(button!.textContent).toContain('Pick base branch')
    expect(button!.className).toContain('empty')
  })

  it('applies the overridden class when isOverridden is true', () => {
    const git = makeGitApiStub()
    const { container } = render(
      <BaseBranchSelector
        git={git}
        currentBase="feature-a"
        defaultBase="main"
        isOverridden={true}
        onChange={vi.fn()}
      />
    )
    const button = container.querySelector('.review-branch-selector')
    expect(button!.className).toContain('overridden')
  })

  it('loads and displays branches when opened, deduping local and remote', async () => {
    const git = makeGitApiStub({
      listLocalBranches: vi.fn().mockResolvedValue(['main', 'feature-a']),
      listRemoteBranches: vi.fn().mockResolvedValue(['main', 'origin/release']),
    })
    const { container } = render(
      <BaseBranchSelector
        git={git}
        currentBase="main"
        defaultBase="main"
        isOverridden={false}
        onChange={vi.fn()}
      />
    )
    fireEvent.click(container.querySelector('.review-branch-selector')!)
    await waitFor(() => {
      expect(container.querySelectorAll('.base-branch-dropdown-item').length).toBe(3)
    })
    const labels = Array.from(container.querySelectorAll('.base-branch-dropdown-item')).map(n => n.textContent)
    expect(labels).toEqual(['main', 'feature-a', 'origin/release'])
  })

  it('filters the branch list by the search input', async () => {
    const git = makeGitApiStub({
      listLocalBranches: vi.fn().mockResolvedValue(['main', 'feature-a', 'feature-b']),
      listRemoteBranches: vi.fn().mockResolvedValue([]),
    })
    const { container } = render(
      <BaseBranchSelector
        git={git}
        currentBase="main"
        defaultBase="main"
        isOverridden={false}
        onChange={vi.fn()}
      />
    )
    fireEvent.click(container.querySelector('.review-branch-selector')!)
    await waitFor(() => {
      expect(container.querySelectorAll('.base-branch-dropdown-item').length).toBe(3)
    })
    const input = container.querySelector('.base-branch-dropdown-search input')!
    fireEvent.change(input, { target: { value: 'feature' } })
    const visible = Array.from(
      container.querySelectorAll('.base-branch-dropdown-item:not(.custom-ref):not(.reset)')
    ).map(n => n.textContent)
    expect(visible).toEqual(['feature-a', 'feature-b'])
  })

  it('calls onChange with the picked branch and closes the dropdown', async () => {
    const git = makeGitApiStub()
    const onChange = vi.fn()
    const { container } = render(
      <BaseBranchSelector
        git={git}
        currentBase="main"
        defaultBase="main"
        isOverridden={false}
        onChange={onChange}
      />
    )
    fireEvent.click(container.querySelector('.review-branch-selector')!)
    await waitFor(() => {
      expect(container.querySelectorAll('.base-branch-dropdown-item').length).toBeGreaterThan(0)
    })
    const featureItem = Array.from(container.querySelectorAll('.base-branch-dropdown-item'))
      .find(n => n.textContent === 'feature-a')!
    fireEvent.click(featureItem)
    expect(onChange).toHaveBeenCalledWith('feature-a')
    expect(container.querySelector('.base-branch-dropdown')).toBeNull()
  })

  it('shows Reset to default and calls onChange(undefined) when overridden', async () => {
    const git = makeGitApiStub()
    const onChange = vi.fn()
    const { container } = render(
      <BaseBranchSelector
        git={git}
        currentBase="feature-a"
        defaultBase="main"
        isOverridden={true}
        onChange={onChange}
      />
    )
    fireEvent.click(container.querySelector('.review-branch-selector')!)
    await waitFor(() => {
      expect(container.querySelector('.base-branch-dropdown-item.reset')).not.toBeNull()
    })
    const resetItem = container.querySelector('.base-branch-dropdown-item.reset')!
    expect(resetItem.textContent).toContain('Reset to default (main)')
    fireEvent.click(resetItem)
    expect(onChange).toHaveBeenCalledWith(undefined)
  })

  it('hides Reset to default when not overridden', async () => {
    const git = makeGitApiStub()
    const { container } = render(
      <BaseBranchSelector
        git={git}
        currentBase="main"
        defaultBase="main"
        isOverridden={false}
        onChange={vi.fn()}
      />
    )
    fireEvent.click(container.querySelector('.review-branch-selector')!)
    await waitFor(() => {
      expect(container.querySelectorAll('.base-branch-dropdown-item').length).toBeGreaterThan(0)
    })
    expect(container.querySelector('.base-branch-dropdown-item.reset')).toBeNull()
  })

  it('shows an error message when branch loading fails', async () => {
    const git = makeGitApiStub({
      listLocalBranches: vi.fn().mockRejectedValue(new Error('git: command failed')),
    })
    const { container } = render(
      <BaseBranchSelector
        git={git}
        currentBase="main"
        defaultBase="main"
        isOverridden={false}
        onChange={vi.fn()}
      />
    )
    fireEvent.click(container.querySelector('.review-branch-selector')!)
    await waitFor(() => {
      expect(container.querySelector('.base-branch-dropdown-error')).not.toBeNull()
    })
    expect(container.querySelector('.base-branch-dropdown-error')!.textContent).toContain('git: command failed')
  })

  it('offers the typed text as a custom commit/ref and calls onChange with it', async () => {
    const git = makeGitApiStub({
      listLocalBranches: vi.fn().mockResolvedValue(['main', 'feature-a']),
      listRemoteBranches: vi.fn().mockResolvedValue([]),
    })
    const onChange = vi.fn()
    const { container } = render(
      <BaseBranchSelector
        git={git}
        currentBase="main"
        defaultBase="main"
        isOverridden={false}
        onChange={onChange}
      />
    )
    fireEvent.click(container.querySelector('.review-branch-selector')!)
    await waitFor(() => {
      expect(container.querySelectorAll('.base-branch-dropdown-item').length).toBeGreaterThan(0)
    })
    const input = container.querySelector('.base-branch-dropdown-search input')!
    fireEvent.change(input, { target: { value: 'a1b2c3d' } })
    const custom = container.querySelector('.base-branch-dropdown-item.custom-ref')!
    expect(custom).not.toBeNull()
    expect(custom.textContent).toContain('a1b2c3d')
    fireEvent.click(custom)
    expect(onChange).toHaveBeenCalledWith('a1b2c3d')
    expect(container.querySelector('.base-branch-dropdown')).toBeNull()
  })

  it('does not offer a custom ref when the typed text exactly names a branch', async () => {
    const git = makeGitApiStub({
      listLocalBranches: vi.fn().mockResolvedValue(['main', 'feature-a']),
      listRemoteBranches: vi.fn().mockResolvedValue([]),
    })
    const { container } = render(
      <BaseBranchSelector
        git={git}
        currentBase="main"
        defaultBase="main"
        isOverridden={false}
        onChange={vi.fn()}
      />
    )
    fireEvent.click(container.querySelector('.review-branch-selector')!)
    await waitFor(() => {
      expect(container.querySelectorAll('.base-branch-dropdown-item').length).toBeGreaterThan(0)
    })
    const input = container.querySelector('.base-branch-dropdown-search input')!
    fireEvent.change(input, { target: { value: 'feature-a' } })
    expect(container.querySelector('.base-branch-dropdown-item.custom-ref')).toBeNull()
  })

  it('truncates a long commit-hash base in the button label while keeping it in the tooltip', () => {
    const git = makeGitApiStub()
    const fullHash = 'a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0'
    const { container } = render(
      <BaseBranchSelector
        git={git}
        currentBase={fullHash}
        defaultBase="main"
        isOverridden={true}
        onChange={vi.fn()}
      />
    )
    const label = container.querySelector('.review-branch-selector-label')!
    expect(label.textContent).toBe('a1b2c3d4e5f6…')
    const button = container.querySelector('.review-branch-selector')!
    expect(button.getAttribute('title')).toContain(fullHash)
  })

  it('does not refetch branches on subsequent opens', async () => {
    const listLocalBranches = vi.fn().mockResolvedValue(['main'])
    const listRemoteBranches = vi.fn().mockResolvedValue([])
    const git = makeGitApiStub({ listLocalBranches, listRemoteBranches })
    const { container } = render(
      <BaseBranchSelector
        git={git}
        currentBase="main"
        defaultBase="main"
        isOverridden={false}
        onChange={vi.fn()}
      />
    )
    const button = container.querySelector('.review-branch-selector')!
    fireEvent.click(button)
    await waitFor(() => {
      expect(container.querySelectorAll('.base-branch-dropdown-item').length).toBe(1)
    })
    fireEvent.click(button) // close
    fireEvent.click(button) // reopen
    await waitFor(() => {
      expect(container.querySelector('.base-branch-dropdown')).not.toBeNull()
    })
    expect(listLocalBranches).toHaveBeenCalledTimes(1)
    expect(listRemoteBranches).toHaveBeenCalledTimes(1)
  })
})

function makeReviewWorkspace(
  favouritePaths: string[],
  viewMode: string,
  uncommittedFiles: UncommittedFile[] = []
): WorkspaceStore {
  const reviewComments = createStore<ReviewCommentState>()(() => ({
    getReviewComments: () => [],
    addReviewComment: vi.fn(),
    deleteReviewComment: vi.fn(),
    toggleReviewCommentAddressed: vi.fn(),
    updateOutdatedReviewComments: vi.fn(),
    clearReviewComments: vi.fn(),
    markReviewCommentsAddressed: vi.fn(),
  }))
  const reviewViewedFiles = createStore<ReviewViewedFilesState>()(() => ({
    getViewedFiles: () => ({}),
    toggleViewedFile: vi.fn(),
    markFilesViewed: vi.fn(),
    reconcileViewedFiles: vi.fn(),
  }))
  const gitController = createStore<GitControllerState>()(() => ({ refreshGit: vi.fn() } as unknown as GitControllerState))
  const gitApi = new Proxy({
    getUncommittedChanges: vi.fn().mockResolvedValue({
      success: true,
      changes: { files: uncommittedFiles, totalAdditions: 0, totalDeletions: 0 },
    }),
    getUncommittedFileContentsForDiff: vi.fn().mockResolvedValue({
      success: true,
      contents: { original: '', modified: '', language: 'typescript' },
    }),
    getHeadCommitHash: vi.fn().mockResolvedValue({ success: true, hash: 'head' }),
  } as unknown as WorkspaceGitApi, {
    get(target, property: string) {
      if (property in target) return (target as unknown as Record<string, unknown>)[property]
      return vi.fn().mockResolvedValue({ success: true })
    },
  })
  const filesystemApi: WorkspaceFilesystemApi = {
    readDirectory: vi.fn((path: string) => Promise.resolve({
      success: true as const,
      contents: {
        path,
        entries: path === '/repo'
          ? [{ name: 'src', path: '/repo/src', relativePath: 'src', isDirectory: true }]
          : [{ name: 'index.ts', path: '/repo/src/index.ts', relativePath: 'src/index.ts', isDirectory: false }],
      },
    })),
    readFile: vi.fn(),
    writeFile: vi.fn(),
    searchFiles: vi.fn(),
  }
  const workspace = makeWorkspace({
    path: '/repo',
    favouritePaths,
    appStates: { review: { applicationId: 'review', title: 'Review', state: { viewMode } } },
  })
  const store = createStore(() => ({
    workspace,
    lookupWorkspace: () => undefined,
    promptHarness: vi.fn(),
    mergeAndRemove: vi.fn(),
    mergeAndKeep: vi.fn(),
    closeAndClean: vi.fn(),
    removeTab: vi.fn(),
    reviewComments,
    reviewViewedFiles,
    gitController,
    updateTabState: vi.fn(),
    gitApi,
    filesystemApi,
    favouritePathsRevision: 0,
    getFavouritePaths: () => favouritePaths,
  }))
  return store as unknown as WorkspaceStore
}

describe('ReviewBrowser favourites', () => {
  it('lists favourite directory contents as a section of the change list, not a tab', async () => {
    const workspace = makeReviewWorkspace(['src'], 'uncommitted')

    const { container } = render(<ReviewBrowser workspace={workspace} tabId="review" isVisible={false} />)

    expect(await screen.findByText('src/index.ts')).toBeDefined()
    // The section lives inside the left change list rather than replacing the whole view.
    expect(container.querySelector('.diff-file-list')?.textContent).toContain('src/index.ts')
    expect(screen.queryByRole('button', { name: /Favourites/ })).toBeNull()
  })

  it('renders no favourites section when no favourite resolves in the worktree', async () => {
    const workspace = makeReviewWorkspace([], 'uncommitted')

    render(<ReviewBrowser workspace={workspace} tabId="review" isVisible={false} />)

    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Uncommitted' }).className).toContain('active')
    })
    expect(screen.queryByText('Favourites')).toBeNull()
  })

  it('falls back to Uncommitted for a view mode persisted before favourites became a section', async () => {
    const workspace = makeReviewWorkspace(['src'], 'favourites')

    render(<ReviewBrowser workspace={workspace} tabId="review" isVisible={false} />)

    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Uncommitted' }).className).toContain('active')
    })
  })

  it('opens a favourite that is absent from the diff in the file viewer', async () => {
    const workspace = makeReviewWorkspace(['src'], 'uncommitted')

    render(<ReviewBrowser workspace={workspace} tabId="review" isVisible={false} />)

    // Nothing is auto-selected — the diff pane stays in charge until the user picks a favourite.
    expect(screen.queryByTestId('file-viewer')).toBeNull()

    fireEvent.click(await screen.findByText('src/index.ts'))

    expect(screen.getByTestId('file-viewer')).toBeDefined()
  })

  it('scrolls the diff instead of opening the viewer when the favourite is part of the diff', async () => {
    const workspace = makeReviewWorkspace(['src'], 'uncommitted', [{
      path: 'src/index.ts',
      status: FileChangeStatus.Modified,
      staged: false,
      additions: 1,
      deletions: 0,
    }])

    const { container } = render(<ReviewBrowser workspace={workspace} tabId="review" isVisible={false} />)

    // The same path is in the Unstaged tree, so anchor on the marker only the
    // favourites section renders.
    await waitFor(() => {
      expect(container.querySelector('.review-favourite-changed')).not.toBeNull()
    })
    const favourite = container.querySelector('.review-favourite-changed')!.closest('.diff-file-item')!

    scrollIntoView.mockClear()
    fireEvent.click(favourite)

    expect(scrollIntoView).toHaveBeenCalled()
    expect(screen.queryByTestId('file-viewer')).toBeNull()
  })
})
