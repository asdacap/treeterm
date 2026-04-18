// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, act } from '@testing-library/react'
import React from 'react'

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

import { CommitsLoadMoreSentinel } from './ReviewBrowser'

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
