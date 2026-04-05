// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import React from 'react'
import { ErrorBoundary } from './ErrorBoundary'
import type { FallbackProps } from './ErrorBoundary'

function Bomb({ shouldThrow }: { shouldThrow: boolean }) {
  if (shouldThrow) throw new Error('boom')
  return <div>OK</div>
}

describe('ErrorBoundary', () => {
  let consoleErrorSpy: ReturnType<typeof vi.spyOn>

  beforeEach(() => {
    vi.clearAllMocks()
    consoleErrorSpy = vi.spyOn(console, 'error').mockImplementation(() => {})
  })

  afterEach(() => {
    consoleErrorSpy.mockRestore()
  })

  it('renders children when no error is thrown', () => {
    render(
      <ErrorBoundary>
        <div>child content</div>
      </ErrorBoundary>
    )
    expect(screen.getByText('child content')).toBeDefined()
  })

  it('renders default "Something went wrong" when error thrown and no fallback', () => {
    render(
      <ErrorBoundary>
        <Bomb shouldThrow={true} />
      </ErrorBoundary>
    )
    expect(screen.getByText('Something went wrong')).toBeDefined()
  })

  it('renders fallback ReactNode when provided', () => {
    render(
      <ErrorBoundary fallback={<div>custom fallback</div>}>
        <Bomb shouldThrow={true} />
      </ErrorBoundary>
    )
    expect(screen.getByText('custom fallback')).toBeDefined()
  })

  it('renders FallbackComponent with error and reset props', () => {
    function TestFallback({ error, reset }: FallbackProps) {
      return (
        <div>
          <span>Error: {error.message}</span>
          <button onClick={reset}>Reset</button>
        </div>
      )
    }

    render(
      <ErrorBoundary FallbackComponent={TestFallback}>
        <Bomb shouldThrow={true} />
      </ErrorBoundary>
    )
    expect(screen.getByText('Error: boom')).toBeDefined()
    expect(screen.getByText('Reset')).toBeDefined()
  })

  it('passes fallbackProps to FallbackComponent', () => {
    interface ExtraProps extends FallbackProps {
      extraInfo: string
    }

    function TestFallback({ error, extraInfo }: ExtraProps) {
      return <div>{extraInfo}: {error.message}</div>
    }

    render(
      <ErrorBoundary<ExtraProps>
        FallbackComponent={TestFallback}
        fallbackProps={{ extraInfo: 'Details' }}
      >
        <Bomb shouldThrow={true} />
      </ErrorBoundary>
    )
    expect(screen.getByText('Details: boom')).toBeDefined()
  })

  it('calls onError callback with error and errorInfo', () => {
    const onError = vi.fn()
    render(
      <ErrorBoundary onError={onError}>
        <Bomb shouldThrow={true} />
      </ErrorBoundary>
    )
    expect(onError).toHaveBeenCalledTimes(1)
    expect(onError.mock.calls[0][0]).toBeInstanceOf(Error)
    expect(onError.mock.calls[0][0].message).toBe('boom')
  })

  it('recovers to render children after reset is called', () => {
    function TestFallback({ reset }: FallbackProps) {
      return <button onClick={reset}>Recover</button>
    }

    let shouldThrow = true
    function MaybeBomb() {
      if (shouldThrow) throw new Error('boom')
      return <div>recovered</div>
    }

    const { rerender } = render(
      <ErrorBoundary FallbackComponent={TestFallback}>
        <MaybeBomb />
      </ErrorBoundary>
    )

    expect(screen.getByText('Recover')).toBeDefined()

    shouldThrow = false
    fireEvent.click(screen.getByText('Recover'))

    rerender(
      <ErrorBoundary FallbackComponent={TestFallback}>
        <MaybeBomb />
      </ErrorBoundary>
    )

    expect(screen.getByText('recovered')).toBeDefined()
  })
})
