// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { createStore } from 'zustand/vanilla'
import { RefreshTitleButton } from './RefreshTitleButton'
import type { WorkspaceStoreState } from '../store/createWorkspaceStore'
import { TitleRefreshStatus } from '../store/createAnalyzerStore'
import type { TitleRefreshResult } from '../store/createAnalyzerStore'
import type { WorkspaceStore } from '../types'

function makeWorkspaceStore(refreshTitleAndDescription: () => Promise<TitleRefreshResult>) {
  return createStore<WorkspaceStoreState>()(() => ({
    refreshTitleAndDescription,
  }) as unknown as WorkspaceStoreState) as unknown as WorkspaceStore
}

const getButton = () => screen.getByRole('button')

describe('RefreshTitleButton', () => {
  const alertMock = vi.fn<(message?: string) => void>()

  beforeEach(() => {
    alertMock.mockClear()
    vi.stubGlobal('alert', alertMock)
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('triggers the LLM labeller on click and does not alert on success', async () => {
    const refresh = vi.fn().mockResolvedValue({ status: TitleRefreshStatus.Success })
    render(<RefreshTitleButton workspace={makeWorkspaceStore(refresh)} />)

    fireEvent.click(getButton())

    await waitFor(() => { expect(getButton()).not.toHaveProperty('disabled', true) })
    expect(refresh).toHaveBeenCalledOnce()
    expect(alertMock).not.toHaveBeenCalled()
  })

  it('disables itself while the labeller is in flight', async () => {
    let resolve: (result: TitleRefreshResult) => void = () => { /* replaced below */ }
    const refresh = vi.fn().mockReturnValue(new Promise<TitleRefreshResult>((r) => { resolve = r }))
    render(<RefreshTitleButton workspace={makeWorkspaceStore(refresh)} />)

    fireEvent.click(getButton())

    // In flight: disabled, so a second click cannot start a duplicate LLM call.
    await waitFor(() => { expect(getButton()).toHaveProperty('disabled', true) })
    expect(getButton().getAttribute('aria-label')).toContain('Generating')
    fireEvent.click(getButton())
    expect(refresh).toHaveBeenCalledOnce()

    resolve({ status: TitleRefreshStatus.Success })
    await waitFor(() => { expect(getButton()).toHaveProperty('disabled', false) })
  })

  it('alerts the error message when the labeller fails', async () => {
    const refresh = vi.fn().mockResolvedValue({ status: TitleRefreshStatus.Failure, error: 'no model configured' })
    render(<RefreshTitleButton workspace={makeWorkspaceStore(refresh)} />)

    fireEvent.click(getButton())

    await waitFor(() => { expect(alertMock).toHaveBeenCalledWith('no model configured') })
    // The button must recover so the user can retry after fixing the cause.
    expect(getButton()).toHaveProperty('disabled', false)
  })

  it('surfaces a thrown error and re-enables itself', async () => {
    const consoleMock = vi.spyOn(console, 'error').mockImplementation(() => { /* noop */ })
    const refresh = vi.fn().mockRejectedValue(new Error('boom'))
    render(<RefreshTitleButton workspace={makeWorkspaceStore(refresh)} />)

    fireEvent.click(getButton())

    await waitFor(() => { expect(alertMock).toHaveBeenCalledWith('boom') })
    expect(getButton()).toHaveProperty('disabled', false)
    consoleMock.mockRestore()
  })
})
