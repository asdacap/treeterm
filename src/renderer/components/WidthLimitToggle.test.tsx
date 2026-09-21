// @vitest-environment jsdom
import { describe, it, expect, vi } from 'vitest'
import { render, fireEvent } from '@testing-library/react'
import { createStore } from 'zustand/vanilla'
import { WidthLimitToggle } from './WidthLimitToggle'
import { isWidthLimitDisabled } from '../types'

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
  const result = render(<WidthLimitToggle workspace={store as never} tabId="tab1" />)
  const input = result.getByLabelText('80 col limit') as HTMLInputElement
  const lastUpdater = (): ((s: unknown) => unknown) => updateTabState.mock.calls.at(-1)?.[1] as (s: unknown) => unknown
  return { ...result, input, updateTabState, lastUpdater }
}

describe('isWidthLimitDisabled', () => {
  it('treats a missing flag and non-terminal state as limited', () => {
    expect(isWidthLimitDisabled({ ptyId: 'pty1', keepOnExit: false })).toBe(false)
    expect(isWidthLimitDisabled({ ptyId: 'pty1', widthLimitDisabled: false })).toBe(false)
    expect(isWidthLimitDisabled({})).toBe(false)
    expect(isWidthLimitDisabled(undefined)).toBe(false)
  })

  it('is disabled only when the flag is set', () => {
    expect(isWidthLimitDisabled({ ptyId: null, widthLimitDisabled: true })).toBe(true)
  })
})

describe('WidthLimitToggle', () => {
  it('is on by default, including for tabs persisted before the flag existed', () => {
    const { input } = renderToggle({ ptyId: 'pty1', keepOnExit: false })
    expect(input.checked).toBe(true)
  })

  it('reflects an unlimited tab', () => {
    const { input } = renderToggle({ ptyId: 'pty1', keepOnExit: false, widthLimitDisabled: true })
    expect(input.checked).toBe(false)
  })

  it('lifts the limit through updateTabState', () => {
    const { input, updateTabState, lastUpdater } = renderToggle({ ptyId: 'pty1', keepOnExit: false, widthLimitDisabled: false })
    fireEvent.click(input)
    expect(updateTabState).toHaveBeenCalledWith('tab1', expect.any(Function))
    expect(lastUpdater()({ ptyId: 'pty1', keepOnExit: false, widthLimitDisabled: false }))
      .toEqual({ ptyId: 'pty1', keepOnExit: false, widthLimitDisabled: true })
  })

  it('restores the limit through updateTabState', () => {
    const { input, lastUpdater } = renderToggle({ ptyId: 'pty1', keepOnExit: false, widthLimitDisabled: true })
    fireEvent.click(input)
    expect(lastUpdater()({ ptyId: 'pty1', keepOnExit: false, widthLimitDisabled: true }))
      .toEqual({ ptyId: 'pty1', keepOnExit: false, widthLimitDisabled: false })
  })
})
