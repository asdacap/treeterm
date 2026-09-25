// @vitest-environment jsdom
import { describe, it, expect, beforeEach, vi } from 'vitest'
import { render, act } from '@testing-library/react'
import { WorkspaceAttentionFlash } from './TreePane'
import { useActivityStateStore, ActivityTransitionKind } from '../store/activityState'
import { ActivityState } from '../types'

// The real app store pulls in monaco, which cannot load under jsdom.
vi.mock('../store/app', () => ({
  useAppStore: Object.assign(vi.fn(() => ({})), { getState: () => ({}) }),
}))

function setState(tabId: string, state: ActivityState): void {
  act(() => {
    useActivityStateStore.getState().setTabState(tabId, state, { kind: ActivityTransitionKind.Debugger, snapshot: '' })
  })
}

describe('WorkspaceAttentionFlash', () => {
  beforeEach(() => {
    useActivityStateStore.setState({ states: {}, transitions: {} })
  })

  it('renders nothing while idle or working', () => {
    const { container } = render(<WorkspaceAttentionFlash tabIds={['t1']} />)
    expect(container.querySelector('.tree-item-flash')).toBeNull()
    setState('t1', ActivityState.Working)
    expect(container.querySelector('.tree-item-flash')).toBeNull()
  })

  it('flashes with the state color and replays on each attention-state change', () => {
    const { container } = render(<WorkspaceAttentionFlash tabIds={['t1', 't2']} />)
    setState('t2', ActivityState.PermissionRequest)
    const first = container.querySelector('.tree-item-flash')
    expect(first?.classList.contains('activity-permission_request')).toBe(true)

    setState('t2', ActivityState.Completed)
    const second = container.querySelector('.tree-item-flash')
    expect(second?.classList.contains('activity-completed')).toBe(true)
    expect(second).not.toBe(first)

    setState('t2', ActivityState.Idle)
    expect(container.querySelector('.tree-item-flash')).toBeNull()
  })
})
