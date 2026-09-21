// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, act } from '@testing-library/react'
import { createStore } from 'zustand/vanilla'
import Terminal from './Terminal'
import GhosttyTerminal from './GhosttyTerminal'
import type { BaseTerminalConfig } from './BaseTerminal'
import { ActivityTransitionKind, useActivityStateStore } from '../store/activityState'
import { ActivityState } from '../types'

// Terminal / GhosttyTerminal pick an engine and wrap BaseTerminal in the status bar.
// Terminal behaviour itself lives in BaseTerminal.test.tsx.
const { configs } = vi.hoisted(() => ({ configs: [] as BaseTerminalConfig[] }))
vi.mock('./BaseTerminal', () => ({
  default: ({ config }: { config: BaseTerminalConfig }) => {
    configs.push(config)
    return <div data-testid="base-terminal" />
  },
}))
vi.mock('../terminal/xtermEngine', () => ({ createXtermEngine: vi.fn() }))
vi.mock('../terminal/ghosttyEngine', () => ({ createGhosttyEngine: vi.fn() }))
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

beforeEach(() => {
  configs.length = 0
  useActivityStateStore.setState({ states: {}, transitions: {} })
})

function makeWorkspaceStore(state: unknown) {
  return createStore<Record<string, unknown>>()(() => ({
    workspace: { id: 'ws1', activeTabId: 'tab1', appStates: { tab1: { applicationId: 'terminal', title: 'T', state } } },
    updateTabState: vi.fn(),
    getTabRef: () => null,
  }))
}

describe('Terminal', () => {
  it('waits for the PTY without a status bar', () => {
    const workspace = makeWorkspaceStore({ ptyId: null, keepOnExit: false })
    const { container, queryByTestId } = render(<Terminal cwd="/tmp" workspace={workspace as never} tabId="tab1" />)
    expect(container.textContent).toContain('Creating terminal...')
    expect(queryByTestId('base-terminal')).toBeNull()
    expect(container.querySelector('.terminal-status-bar')).toBeNull()
  })

  it('renders the terminal under a status bar with the activity badge and idle detector switch', async () => {
    const { createXtermEngine } = await import('../terminal/xtermEngine')
    const workspace = makeWorkspaceStore({ ptyId: 'pty1', keepOnExit: false })
    const { container, getByTestId, getByLabelText } = render(<Terminal cwd="/tmp" workspace={workspace as never} tabId="tab1" />)

    expect(getByTestId('base-terminal')).toBeTruthy()
    expect(configs[0]?.createEngine).toBe(createXtermEngine)
    expect(configs[0]?.themeBackground).toBe('#1e1e1e')
    expect((getByLabelText('Idle detector') as HTMLInputElement).checked).toBe(true)
    const badge = container.querySelector('.terminal-status-bar .activity-state-badge')!
    expect(badge.textContent).toBe('idle')

    act(() => { useActivityStateStore.getState().setTabState('tab1', ActivityState.Working, { kind: ActivityTransitionKind.ViewportChanged, snapshot: '$ ls' }) })
    expect(badge.textContent).toBe('working')
  })

  it('uses the sandbox background when sandboxed', () => {
    const workspace = makeWorkspaceStore({ ptyId: 'pty1', keepOnExit: false })
    render(<Terminal cwd="/tmp" workspace={workspace as never} tabId="tab1" sandbox={{ enabled: true, allowNetwork: false, allowedPaths: [] }} />)
    expect(configs[0]?.themeBackground).toBe('#1a1a2e')
  })
})

describe('GhosttyTerminal', () => {
  it('waits for the PTY without a status bar', () => {
    const workspace = makeWorkspaceStore({ ptyId: null, keepOnExit: false })
    const { container } = render(<GhosttyTerminal workspace={workspace as never} tabId="tab1" />)
    expect(container.textContent).toContain('Creating terminal...')
    expect(container.querySelector('.terminal-status-bar')).toBeNull()
  })

  it('renders the ghostty engine under the same status bar', async () => {
    const { createGhosttyEngine } = await import('../terminal/ghosttyEngine')
    const workspace = makeWorkspaceStore({ ptyId: 'pty1', keepOnExit: false, idleDetectorDisabled: true })
    const { container, getByLabelText } = render(<GhosttyTerminal workspace={workspace as never} tabId="tab1" />)

    expect(configs[0]?.createEngine).toBe(createGhosttyEngine)
    expect((getByLabelText('Idle detector') as HTMLInputElement).checked).toBe(false)
    expect(container.querySelector('.terminal-status-bar .activity-state-badge')?.textContent).toBe('idle')
  })
})
