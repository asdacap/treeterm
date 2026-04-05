// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import React from 'react'
import WorkspaceErrorFallback from './WorkspaceErrorFallback'

describe('WorkspaceErrorFallback', () => {
  const reset = vi.fn()

  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('renders "Workspace Error" heading', () => {
    render(<WorkspaceErrorFallback error={new Error('fail')} reset={reset} />)
    expect(screen.getByText('Workspace Error')).toBeDefined()
  })

  it('displays the error message', () => {
    render(<WorkspaceErrorFallback error={new Error('workspace broke')} reset={reset} />)
    expect(screen.getByText('workspace broke')).toBeDefined()
  })

  it('calls reset when "Reload Workspace" button is clicked', () => {
    render(<WorkspaceErrorFallback error={new Error('e')} reset={reset} />)
    fireEvent.click(screen.getByText('Reload Workspace'))
    expect(reset).toHaveBeenCalledTimes(1)
  })
})
