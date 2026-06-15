// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'

import { getSortedFilePaths, sortFilesAsTree, filterFilesByDir, CommittedDiffFileTree, UncommittedDiffFileTree } from './DiffFileTree'
import type { DiffFile, UncommittedFile } from '../types'
import { FileChangeStatus } from '../types'

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

describe('sortFilesAsTree', () => {
  it('returns files sorted with directories first', () => {
    const files = [
      makeDiffFile({ path: 'z.ts' }),
      makeDiffFile({ path: 'src/a.ts' }),
      makeDiffFile({ path: 'a.ts' }),
    ]
    const sorted = sortFilesAsTree(files)
    expect(sorted.map(f => f.path)).toEqual(['src/a.ts', 'a.ts', 'z.ts'])
  })

  it('preserves original file objects', () => {
    const fileA = makeDiffFile({ path: 'b.ts', additions: 99 })
    const fileB = makeDiffFile({ path: 'a.ts', additions: 1 })
    const sorted = sortFilesAsTree([fileA, fileB])
    expect(sorted[0]).toBe(fileB)
    expect(sorted[1]).toBe(fileA)
  })

  it('returns empty array for empty input', () => {
    expect(sortFilesAsTree([])).toEqual([])
  })
})

describe('filterFilesByDir', () => {
  it('returns all files when dir is null', () => {
    const files = [makeDiffFile({ path: 'a.ts' }), makeDiffFile({ path: 'src/b.ts' })]
    expect(filterFilesByDir(files, null)).toBe(files)
  })

  it('keeps only files under the given directory', () => {
    const files = [
      makeDiffFile({ path: 'src/app.ts' }),
      makeDiffFile({ path: 'src/util/x.ts' }),
      makeDiffFile({ path: 'docs/readme.md' }),
    ]
    expect(filterFilesByDir(files, 'src').map(f => f.path)).toEqual(['src/app.ts', 'src/util/x.ts'])
    expect(filterFilesByDir(files, 'src/util').map(f => f.path)).toEqual(['src/util/x.ts'])
  })

  it('does not match directories that share a name prefix', () => {
    const files = [makeDiffFile({ path: 'src/a.ts' }), makeDiffFile({ path: 'src-gen/b.ts' })]
    expect(filterFilesByDir(files, 'src').map(f => f.path)).toEqual(['src/a.ts'])
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
    const files = [makeDiffFile({ path: 'app.ts', additions: 7, deletions: 3 })]
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

  it('sums additions and deletions per directory', () => {
    const files = [
      makeDiffFile({ path: 'src/a.ts', additions: 3, deletions: 1 }),
      makeDiffFile({ path: 'src/b.ts', additions: 4, deletions: 2 }),
    ]
    const { container } = render(
      <CommittedDiffFileTree
        files={files}
        selectedFile={null}
        onSelectFile={onSelectFile}
        getStatusIcon={getStatusIcon}
      />
    )
    const dirStats = container.querySelector('.diff-tree-dir .diff-file-stats')
    if (!dirStats) throw new Error('expected directory stats')
    expect(dirStats.querySelector('.additions')?.textContent).toBe('+7')
    expect(dirStats.querySelector('.deletions')?.textContent).toBe('-3')
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

  it('sums additions and deletions per directory', () => {
    const files = [
      makeUncommittedFile({ path: 'src/a.ts', additions: 3, deletions: 1 }),
      makeUncommittedFile({ path: 'src/b.ts', additions: 4, deletions: 2 }),
    ]
    const { container } = render(
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
    const dirStats = container.querySelector('.diff-tree-dir .diff-file-stats')
    if (!dirStats) throw new Error('expected directory stats')
    expect(dirStats.querySelector('.additions')?.textContent).toBe('+7')
    expect(dirStats.querySelector('.deletions')?.textContent).toBe('-3')
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

  it('filters to a directory via the right-click context menu', () => {
    const onFilterDir = vi.fn()
    const files = [makeDiffFile({ path: 'src/a.ts' }), makeDiffFile({ path: 'src/b.ts' })]
    const { container } = render(
      <CommittedDiffFileTree
        files={files}
        selectedFile={null}
        onSelectFile={onSelectFile}
        getStatusIcon={getStatusIcon}
        onFilterDir={onFilterDir}
      />
    )
    // No menu item visible until the directory row is right-clicked
    expect(screen.queryByText('Filter to this folder')).toBeNull()

    const dir = container.querySelector('.diff-tree-dir')
    if (!dir) throw new Error('expected a directory row')
    fireEvent.contextMenu(dir)

    fireEvent.click(screen.getByText('Filter to this folder'))
    expect(onFilterDir).toHaveBeenCalledWith('src')
  })
})
