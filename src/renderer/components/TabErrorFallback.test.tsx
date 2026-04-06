// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import TabErrorFallback from './TabErrorFallback'

describe('TabErrorFallback', () => {
  const reset = vi.fn()
  const onClose = vi.fn()

  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('renders "Tab Error" heading', () => {
    render(
      <TabErrorFallback error={new Error('oops')} reset={reset} tabTitle="My Tab" onClose={onClose} />
    )
    expect(screen.getByText('Tab Error')).toBeDefined()
  })

  it('displays the tab title in the error description', () => {
    render(
      <TabErrorFallback error={new Error('oops')} reset={reset} tabTitle="My Tab" onClose={onClose} />
    )
    expect(screen.getByText(/My Tab/)).toBeDefined()
  })

  it('displays the error message', () => {
    render(
      <TabErrorFallback error={new Error('specific error')} reset={reset} tabTitle="T" onClose={onClose} />
    )
    expect(screen.getByText('specific error')).toBeDefined()
  })

  it('calls reset when "Reload Tab" button is clicked', () => {
    render(
      <TabErrorFallback error={new Error('e')} reset={reset} tabTitle="T" onClose={onClose} />
    )
    fireEvent.click(screen.getByText('Reload Tab'))
    expect(reset).toHaveBeenCalledTimes(1)
  })

  it('calls onClose when "Close Tab" button is clicked', () => {
    render(
      <TabErrorFallback error={new Error('e')} reset={reset} tabTitle="T" onClose={onClose} />
    )
    fireEvent.click(screen.getByText('Close Tab'))
    expect(onClose).toHaveBeenCalledTimes(1)
  })
})
