// @vitest-environment jsdom
import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'

import AppErrorFallback from './AppErrorFallback'

describe('AppErrorFallback', () => {
  const reloadMock = vi.fn()

  it('renders "Application Error" heading', () => {
    render(<AppErrorFallback onReload={reloadMock} />)
    expect(screen.getByText('Application Error')).toBeDefined()
  })

  it('renders error message when error is provided', () => {
    render(<AppErrorFallback error={new Error('Something broke')} onReload={reloadMock} />)
    expect(screen.getByText('Something broke')).toBeDefined()
  })

  it('renders without error details when error is undefined', () => {
    const { container } = render(<AppErrorFallback onReload={reloadMock} />)
    expect(container.querySelector('.app-error-details')).toBeNull()
  })

  it('shows "Show Stack Trace" button when error has stack', () => {
    const err = new Error('fail')
    err.stack = 'Error: fail\n    at test.ts:1'
    render(<AppErrorFallback error={err} onReload={reloadMock} />)
    expect(screen.getByText('Show Stack Trace')).toBeDefined()
  })

  it('does not show stack toggle when error has no stack', () => {
    const err = new Error('fail')
    err.stack = undefined
    render(<AppErrorFallback error={err} onReload={reloadMock} />)
    expect(screen.queryByText('Show Stack Trace')).toBeNull()
  })

  it('toggles stack trace visibility on button click', () => {
    const err = new Error('fail')
    err.stack = 'Error: fail\n    at test.ts:1'
    render(<AppErrorFallback error={err} onReload={reloadMock} />)

    // Initially hidden
    expect(screen.queryByText(/at test.ts:1/)).toBeNull()

    // Click to show
    fireEvent.click(screen.getByText('Show Stack Trace'))
    expect(screen.getByText(/at test.ts:1/)).toBeDefined()
    expect(screen.getByText('Hide Stack Trace')).toBeDefined()

    // Click to hide
    fireEvent.click(screen.getByText('Hide Stack Trace'))
    expect(screen.queryByText(/at test.ts:1/)).toBeNull()
  })

  it('calls onReload on "Reload Application" click', () => {
    render(<AppErrorFallback onReload={reloadMock} />)
    fireEvent.click(screen.getByText('Reload Application'))
    expect(reloadMock).toHaveBeenCalledTimes(1)
  })
})
