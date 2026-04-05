// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import React from 'react'
import { CommentDisplay } from './CommentDisplay'
import type { ReviewComment } from '../types'

function makeComment(overrides: Partial<ReviewComment> = {}): ReviewComment {
  return {
    id: 'c1',
    filePath: 'src/app.ts',
    lineNumber: 10,
    text: 'Fix this bug',
    commitHash: 'abc123',
    createdAt: 1700000000000,
    isOutdated: false,
    addressed: false,
    side: 'modified',
    ...overrides,
  }
}

describe('CommentDisplay', () => {
  const onDelete = vi.fn()

  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('renders comment text', () => {
    render(<CommentDisplay comment={makeComment()} onDelete={onDelete} />)
    expect(screen.getByText('Fix this bug')).toBeDefined()
  })

  it('renders line number and side in header', () => {
    render(<CommentDisplay comment={makeComment()} onDelete={onDelete} />)
    expect(screen.getByText('Line 10 (modified)')).toBeDefined()
  })

  it('renders formatted creation date', () => {
    const { container } = render(
      <CommentDisplay comment={makeComment()} onDelete={onDelete} />
    )
    const meta = container.querySelector('.comment-display-meta')!
    expect(meta.textContent).toBe(new Date(1700000000000).toLocaleString())
  })

  it('shows "Outdated" badge when comment is outdated', () => {
    render(
      <CommentDisplay comment={makeComment({ isOutdated: true })} onDelete={onDelete} />
    )
    expect(screen.getByText('Outdated')).toBeDefined()
  })

  it('hides "Outdated" badge when comment is not outdated', () => {
    render(<CommentDisplay comment={makeComment()} onDelete={onDelete} />)
    expect(screen.queryByText('Outdated')).toBeNull()
  })

  it('applies outdated CSS class when comment is outdated', () => {
    const { container } = render(
      <CommentDisplay comment={makeComment({ isOutdated: true })} onDelete={onDelete} />
    )
    expect(container.querySelector('.comment-display.outdated')).toBeDefined()
  })

  it('hides line ref but shows header when hideLineRef=true and isOutdated=true', () => {
    render(
      <CommentDisplay
        comment={makeComment({ isOutdated: true })}
        onDelete={onDelete}
        hideLineRef
      />
    )
    expect(screen.queryByText(/Line 10/)).toBeNull()
    expect(screen.getByText('Outdated')).toBeDefined()
  })

  it('hides entire header when hideLineRef=true and isOutdated=false', () => {
    const { container } = render(
      <CommentDisplay comment={makeComment()} onDelete={onDelete} hideLineRef />
    )
    expect(container.querySelector('.comment-display-header')).toBeNull()
  })

  it('renders inline delete button when header is hidden', () => {
    const { container } = render(
      <CommentDisplay comment={makeComment()} onDelete={onDelete} hideLineRef />
    )
    const inlineBtn = container.querySelector('.comment-delete-btn.inline')
    expect(inlineBtn).toBeDefined()
  })

  it('calls onDelete with comment.id when delete button is clicked', () => {
    const { container } = render(
      <CommentDisplay comment={makeComment({ id: 'c42' })} onDelete={onDelete} />
    )
    const btn = container.querySelector('.comment-delete-btn')!
    fireEvent.click(btn)
    expect(onDelete).toHaveBeenCalledWith('c42')
  })

  it('calls onDelete from inline delete button', () => {
    const { container } = render(
      <CommentDisplay
        comment={makeComment({ id: 'c99' })}
        onDelete={onDelete}
        hideLineRef
      />
    )
    const btn = container.querySelector('.comment-delete-btn.inline')!
    fireEvent.click(btn)
    expect(onDelete).toHaveBeenCalledWith('c99')
  })
})
