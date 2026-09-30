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
    const { container } = render(<WorkspaceAttentionFlash tabIds={['t1']} unread={false} />)
    expect(container.querySelector('.tree-item-flash')).toBeNull()
    setState('t1', ActivityState.Working)
    expect(container.querySelector('.tree-item-flash')).toBeNull()
  })

  it('flashes with the state color and replays on each attention-state change', () => {
    const { container } = render(<WorkspaceAttentionFlash tabIds={['t1', 't2']} unread={false} />)
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

  it('does not flash when the unread marker was already on before the change', () => {
    const { container, rerender } = render(<WorkspaceAttentionFlash tabIds={['t1']} unread={true} />)
    setState('t1', ActivityState.Completed)
    expect(container.querySelector('.tree-item-flash')).toBeNull()
    setState('t1', ActivityState.PermissionRequest)
    expect(container.querySelector('.tree-item-flash')).toBeNull()

    // Once acknowledged, the next attention change flashes again.
    rerender(<WorkspaceAttentionFlash tabIds={['t1']} unread={false} />)
    setState('t1', ActivityState.Completed)
    expect(container.querySelector('.tree-item-flash.activity-completed')).not.toBeNull()
  })

  it('still flashes when the change itself marks the workspace unread', () => {
    const { container, rerender } = render(<WorkspaceAttentionFlash tabIds={['t1']} unread={false} />)
    setState('t1', ActivityState.Working)
    act(() => {
      useActivityStateStore.getState().setTabState('t1', ActivityState.Completed, { kind: ActivityTransitionKind.Debugger, snapshot: '' })
      rerender(<WorkspaceAttentionFlash tabIds={['t1']} unread={true} />)
    })
    expect(container.querySelector('.tree-item-flash.activity-completed')).not.toBeNull()
  })
})
