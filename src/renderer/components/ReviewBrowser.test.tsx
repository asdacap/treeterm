// @vitest-environment jsdom
/* eslint-disable custom/no-string-literal-comparison -- tests compare DOM text content against literal branch names */
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, act, fireEvent, waitFor } from '@testing-library/react'
import React from 'react'
import type { WorkspaceGitApi } from '../types'

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

// Stub pierre-diffs imports pulled in transitively via ReviewBrowser.
vi.mock('@pierre/diffs/react', () => ({
  MultiFileDiff: () => <div />,
  WorkerPoolContextProvider: ({ children }: { children: React.ReactNode }) => <>{children}</>,
}))
vi.mock('@pierre/diffs', () => ({}))
vi.mock('../pierre-diffs-config', () => ({
  createDiffsWorker: () => ({} as Worker),
}))

import { CommitsLoadMoreSentinel, BaseBranchSelector } from './ReviewBrowser'

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
    const visible = Array.from(container.querySelectorAll('.base-branch-dropdown-item')).map(n => n.textContent)
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
