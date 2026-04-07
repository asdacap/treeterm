// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'

import CloseConfirmDialog from './CloseConfirmDialog'
import type { Workspace } from '../../shared/types'

function makeWorkspace(overrides: Partial<Workspace> = {}): Workspace {
  return {
    id: 'ws-1',
    path: '/test',
    name: 'feature-branch',
    parentId: null,
    status: 'active',
    isGitRepo: true,
    gitBranch: 'feat/x',
    gitRootPath: '/test',
    isWorktree: true,
    isDetached: false,
    appStates: {},
    activeTabId: null,
    settings: { defaultApplicationId: '' },
    metadata: {},
    createdAt: Date.now(),
    lastActivity: Date.now(),
    ...overrides,
  }
}

describe('CloseConfirmDialog', () => {
  const onConfirm = vi.fn()
  const onCancel = vi.fn()

  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('renders "Unmerged Workspaces" heading', () => {
    render(
      <CloseConfirmDialog unmergedWorkspaces={[]} onConfirm={onConfirm} onCancel={onCancel} />
    )
    expect(screen.getByText('Unmerged Workspaces')).toBeDefined()
  })

  it('renders each workspace name in the list', () => {
    const workspaces = [
      makeWorkspace({ id: '1', name: 'ws-alpha' }),
      makeWorkspace({ id: '2', name: 'ws-beta' }),
    ]
    render(
      <CloseConfirmDialog unmergedWorkspaces={workspaces} onConfirm={onConfirm} onCancel={onCancel} />
    )
    expect(screen.getByText('ws-alpha')).toBeDefined()
    expect(screen.getByText('ws-beta')).toBeDefined()
  })

  it('renders git branch in parentheses', () => {
    render(
      <CloseConfirmDialog
        unmergedWorkspaces={[makeWorkspace({ gitBranch: 'feat/login' })]}
        onConfirm={onConfirm}
        onCancel={onCancel}
      />
    )
    expect(screen.getByText('(feat/login)')).toBeDefined()
  })

  it('does not render branch when gitBranch is null', () => {
    render(
      <CloseConfirmDialog
        unmergedWorkspaces={[makeWorkspace({ gitBranch: null })]}
        onConfirm={onConfirm}
        onCancel={onCancel}
      />
    )
    expect(screen.queryByText(/\(/)).toBeNull()
  })

  it('calls onConfirm when "Close Anyway" is clicked', () => {
    render(
      <CloseConfirmDialog unmergedWorkspaces={[]} onConfirm={onConfirm} onCancel={onCancel} />
    )
    fireEvent.click(screen.getByText('Close Anyway'))
    expect(onConfirm).toHaveBeenCalledTimes(1)
  })

  it('calls onCancel when "Cancel" button is clicked', () => {
    render(
      <CloseConfirmDialog unmergedWorkspaces={[]} onConfirm={onConfirm} onCancel={onCancel} />
    )
    fireEvent.click(screen.getByText('Cancel'))
    expect(onCancel).toHaveBeenCalledTimes(1)
  })

  it('calls onCancel when dialog close (x) button is clicked', () => {
    render(
      <CloseConfirmDialog unmergedWorkspaces={[]} onConfirm={onConfirm} onCancel={onCancel} />
    )
    fireEvent.click(screen.getByText('x'))
    expect(onCancel).toHaveBeenCalledTimes(1)
  })

  it('calls onCancel when overlay is clicked', () => {
    const { container } = render(
      <CloseConfirmDialog unmergedWorkspaces={[]} onConfirm={onConfirm} onCancel={onCancel} />
    )
    const overlay = container.querySelector('.dialog-overlay')
    expect(overlay).toBeDefined()
    if (overlay) fireEvent.click(overlay)
    expect(onCancel).toHaveBeenCalledTimes(1)
  })

  it('does not call onCancel when inner dialog is clicked', () => {
    const { container } = render(
      <CloseConfirmDialog unmergedWorkspaces={[]} onConfirm={onConfirm} onCancel={onCancel} />
    )
    const dialog = container.querySelector('.close-confirm-dialog')
    expect(dialog).toBeDefined()
    if (dialog) fireEvent.click(dialog)
    expect(onCancel).not.toHaveBeenCalled()
  })

  it('calls onCancel on Escape key press', () => {
    const { container } = render(
      <CloseConfirmDialog unmergedWorkspaces={[]} onConfirm={onConfirm} onCancel={onCancel} />
    )
    const overlayEl = container.querySelector('.dialog-overlay')
    expect(overlayEl).toBeDefined()
    if (overlayEl) fireEvent.keyDown(overlayEl, { key: 'Escape' })
    expect(onCancel).toHaveBeenCalledTimes(1)
  })
})
