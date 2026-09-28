// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, waitFor, act } from '@testing-library/react'
import { createStore } from 'zustand/vanilla'
import { BranchBadge } from './BranchBadge'
import { useContextMenuStore } from '../store/contextMenu'
import type { WorkspaceStoreState } from '../store/createWorkspaceStore'
import { TitleRefreshStatus } from '../store/createAnalyzerStore'
import type { TitleRefreshResult } from '../store/createAnalyzerStore'
import type { ClipboardApi, WorkspaceStore } from '../types'

// The real store imports the app store, which drags Monaco into jsdom.
vi.mock('../store/contextMenu', async () => {
  const { create } = await import('zustand')
  const store = create<{
    activeMenuId: string | null
    position: { x: number; y: number }
    open: (menuId: string, x: number, y: number) => void
    close: () => void
  }>()((set) => ({
    activeMenuId: null,
    position: { x: 0, y: 0 },
    open: (menuId, x, y) => { set({ activeMenuId: menuId, position: { x, y } }); },
    close: () => { set({ activeMenuId: null }); },
  }))
  return { useContextMenuStore: store }
})

function makeWorkspaceStore(refreshBranchName: () => Promise<TitleRefreshResult>) {
  return createStore<WorkspaceStoreState>()(() => ({
    refreshBranchName,
  }) as unknown as WorkspaceStoreState) as unknown as WorkspaceStore
}

function makeClipboard(): ClipboardApi & { writeText: ReturnType<typeof vi.fn> } {
  return { writeText: vi.fn(), readText: vi.fn() } as unknown as ClipboardApi & { writeText: ReturnType<typeof vi.fn> }
}

function renderBadge(opts: { refresh?: () => Promise<TitleRefreshResult>; canAutoRename?: boolean; clipboard?: ClipboardApi } = {}) {
  const refresh = opts.refresh ?? vi.fn().mockResolvedValue({ status: TitleRefreshStatus.Success })
  const clipboard = opts.clipboard ?? makeClipboard()
  render(
    <BranchBadge
      branch="feat/old-name"
      worktreePath="/repo/.worktrees/old"
      workspace={makeWorkspaceStore(refresh)}
      canAutoRename={opts.canAutoRename ?? true}
      clipboard={clipboard}
    />
  )
  return { refresh, clipboard }
}

const openMenu = () => { fireEvent.contextMenu(screen.getByText('feat/old-name')) }

describe('BranchBadge', () => {
  const alertMock = vi.fn<(message?: string) => void>()

  beforeEach(() => {
    alertMock.mockClear()
    vi.stubGlobal('alert', alertMock)
    act(() => { useContextMenuStore.getState().close() })
  })

  afterEach(() => {
    vi.unstubAllGlobals()
    vi.useRealTimers()
  })

  it('copies the branch name on click and briefly shows Copied!', () => {
    vi.useFakeTimers()
    const { clipboard } = renderBadge()

    fireEvent.click(screen.getByText('feat/old-name'))

    expect(clipboard.writeText).toHaveBeenCalledWith('feat/old-name')
    expect(screen.getByText('Copied!')).toBeTruthy()
    act(() => { vi.advanceTimersByTime(1500) })
    expect(screen.getByText('feat/old-name')).toBeTruthy()
  })

  it('copies the worktree path from the context menu', () => {
    const { clipboard } = renderBadge()
    openMenu()

    fireEvent.click(screen.getByText('Copy worktree path'))

    expect(clipboard.writeText).toHaveBeenCalledWith('/repo/.worktrees/old')
    expect(screen.queryByText('Copy worktree path')).toBeNull()
  })

  it('hides Auto rename branch when the workspace cannot be renamed', () => {
    renderBadge({ canAutoRename: false })
    openMenu()

    expect(screen.getByText('Copy worktree path')).toBeTruthy()
    expect(screen.queryByText('Auto rename branch')).toBeNull()
  })

  it('reapplies the LLM branch name, showing progress while in flight', async () => {
    let resolve: (result: TitleRefreshResult) => void = () => { /* replaced below */ }
    const refresh = vi.fn().mockReturnValue(new Promise<TitleRefreshResult>((r) => { resolve = r }))
    renderBadge({ refresh })
    openMenu()

    fireEvent.click(screen.getByText('Auto rename branch'))

    expect(refresh).toHaveBeenCalledOnce()
    await waitFor(() => { expect(screen.getByText(/Renaming/)).toBeTruthy() })
    // Menu closed and, while in flight, reopening it does not offer a duplicate rename.
    fireEvent.contextMenu(screen.getByText(/Renaming/))
    expect(screen.queryByText('Auto rename branch')).toBeNull()

    act(() => { resolve({ status: TitleRefreshStatus.Success }) })
    await waitFor(() => { expect(screen.queryByText(/Renaming/)).toBeNull() })
    expect(alertMock).not.toHaveBeenCalled()
  })

  it('alerts the error when the rename fails', async () => {
    const refresh = vi.fn().mockResolvedValue({ status: TitleRefreshStatus.Failure, error: 'no model configured' })
    renderBadge({ refresh })
    openMenu()

    fireEvent.click(screen.getByText('Auto rename branch'))

    await waitFor(() => { expect(alertMock).toHaveBeenCalledWith('no model configured') })
  })

  it('alerts when the rename throws', async () => {
    const consoleSpy = vi.spyOn(console, 'error').mockImplementation(() => {})
    renderBadge({ refresh: vi.fn().mockRejectedValue(new Error('boom')) })
    openMenu()

    fireEvent.click(screen.getByText('Auto rename branch'))

    await waitFor(() => { expect(alertMock).toHaveBeenCalledWith('boom') })
    consoleSpy.mockRestore()
  })

  it('stringifies non-Error throws', async () => {
    const consoleSpy = vi.spyOn(console, 'error').mockImplementation(() => {})
    renderBadge({ refresh: vi.fn().mockRejectedValue('plain failure') })
    openMenu()

    fireEvent.click(screen.getByText('Auto rename branch'))

    await waitFor(() => { expect(alertMock).toHaveBeenCalledWith('plain failure') })
    consoleSpy.mockRestore()
  })
})
