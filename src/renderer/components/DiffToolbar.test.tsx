// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'

import { DiffToolbar } from './DiffToolbar'

const defaultProps = {
  isSplitView: true,
  onToggleSplit: vi.fn(),
  hideUnchanged: false,
  onToggleHideUnchanged: vi.fn(),
  totalComments: 0,
}

describe('DiffToolbar', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('renders split view toggle', () => {
    render(<DiffToolbar {...defaultProps} />)
    expect(screen.getByTitle('Switch to unified view')).toBeDefined()
  })

  it('renders unified view toggle when not split', () => {
    render(<DiffToolbar {...defaultProps} isSplitView={false} />)
    expect(screen.getByTitle('Switch to split view')).toBeDefined()
  })

  it('calls onToggleSplit on click', () => {
    const onToggleSplit = vi.fn()
    render(<DiffToolbar {...defaultProps} onToggleSplit={onToggleSplit} />)
    fireEvent.click(screen.getByTitle('Switch to unified view'))
    expect(onToggleSplit).toHaveBeenCalledTimes(1)
  })

  it('renders hide unchanged toggle', () => {
    render(<DiffToolbar {...defaultProps} />)
    expect(screen.getByTitle('Hide unchanged regions')).toBeDefined()
  })

  it('renders show unchanged toggle when hidden', () => {
    render(<DiffToolbar {...defaultProps} hideUnchanged={true} />)
    expect(screen.getByTitle('Show unchanged regions')).toBeDefined()
  })

  it('calls onToggleHideUnchanged on click', () => {
    const onToggleHideUnchanged = vi.fn()
    render(<DiffToolbar {...defaultProps} onToggleHideUnchanged={onToggleHideUnchanged} />)
    fireEvent.click(screen.getByTitle('Hide unchanged regions'))
    expect(onToggleHideUnchanged).toHaveBeenCalledTimes(1)
  })

  it('shows comment count when totalComments > 0', () => {
    render(<DiffToolbar {...defaultProps} totalComments={5} />)
    expect(screen.getByTitle('5 comment(s)')).toBeDefined()
  })

  it('does not show comment count when totalComments is 0', () => {
    render(<DiffToolbar {...defaultProps} totalComments={0} />)
    expect(screen.queryByTitle(/comment/)).toBeNull()
  })
})
