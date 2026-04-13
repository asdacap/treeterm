// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import React from 'react'

// Mock IntersectionObserver
const mockObserve = vi.fn()
const mockDisconnect = vi.fn()
let intersectionCallback: IntersectionObserverCallback | null = null

class MockIntersectionObserver {
  constructor(callback: IntersectionObserverCallback) {
    intersectionCallback = callback
  }
  observe = mockObserve
  disconnect = mockDisconnect
  unobserve = vi.fn()
}

vi.stubGlobal('IntersectionObserver', MockIntersectionObserver)

vi.mock('@pierre/diffs/react', () => ({
  MultiFileDiff: (props: Record<string, unknown>) => {
    const { oldFile, newFile } = props as {
      oldFile: { name: string }
      newFile: { name: string }
    }
    return <div data-testid="multi-file-diff" data-old-name={oldFile.name} data-new-name={newFile.name} />
  },
  WorkerPoolContextProvider: ({ children }: { children: React.ReactNode }) => <>{children}</>,
}))

vi.mock('@pierre/diffs', () => ({}))
vi.mock('../pierre-diffs-config', () => ({
  createDiffsWorker: () => ({} as Worker),
}))

import { FileDiffSection } from './FileDiffSection'
import type { DiffFile, FileDiffContents, ReviewComment } from '../types'
import { FileChangeStatus } from '../types'

function makeDiffFile(overrides: Partial<DiffFile> = {}): DiffFile {
  return {
    path: 'src/app.ts',
    status: FileChangeStatus.Modified,
    additions: 10,
    deletions: 5,
    ...overrides,
  }
}

function makeContents(overrides: Partial<FileDiffContents> = {}): FileDiffContents {
  return {
    originalContent: 'old content',
    modifiedContent: 'new content',
    language: 'typescript',
    ...overrides,
  }
}

const defaultProps = {
  file: makeDiffFile(),
  contents: null as FileDiffContents | null,
  loading: false,
  error: null as string | null,
  onRequestLoad: vi.fn(),
  diffStyle: 'split' as const,
  expandUnchanged: true,
  getStatusIcon: vi.fn((status: FileChangeStatus) => <span data-testid={`icon-${status}`}>M</span>),
  comments: [] as ReviewComment[],
  onLineClick: vi.fn(),
  inlineCommentInput: null,
}

describe('FileDiffSection', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    intersectionCallback = null
  })

  it('renders file header with path and stats', () => {
    render(<FileDiffSection {...defaultProps} />)
    expect(screen.getByText('src/app.ts')).toBeDefined()
    expect(screen.getByText('+10')).toBeDefined()
    expect(screen.getByText('-5')).toBeDefined()
  })

  it('calls getStatusIcon with file status', () => {
    const getStatusIcon = vi.fn(() => <span>M</span>)
    render(<FileDiffSection {...defaultProps} getStatusIcon={getStatusIcon} />)
    expect(getStatusIcon).toHaveBeenCalledWith(FileChangeStatus.Modified)
  })

  it('shows placeholder when no contents and not loading', () => {
    render(<FileDiffSection {...defaultProps} />)
    expect(screen.getByText('Scroll to load diff')).toBeDefined()
  })

  it('shows loading state', () => {
    render(<FileDiffSection {...defaultProps} loading={true} />)
    expect(screen.getByText('Loading diff...')).toBeDefined()
  })

  it('shows error state', () => {
    render(<FileDiffSection {...defaultProps} error="Failed to load" />)
    expect(screen.getByText('Failed to load')).toBeDefined()
  })

  it('renders diff when contents provided', () => {
    render(<FileDiffSection {...defaultProps} contents={makeContents()} />)
    expect(screen.getByTestId('multi-file-diff')).toBeDefined()
  })

  it('collapses body when header is clicked', () => {
    render(<FileDiffSection {...defaultProps} contents={makeContents()} />)
    expect(screen.getByTestId('multi-file-diff')).toBeDefined()

    fireEvent.click(screen.getByText('src/app.ts'))
    expect(screen.queryByTestId('multi-file-diff')).toBeNull()
  })

  it('expands body when header clicked again', () => {
    render(<FileDiffSection {...defaultProps} contents={makeContents()} />)
    // Collapse
    fireEvent.click(screen.getByText('src/app.ts'))
    expect(screen.queryByTestId('multi-file-diff')).toBeNull()
    // Expand
    fireEvent.click(screen.getByText('src/app.ts'))
    expect(screen.getByTestId('multi-file-diff')).toBeDefined()
  })

  it('sets up IntersectionObserver on mount', () => {
    render(<FileDiffSection {...defaultProps} />)
    expect(mockObserve).toHaveBeenCalledTimes(1)
  })

  it('calls onRequestLoad when intersection fires', () => {
    const onRequestLoad = vi.fn()
    render(<FileDiffSection {...defaultProps} onRequestLoad={onRequestLoad} />)
    expect(onRequestLoad).not.toHaveBeenCalled()

    // Simulate intersection
    if (intersectionCallback) {
      intersectionCallback(
        [{ isIntersecting: true } as IntersectionObserverEntry],
        {} as IntersectionObserver
      )
    }
    expect(onRequestLoad).toHaveBeenCalledTimes(1)
    expect(mockDisconnect).toHaveBeenCalled()
  })

  it('does not call onRequestLoad when not intersecting', () => {
    const onRequestLoad = vi.fn()
    render(<FileDiffSection {...defaultProps} onRequestLoad={onRequestLoad} />)

    if (intersectionCallback) {
      intersectionCallback(
        [{ isIntersecting: false } as IntersectionObserverEntry],
        {} as IntersectionObserver
      )
    }
    expect(onRequestLoad).not.toHaveBeenCalled()
  })

  it('renders staging action button when provided', () => {
    const onAction = vi.fn()
    render(
      <FileDiffSection
        {...defaultProps}
        stagingAction={{ label: 'Stage', onAction, disabled: false }}
      />
    )
    expect(screen.getByText('Stage')).toBeDefined()
  })

  it('calls staging action on button click without collapsing', () => {
    const onAction = vi.fn()
    render(
      <FileDiffSection
        {...defaultProps}
        contents={makeContents()}
        stagingAction={{ label: 'Stage', onAction, disabled: false }}
      />
    )
    fireEvent.click(screen.getByText('Stage'))
    expect(onAction).toHaveBeenCalledTimes(1)
    // Should not collapse
    expect(screen.getByTestId('multi-file-diff')).toBeDefined()
  })

  it('disables staging button when disabled', () => {
    render(
      <FileDiffSection
        {...defaultProps}
        stagingAction={{ label: 'Stage', onAction: vi.fn(), disabled: true }}
      />
    )
    expect(screen.getByText<HTMLButtonElement>('Stage').disabled).toBe(true)
  })
})
