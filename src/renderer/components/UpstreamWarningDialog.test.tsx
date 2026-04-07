// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import UpstreamWarningDialog from './UpstreamWarningDialog'

describe('UpstreamWarningDialog', () => {
  const onConfirm = vi.fn()
  const onCancel = vi.fn()

  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('renders "Upstream Updates Available" heading', () => {
    render(
      <UpstreamWarningDialog behindCount={3} workspaceName="main" onConfirm={onConfirm} onCancel={onCancel} />
    )
    expect(screen.getByText('Upstream Updates Available')).toBeDefined()
  })

  it('displays workspace name in bold', () => {
    const { container } = render(
      <UpstreamWarningDialog behindCount={1} workspaceName="my-branch" onConfirm={onConfirm} onCancel={onCancel} />
    )
    const strong = container.querySelector('strong')
    expect(strong).toBeDefined()
    expect(strong?.textContent).toBe('my-branch')
  })

  it('shows singular "commit" when behindCount is 1', () => {
    render(
      <UpstreamWarningDialog behindCount={1} workspaceName="ws" onConfirm={onConfirm} onCancel={onCancel} />
    )
    expect(screen.getByText(/1 commit behind/)).toBeDefined()
  })

  it('shows plural "commits" when behindCount > 1', () => {
    render(
      <UpstreamWarningDialog behindCount={5} workspaceName="ws" onConfirm={onConfirm} onCancel={onCancel} />
    )
    expect(screen.getByText(/5 commits behind/)).toBeDefined()
  })

  it('calls onConfirm when "Fork Anyway" is clicked', () => {
    render(
      <UpstreamWarningDialog behindCount={1} workspaceName="ws" onConfirm={onConfirm} onCancel={onCancel} />
    )
    fireEvent.click(screen.getByText('Fork Anyway'))
    expect(onConfirm).toHaveBeenCalledTimes(1)
  })

  it('calls onCancel when "Cancel" is clicked', () => {
    render(
      <UpstreamWarningDialog behindCount={1} workspaceName="ws" onConfirm={onConfirm} onCancel={onCancel} />
    )
    fireEvent.click(screen.getByText('Cancel'))
    expect(onCancel).toHaveBeenCalledTimes(1)
  })

  it('calls onCancel when dialog close (x) button is clicked', () => {
    render(
      <UpstreamWarningDialog behindCount={1} workspaceName="ws" onConfirm={onConfirm} onCancel={onCancel} />
    )
    fireEvent.click(screen.getByText('x'))
    expect(onCancel).toHaveBeenCalledTimes(1)
  })

  it('calls onCancel when overlay is clicked', () => {
    const { container } = render(
      <UpstreamWarningDialog behindCount={1} workspaceName="ws" onConfirm={onConfirm} onCancel={onCancel} />
    )
    const overlay = container.querySelector('.dialog-overlay')
    expect(overlay).toBeDefined()
    if (overlay) fireEvent.click(overlay)
    expect(onCancel).toHaveBeenCalledTimes(1)
  })

  it('does not call onCancel when inner dialog is clicked', () => {
    const { container } = render(
      <UpstreamWarningDialog behindCount={1} workspaceName="ws" onConfirm={onConfirm} onCancel={onCancel} />
    )
    const dialog = container.querySelector('.upstream-warning-dialog')
    expect(dialog).toBeDefined()
    if (dialog) fireEvent.click(dialog)
    expect(onCancel).not.toHaveBeenCalled()
  })

  it('calls onCancel on Escape key press', () => {
    const { container } = render(
      <UpstreamWarningDialog behindCount={1} workspaceName="ws" onConfirm={onConfirm} onCancel={onCancel} />
    )
    const overlayEl = container.querySelector('.dialog-overlay')
    expect(overlayEl).toBeDefined()
    if (overlayEl) fireEvent.keyDown(overlayEl, { key: 'Escape' })
    expect(onCancel).toHaveBeenCalledTimes(1)
  })
})
