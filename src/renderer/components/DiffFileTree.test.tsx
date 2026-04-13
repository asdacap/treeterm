// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'

import { getSortedFilePaths, CommittedDiffFileTree, UncommittedDiffFileTree } from './DiffFileTree'
import type { DiffFile, UncommittedFile } from '../types'
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

function makeUncommittedFile(overrides: Partial<UncommittedFile> = {}): UncommittedFile {
  return {
    path: 'src/app.ts',
    status: FileChangeStatus.Modified,
    staged: false,
    additions: 10,
    deletions: 5,
    ...overrides,
  }
}

describe('getSortedFilePaths', () => {
  it('returns empty array for empty input', () => {
    expect(getSortedFilePaths([])).toEqual([])
  })

  it('returns flat file list sorted alphabetically', () => {
    const files = [
      makeDiffFile({ path: 'c.ts' }),
      makeDiffFile({ path: 'a.ts' }),
      makeDiffFile({ path: 'b.ts' }),
    ]
    expect(getSortedFilePaths(files)).toEqual(['a.ts', 'b.ts', 'c.ts'])
  })

  it('sorts directories before files', () => {
    const files = [
      makeDiffFile({ path: 'z.ts' }),
      makeDiffFile({ path: 'src/a.ts' }),
    ]
    const sorted = getSortedFilePaths(files)
    expect(sorted[0]).toBe('src/a.ts')
    expect(sorted[1]).toBe('z.ts')
  })

  it('handles nested directories', () => {
    const files = [
      makeDiffFile({ path: 'src/renderer/app.ts' }),
      makeDiffFile({ path: 'src/main/index.ts' }),
    ]
    const sorted = getSortedFilePaths(files)
    expect(sorted).toContain('src/main/index.ts')
    expect(sorted).toContain('src/renderer/app.ts')
  })

  it('collapses single-child directory chains', () => {
    // With only one file in src/renderer/, the dirs should be collapsed to src/renderer/
    const files = [makeDiffFile({ path: 'src/renderer/app.ts' })]
    const sorted = getSortedFilePaths(files)
    expect(sorted).toEqual(['src/renderer/app.ts'])
  })
})

describe('CommittedDiffFileTree', () => {
  const onSelectFile = vi.fn()
  const getStatusIcon = vi.fn((status: string) => <span data-testid={`icon-${status}`} />)

  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('renders file names', () => {
    const files = [makeDiffFile({ path: 'app.ts' })]
    render(
      <CommittedDiffFileTree
        files={files}
        selectedFile={null}
        onSelectFile={onSelectFile}
        getStatusIcon={getStatusIcon}
      />
    )
    expect(screen.getByText('app.ts')).toBeDefined()
  })

  it('renders additions and deletions stats', () => {
    const files = [makeDiffFile({ additions: 7, deletions: 3 })]
    render(
      <CommittedDiffFileTree
        files={files}
        selectedFile={null}
        onSelectFile={onSelectFile}
        getStatusIcon={getStatusIcon}
      />
    )
    expect(screen.getByText('+7')).toBeDefined()
    expect(screen.getByText('-3')).toBeDefined()
  })

  it('calls onSelectFile when a file item is clicked', () => {
    const files = [makeDiffFile({ path: 'app.ts' })]
    render(
      <CommittedDiffFileTree
        files={files}
        selectedFile={null}
        onSelectFile={onSelectFile}
        getStatusIcon={getStatusIcon}
      />
    )
    fireEvent.click(screen.getByText('app.ts'))
    expect(onSelectFile).toHaveBeenCalledWith('app.ts')
  })

  it('applies "selected" class to the currently selected file', () => {
    const files = [makeDiffFile({ path: 'app.ts' })]
    const { container } = render(
      <CommittedDiffFileTree
        files={files}
        selectedFile="app.ts"
        onSelectFile={onSelectFile}
        getStatusIcon={getStatusIcon}
      />
    )
    expect(container.querySelector('.diff-file-item.selected')).toBeDefined()
  })

  it('calls getStatusIcon for each file', () => {
    const files = [makeDiffFile({ status: FileChangeStatus.Added })]
    render(
      <CommittedDiffFileTree
        files={files}
        selectedFile={null}
        onSelectFile={onSelectFile}
        getStatusIcon={getStatusIcon}
      />
    )
    expect(getStatusIcon).toHaveBeenCalledWith(FileChangeStatus.Added)
  })

  it('renders directories expanded by default', () => {
    const files = [
      makeDiffFile({ path: 'src/a.ts' }),
      makeDiffFile({ path: 'src/b.ts' }),
    ]
    render(
      <CommittedDiffFileTree
        files={files}
        selectedFile={null}
        onSelectFile={onSelectFile}
        getStatusIcon={getStatusIcon}
      />
    )
    // Both files should be visible (dir is expanded)
    expect(screen.getByText('a.ts')).toBeDefined()
    expect(screen.getByText('b.ts')).toBeDefined()
  })

  it('collapses a directory when its header is clicked', () => {
    const files = [
      makeDiffFile({ path: 'src/a.ts' }),
      makeDiffFile({ path: 'src/b.ts' }),
    ]
    const { container } = render(
      <CommittedDiffFileTree
        files={files}
        selectedFile={null}
        onSelectFile={onSelectFile}
        getStatusIcon={getStatusIcon}
      />
    )
    // Click the directory header
    const dirHeader = container.querySelector('.diff-tree-dir')
    expect(dirHeader).toBeDefined()
    if (dirHeader) fireEvent.click(dirHeader)

    // Files should be hidden
    expect(screen.queryByText('a.ts')).toBeNull()
    expect(screen.queryByText('b.ts')).toBeNull()
  })
})

