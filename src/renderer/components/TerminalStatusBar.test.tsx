// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, fireEvent, act } from '@testing-library/react'
import { createStore } from 'zustand/vanilla'
import { TerminalStatusBar } from './TerminalStatusBar'
import { ActivityTransitionKind, useActivityStateStore } from '../store/activityState'
import { ActivityState } from '../types'
import type { CachedTerminal } from '../types'

// The badge's context menu pulls in the context-menu store (→ app store → monaco); mock it
// so this lightweight suite stays free of the editor stack.
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

beforeEach(() => { useActivityStateStore.setState({ states: {}, transitions: {} }) })

function makeWorkspaceStore(cachedTerminal: CachedTerminal | null) {
  return createStore<Record<string, unknown>>()(() => ({
    workspace: { id: 'ws1', activeTabId: 'tab1', appStates: { tab1: { applicationId: 'terminal', title: 'T', state: { ptyId: 'pty1' } } } },
    updateTabState: vi.fn(),
    addTab: vi.fn(),
    getTabRef: () => ({ cachedTerminal }),
  }))
}

function makeEngine(lines: string[]): CachedTerminal['engine'] {
  return {
    rows: lines.length,
    raw: { buffer: { active: { length: lines.length, getLine: (y: number) => ({ translateToString: () => lines[y] }) } } },
  } as unknown as CachedTerminal['engine']
}

describe('TerminalStatusBar', () => {
  it('tracks the tab activity state and exposes the idle detector switch', () => {
    const workspace = makeWorkspaceStore(null)
    const { container, getByLabelText } = render(<TerminalStatusBar workspace={workspace as never} tabId="tab1" />)

    const badge = container.querySelector('.terminal-status-bar .activity-state-badge')!
    expect(badge.textContent).toBe('idle')
    expect((getByLabelText('Idle detector') as HTMLInputElement).checked).toBe(true)

    act(() => {
      useActivityStateStore.getState().setTabState('tab1', ActivityState.Working, { kind: ActivityTransitionKind.ViewportChanged, snapshot: '$ ls' })
    })
    expect(badge.textContent).toBe('working')
  })

  it('seeds the debugger with the cached terminal viewport', () => {
    const workspace = makeWorkspaceStore({ engine: makeEngine(['$ make', 'ok']) } as CachedTerminal)
    const { container, getByText } = render(<TerminalStatusBar workspace={workspace as never} tabId="tab1" />)

    fireEvent.contextMenu(container.querySelector('.activity-state-badge')!)
    fireEvent.click(getByText('Debug System Prompt'))

    expect(workspace.getState().addTab).toHaveBeenCalledWith('system-prompt-debugger', { bufferText: '$ make\nok' })
  })

  it('seeds an empty buffer before the terminal is cached', () => {
    const workspace = makeWorkspaceStore(null)
    const { container, getByText } = render(<TerminalStatusBar workspace={workspace as never} tabId="tab1" />)

    fireEvent.contextMenu(container.querySelector('.activity-state-badge')!)
    fireEvent.click(getByText('Debug System Prompt'))

    expect(workspace.getState().addTab).toHaveBeenCalledWith('system-prompt-debugger', { bufferText: '' })
  })
})
