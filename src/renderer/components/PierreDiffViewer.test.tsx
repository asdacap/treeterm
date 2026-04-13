// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import React from 'react'

vi.mock('@pierre/diffs/react', () => ({
  MultiFileDiff: (props: Record<string, unknown>) => {
    const { oldFile, newFile, options, lineAnnotations, renderAnnotation } = props as {
      oldFile: { name: string; contents: string }
      newFile: { name: string; contents: string }
      options: { diffStyle: string; expandUnchanged: boolean; disableFileHeader: boolean }
      lineAnnotations?: Array<{ side: string; lineNumber: number; metadata: unknown }>
      renderAnnotation?: (annotation: unknown) => React.ReactNode
    }
    return (
      <div
        data-testid="multi-file-diff"
        data-old-name={oldFile.name}
        data-new-name={newFile.name}
        data-diff-style={options.diffStyle}
        data-expand-unchanged={String(options.expandUnchanged)}
        data-disable-file-header={String(options.disableFileHeader)}
      >
        {lineAnnotations && renderAnnotation && lineAnnotations.map((a, i) => (
          <div key={i}>{renderAnnotation(a)}</div>
        ))}
      </div>
    )
  },
  WorkerPoolContextProvider: ({ children }: { children: React.ReactNode }) => <>{children}</>,
}))

vi.mock('@pierre/diffs', () => ({}))

vi.mock('../pierre-diffs-config', () => ({
  createDiffsWorker: () => ({} as Worker),
}))

import { PierreDiffViewer } from './PierreDiffViewer'
import type { ReviewComment } from '../types'

function makeComment(overrides: Partial<ReviewComment> = {}): ReviewComment {
  return {
    id: 'c1',
    filePath: 'test.ts',
    lineNumber: 10,
    text: 'test comment',
    commitHash: null,
    createdAt: Date.now(),
    isOutdated: false,
    addressed: false,
    side: 'modified' as const,
    ...overrides,
  }
}

const defaultProps = {
  originalContent: 'line1\nline2\n',
  modifiedContent: 'line1\nline2 changed\n',
  filePath: 'src/test.ts',
  originalLabel: 'base',
  modifiedLabel: 'head',
  hasPreviousFile: false,
  hasNextFile: false,
  comments: [] as ReviewComment[],
  inlineCommentInput: null,
}

describe('PierreDiffViewer', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('renders MultiFileDiff with correct file props', () => {
    render(<PierreDiffViewer {...defaultProps} />)
    const diff = screen.getByTestId('multi-file-diff')
    expect(diff.getAttribute('data-old-name')).toBe('src/test.ts')
    expect(diff.getAttribute('data-new-name')).toBe('src/test.ts')
  })

  it('renders labels in toolbar', () => {
    render(<PierreDiffViewer {...defaultProps} />)
    expect(screen.getByText('base')).toBeDefined()
    expect(screen.getByText('head')).toBeDefined()
  })

  it('defaults to split view', () => {
    render(<PierreDiffViewer {...defaultProps} />)
    const diff = screen.getByTestId('multi-file-diff')
    expect(diff.getAttribute('data-diff-style')).toBe('split')
  })

  it('toggles to unified view on button click', () => {
    render(<PierreDiffViewer {...defaultProps} />)
    const toggleBtn = screen.getByTitle('Switch to unified view')
    fireEvent.click(toggleBtn)
    const diff = screen.getByTestId('multi-file-diff')
    expect(diff.getAttribute('data-diff-style')).toBe('unified')
  })

  it('toggles hide unchanged regions', () => {
    render(<PierreDiffViewer {...defaultProps} />)
    const diff = screen.getByTestId('multi-file-diff')
    expect(diff.getAttribute('data-expand-unchanged')).toBe('true')
    const toggleBtn = screen.getByTitle('Hide unchanged regions')
    fireEvent.click(toggleBtn)
    expect(diff.getAttribute('data-expand-unchanged')).toBe('false')
  })

  it('disableFileHeader is always true', () => {
    render(<PierreDiffViewer {...defaultProps} />)
    const diff = screen.getByTestId('multi-file-diff')
    expect(diff.getAttribute('data-disable-file-header')).toBe('true')
  })

  it('does not render navigation buttons when no handlers provided', () => {
    render(<PierreDiffViewer {...defaultProps} />)
    expect(screen.queryByTitle('Previous file')).toBeNull()
    expect(screen.queryByTitle('Next file')).toBeNull()
  })

  it('renders navigation buttons when handlers provided', () => {
    const onPrev = vi.fn()
    const onNext = vi.fn()
    render(
      <PierreDiffViewer
        {...defaultProps}
        onPreviousFile={onPrev}
        onNextFile={onNext}
        hasPreviousFile={true}
        hasNextFile={false}
      />
    )
    const prevBtn = screen.getByTitle('Previous file')
    const nextBtn = screen.getByTitle('Next file')
    expect((prevBtn as HTMLButtonElement).disabled).toBe(false)
    expect((nextBtn as HTMLButtonElement).disabled).toBe(true)
    fireEvent.click(prevBtn)
    expect(onPrev).toHaveBeenCalledTimes(1)
  })

  it('shows comment count when comments exist', () => {
    const comments = [makeComment({ id: 'c1' }), makeComment({ id: 'c2' })]
    render(<PierreDiffViewer {...defaultProps} comments={comments} />)
    expect(screen.getByTitle('2 comment(s)')).toBeDefined()
  })

  it('does not show comment count when no comments', () => {
    render(<PierreDiffViewer {...defaultProps} />)
    expect(screen.queryByTitle(/comment/)).toBeNull()
  })

  it('renders comment text when comments are provided', () => {
    const comments = [makeComment({ text: 'Fix this bug' })]
    render(<PierreDiffViewer {...defaultProps} comments={comments} />)
    expect(screen.getByText('Fix this bug')).toBeDefined()
  })

  it('renders CommentInput when inlineCommentInput is set', () => {
    const onSubmit = vi.fn()
    const onCancel = vi.fn()
    render(
      <PierreDiffViewer
        {...defaultProps}
        inlineCommentInput={{ lineNumber: 5, side: 'modified' }}
        onCommentSubmit={onSubmit}
        onCommentCancel={onCancel}
      />
    )
    expect(screen.getByText('Comment on line 5 (modified)')).toBeDefined()
  })

  it('calls onCommentDelete when delete is clicked on a comment', () => {
    const onDelete = vi.fn()
    const comments = [makeComment({ id: 'del-1', text: 'delete me' })]
    render(
      <PierreDiffViewer
        {...defaultProps}
        comments={comments}
        onCommentDelete={onDelete}
      />
    )
    const deleteBtn = screen.getByTitle('Delete comment')
    fireEvent.click(deleteBtn)
    expect(onDelete).toHaveBeenCalledWith('del-1')
  })
})