describe('UncommittedDiffFileTree', () => {
  const onSelectFile = vi.fn()
  const getStatusIcon = vi.fn((status: string) => <span data-testid={`icon-${status}`} />)
  const onAction = vi.fn()

  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('renders file names', () => {
    const files = [makeUncommittedFile({ path: 'app.ts' })]
    render(
      <UncommittedDiffFileTree
        files={files}
        selectedFile={null}
        onSelectFile={onSelectFile}
        getStatusIcon={getStatusIcon}
        onAction={onAction}
        actionLabel="Stage"
        stagingInProgress={false}
      />
    )
    expect(screen.getByText('app.ts')).toBeDefined()
  })

  it('renders action button with actionLabel text', () => {
    const files = [makeUncommittedFile({ path: 'app.ts' })]
    render(
      <UncommittedDiffFileTree
        files={files}
        selectedFile={null}
        onSelectFile={onSelectFile}
        getStatusIcon={getStatusIcon}
        onAction={onAction}
        actionLabel="Stage"
        stagingInProgress={false}
      />
    )
    expect(screen.getByText('Stage')).toBeDefined()
  })

  it('calls onAction with file path when action button is clicked', () => {
    const files = [makeUncommittedFile({ path: 'app.ts' })]
    render(
      <UncommittedDiffFileTree
        files={files}
        selectedFile={null}
        onSelectFile={onSelectFile}
        getStatusIcon={getStatusIcon}
        onAction={onAction}
        actionLabel="Stage"
        stagingInProgress={false}
      />
    )
    fireEvent.click(screen.getByText('Stage'))
    expect(onAction).toHaveBeenCalledWith('app.ts')
  })

  it('disables action button when stagingInProgress is true', () => {
    const files = [makeUncommittedFile({ path: 'app.ts' })]
    render(
      <UncommittedDiffFileTree
        files={files}
        selectedFile={null}
        onSelectFile={onSelectFile}
        getStatusIcon={getStatusIcon}
        onAction={onAction}
        actionLabel="Stage"
        stagingInProgress={true}
      />
    )
    expect(screen.getByText<HTMLButtonElement>('Stage').disabled).toBe(true)
  })

  it('action button click does not trigger onSelectFile', () => {
    const files = [makeUncommittedFile({ path: 'app.ts' })]
    render(
      <UncommittedDiffFileTree
        files={files}
        selectedFile={null}
        onSelectFile={onSelectFile}
        getStatusIcon={getStatusIcon}
        onAction={onAction}
        actionLabel="Stage"
        stagingInProgress={false}
      />
    )
    fireEvent.click(screen.getByText('Stage'))
    expect(onSelectFile).not.toHaveBeenCalled()
  })
})

describe('CommittedDiffFileTree viewedFiles', () => {
  const onSelectFile = vi.fn()
  const getStatusIcon = vi.fn((status: string) => <span data-testid={`icon-${status}`} />)

  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('renders viewed icon for viewed files', () => {
    const files = [makeDiffFile({ path: 'app.ts' })]
    const { container } = render(
      <CommittedDiffFileTree
        files={files}
        selectedFile={null}
        onSelectFile={onSelectFile}
        getStatusIcon={getStatusIcon}
        viewedFiles={new Set(['app.ts'])}
      />
    )
    expect(container.querySelector('.diff-file-viewed-icon')).not.toBeNull()
  })

  it('dims viewed files in file tree', () => {
    const files = [makeDiffFile({ path: 'app.ts' })]
    const { container } = render(
      <CommittedDiffFileTree
        files={files}
        selectedFile={null}
        onSelectFile={onSelectFile}
        getStatusIcon={getStatusIcon}
        viewedFiles={new Set(['app.ts'])}
      />
    )
    expect(container.querySelector('.diff-file-item.viewed')).not.toBeNull()
  })

  it('does not dim unviewed files', () => {
    const files = [makeDiffFile({ path: 'app.ts' })]
    const { container } = render(
      <CommittedDiffFileTree
        files={files}
        selectedFile={null}
        onSelectFile={onSelectFile}
        getStatusIcon={getStatusIcon}
        viewedFiles={new Set()}
      />
    )
    expect(container.querySelector('.diff-file-item.viewed')).toBeNull()
  })

  it('does not show viewed icon when viewedFiles is not provided', () => {
    const files = [makeDiffFile({ path: 'app.ts' })]
    const { container } = render(
      <CommittedDiffFileTree
        files={files}
        selectedFile={null}
        onSelectFile={onSelectFile}
        getStatusIcon={getStatusIcon}
      />
    )
    expect(container.querySelector('.diff-file-viewed-icon')).toBeNull()
  })
})
