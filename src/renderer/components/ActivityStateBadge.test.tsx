// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, fireEvent } from '@testing-library/react'
import { createStore } from 'zustand/vanilla'
import { ActivityStateBadge } from './ActivityStateBadge'
import { useContextMenuStore } from '../store/contextMenu'
import { ActivityState } from '../types'

// The context-menu store pulls in the app store (→ monaco) transitively; mock it so
// this lightweight suite stays free of the editor stack.
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

beforeEach(() => { useContextMenuStore.setState({ activeMenuId: null }) })

function makeWorkspaceStore() {
  return createStore<Record<string, unknown>>()(() => ({ addTab: vi.fn() }))
}

function renderBadge(workspace: ReturnType<typeof makeWorkspaceStore>, props: Partial<React.ComponentProps<typeof ActivityStateBadge>> = {}) {
  return render(
    <ActivityStateBadge
      workspace={workspace as never}
      tabId="tab1"
      state={ActivityState.Idle}
      getBufferText={() => 'screen text'}
      {...props}
    />
  )
}

describe('ActivityStateBadge', () => {
  it('names the state and shows the spinner only while analyzing', () => {
    const { container, rerender } = renderBadge(makeWorkspaceStore(), { state: ActivityState.PermissionRequest, title: 'wants to rm' })
    const badge = container.querySelector('.activity-state-badge')!
    expect(badge.textContent).toBe('permission request')
    expect(badge.getAttribute('title')).toBe('wants to rm')
    expect(badge.querySelector('.activity-state-badge-spinner')).toBeNull()

    rerender(
      <ActivityStateBadge workspace={makeWorkspaceStore() as never} tabId="tab1" state={ActivityState.Working} analyzing getBufferText={() => ''} />
    )
    expect(container.querySelector('.activity-state-badge-spinner')).not.toBeNull()
    expect(container.querySelector('.context-menu')).toBeNull()
  })

  it('distinguishes an application error from a classifier error', () => {
    const { container, rerender } = renderBadge(makeWorkspaceStore(), { state: ActivityState.ApplicationError, title: 'API overloaded' })
    const badge = container.querySelector('.activity-state-badge')!
    expect(badge.textContent).toBe('application error')
    const applicationErrorColor = (badge as HTMLElement).style.background

    rerender(
      <ActivityStateBadge workspace={makeWorkspaceStore() as never} tabId="tab1" state={ActivityState.Error} getBufferText={() => ''} />
    )
    expect(badge.textContent).toBe('error')
    expect((badge as HTMLElement).style.background).not.toBe(applicationErrorColor)
  })

  it('opens the menu on right-click and seeds the debugger with the tab buffer', () => {
    const workspace = makeWorkspaceStore()
    const { container, getByText } = renderBadge(workspace)

    fireEvent.contextMenu(container.querySelector('.activity-state-badge')!, { clientX: 10, clientY: 20 })
    expect(useContextMenuStore.getState().activeMenuId).toBe('activity-badge-tab1')
    expect(useContextMenuStore.getState().position).toEqual({ x: 10, y: 20 })

    fireEvent.click(getByText('Debug System Prompt'))
    expect(workspace.getState().addTab).toHaveBeenCalledWith('system-prompt-debugger', { bufferText: 'screen text' })
    expect(useContextMenuStore.getState().activeMenuId).toBeNull()
    expect(container.querySelector('.context-menu')).toBeNull()
  })

  it('opens the history tab for this tab from the menu', () => {
    const workspace = makeWorkspaceStore()
    const { container, getByText } = renderBadge(workspace)

    fireEvent.contextMenu(container.querySelector('.activity-state-badge')!)
    fireEvent.click(getByText('History'))

    expect(workspace.getState().addTab).toHaveBeenCalledWith('analyzer-history', { sourceTabId: 'tab1' })
    expect(useContextMenuStore.getState().activeMenuId).toBeNull()
  })
})
