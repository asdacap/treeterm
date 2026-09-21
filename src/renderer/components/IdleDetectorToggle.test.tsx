// @vitest-environment jsdom
import { describe, it, expect, vi } from 'vitest'
import { render, fireEvent } from '@testing-library/react'
import { createStore } from 'zustand/vanilla'
import { IdleDetectorToggle } from './IdleDetectorToggle'
import { isIdleDetectorDisabled } from '../types'

function makeWorkspaceStore(state: unknown) {
  const updateTabState = vi.fn()
  const store = createStore<Record<string, unknown>>()(() => ({
    workspace: { id: 'ws1', appStates: { tab1: { applicationId: 'terminal', title: 'T', state } } },
    updateTabState,
  }))
  return { store, updateTabState }
}

function renderToggle(state: unknown) {
  const { store, updateTabState } = makeWorkspaceStore(state)
  const result = render(<IdleDetectorToggle workspace={store as never} tabId="tab1" />)
  const input = result.getByLabelText('Idle detector') as HTMLInputElement
  const lastUpdater = (): ((s: unknown) => unknown) => updateTabState.mock.calls.at(-1)?.[1] as (s: unknown) => unknown
  return { ...result, input, updateTabState, lastUpdater }
}

describe('isIdleDetectorDisabled', () => {
  it('treats a missing flag and non-terminal state as enabled', () => {
    expect(isIdleDetectorDisabled({ ptyId: 'pty1', keepOnExit: false })).toBe(false)
    expect(isIdleDetectorDisabled({ ptyId: 'pty1', idleDetectorDisabled: false })).toBe(false)
    expect(isIdleDetectorDisabled({})).toBe(false)
    expect(isIdleDetectorDisabled(undefined)).toBe(false)
  })

  it('is disabled only when the flag is set', () => {
    expect(isIdleDetectorDisabled({ ptyId: null, idleDetectorDisabled: true })).toBe(true)
  })
})

describe('IdleDetectorToggle', () => {
  it('is on by default, including for tabs persisted before the flag existed', () => {
    const { input } = renderToggle({ ptyId: 'pty1', keepOnExit: false })
    expect(input.checked).toBe(true)
  })

  it('reflects a disabled tab', () => {
    const { input } = renderToggle({ ptyId: 'pty1', keepOnExit: false, idleDetectorDisabled: true })
    expect(input.checked).toBe(false)
  })

  it('turns the detector off through updateTabState', () => {
    const { input, updateTabState, lastUpdater } = renderToggle({ ptyId: 'pty1', keepOnExit: false, idleDetectorDisabled: false })
    fireEvent.click(input)
    expect(updateTabState).toHaveBeenCalledWith('tab1', expect.any(Function))
    expect(lastUpdater()({ ptyId: 'pty1', keepOnExit: false, idleDetectorDisabled: false }))
      .toEqual({ ptyId: 'pty1', keepOnExit: false, idleDetectorDisabled: true })
  })

  it('turns the detector back on through updateTabState', () => {
    const { input, lastUpdater } = renderToggle({ ptyId: 'pty1', keepOnExit: false, idleDetectorDisabled: true })
    fireEvent.click(input)
    expect(lastUpdater()({ ptyId: 'pty1', keepOnExit: false, idleDetectorDisabled: true }))
      .toEqual({ ptyId: 'pty1', keepOnExit: false, idleDetectorDisabled: false })
  })
})
