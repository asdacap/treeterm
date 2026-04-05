// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import React from 'react'
import { CommentInput } from './CommentInput'

describe('CommentInput', () => {
  const onSubmit = vi.fn()
  const onCancel = vi.fn()

  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('renders header with line number', () => {
    render(<CommentInput lineNumber={42} onSubmit={onSubmit} onCancel={onCancel} />)
    expect(screen.getByText('Comment on line 42')).toBeDefined()
  })

  it('renders header with line number and side when side is provided', () => {
    render(
      <CommentInput lineNumber={42} side="original" onSubmit={onSubmit} onCancel={onCancel} />
    )
    expect(screen.getByText('Comment on line 42 (original)')).toBeDefined()
  })

  it('submit button is disabled when textarea is empty', () => {
    render(<CommentInput lineNumber={1} onSubmit={onSubmit} onCancel={onCancel} />)
    const btn = screen.getByText('Add Comment')
    expect((btn as HTMLButtonElement).disabled).toBe(true)
  })

  it('submit button is disabled when textarea contains only whitespace', () => {
    render(<CommentInput lineNumber={1} onSubmit={onSubmit} onCancel={onCancel} />)
    const textarea = screen.getByPlaceholderText(/Add your comment/)
    fireEvent.change(textarea, { target: { value: '   ' } })
    const btn = screen.getByText('Add Comment')
    expect((btn as HTMLButtonElement).disabled).toBe(true)
  })

  it('submit button is enabled when textarea has content', () => {
    render(<CommentInput lineNumber={1} onSubmit={onSubmit} onCancel={onCancel} />)
    const textarea = screen.getByPlaceholderText(/Add your comment/)
    fireEvent.change(textarea, { target: { value: 'hello' } })
    const btn = screen.getByText('Add Comment')
    expect((btn as HTMLButtonElement).disabled).toBe(false)
  })

  it('calls onSubmit with trimmed text when Add Comment is clicked', () => {
    render(<CommentInput lineNumber={1} onSubmit={onSubmit} onCancel={onCancel} />)
    const textarea = screen.getByPlaceholderText(/Add your comment/)
    fireEvent.change(textarea, { target: { value: '  hello world  ' } })
    fireEvent.click(screen.getByText('Add Comment'))
    expect(onSubmit).toHaveBeenCalledWith('hello world')
  })

  it('does not call onSubmit when text is empty on button click', () => {
    render(<CommentInput lineNumber={1} onSubmit={onSubmit} onCancel={onCancel} />)
    fireEvent.click(screen.getByText('Add Comment'))
    expect(onSubmit).not.toHaveBeenCalled()
  })

  it('calls onSubmit on Cmd+Enter (metaKey)', () => {
    render(<CommentInput lineNumber={1} onSubmit={onSubmit} onCancel={onCancel} />)
    const textarea = screen.getByPlaceholderText(/Add your comment/)
    fireEvent.change(textarea, { target: { value: 'test' } })
    fireEvent.keyDown(textarea, { key: 'Enter', metaKey: true })
    expect(onSubmit).toHaveBeenCalledWith('test')
  })

  it('calls onSubmit on Ctrl+Enter (ctrlKey)', () => {
    render(<CommentInput lineNumber={1} onSubmit={onSubmit} onCancel={onCancel} />)
    const textarea = screen.getByPlaceholderText(/Add your comment/)
    fireEvent.change(textarea, { target: { value: 'test' } })
    fireEvent.keyDown(textarea, { key: 'Enter', ctrlKey: true })
    expect(onSubmit).toHaveBeenCalledWith('test')
  })

  it('calls onCancel on Escape keydown', () => {
    render(<CommentInput lineNumber={1} onSubmit={onSubmit} onCancel={onCancel} />)
    const textarea = screen.getByPlaceholderText(/Add your comment/)
    fireEvent.keyDown(textarea, { key: 'Escape' })
    expect(onCancel).toHaveBeenCalledTimes(1)
  })

  it('calls onCancel when Cancel button is clicked', () => {
    render(<CommentInput lineNumber={1} onSubmit={onSubmit} onCancel={onCancel} />)
    fireEvent.click(screen.getByText('Cancel'))
    expect(onCancel).toHaveBeenCalledTimes(1)
  })
})
