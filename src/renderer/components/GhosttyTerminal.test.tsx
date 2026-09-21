// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render } from '@testing-library/react'
import { createStore } from 'zustand/vanilla'
import GhosttyTerminal from './GhosttyTerminal'
import type { BaseTerminalConfig } from './BaseTerminal'

// GhosttyTerminal is a thin selector: it picks the ghostty engine and hands everything else
// to BaseTerminal. Its behaviour lives in BaseTerminal.test.tsx and ghosttyEngine.test.ts.
const { configs } = vi.hoisted(() => ({ configs: [] as BaseTerminalConfig[] }))
vi.mock('./BaseTerminal', () => ({
  default: ({ config }: { config: BaseTerminalConfig }) => {
    configs.push(config)
    return <div data-testid="base-terminal" />
  },
}))
vi.mock('../terminal/ghosttyEngine', () => ({ createGhosttyEngine: vi.fn() }))
// The badge's context menu pulls in the context-menu store (→ app store → monaco); mock it
// so this lightweight suite stays free of the editor stack.
// The real settings store drags in store/app (and monaco), which jsdom cannot load.
vi.mock('../store/settings', () => {
  const settings = { terminal: { maxCols: 160 } }
  const useSettingsStore = <T,>(selector: (s: { settings: unknown }) => T): T => selector({ settings })
  useSettingsStore.getState = (): { settings: unknown } => ({ settings })
  return { useSettingsStore }
})
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

beforeEach(() => { configs.length = 0 })

function makeWorkspaceStore(tabId: string, ptyId: string | null) {
  return createStore<Record<string, unknown>>()(() => ({
    workspace: {
      id: 'ws1',
      activeTabId: tabId,
      appStates: { [tabId]: { applicationId: 'ghostty-terminal', title: 'Ghostty', state: { ptyId } } },
    },
    getTabRef: () => null,
  }))
}

describe('GhosttyTerminal', () => {
  it('waits for the PTY rather than booting a terminal with nothing behind it', () => {
    const workspace = makeWorkspaceStore('tab1', null)

    const { container, queryByTestId } = render(<GhosttyTerminal workspace={workspace as never} tabId="tab1" />)

    expect(container.textContent).toContain('Creating terminal...')
    expect(queryByTestId('base-terminal')).toBeNull()
  })

  it('drives BaseTerminal with the ghostty engine', async () => {
    const { createGhosttyEngine } = await import('../terminal/ghosttyEngine')
    const workspace = makeWorkspaceStore('tab1', 'pty1')

    const { getByTestId } = render(<GhosttyTerminal workspace={workspace as never} tabId="tab1" />)

    expect(getByTestId('base-terminal')).toBeTruthy()
    expect(configs[0]?.createEngine).toBe(createGhosttyEngine)
    expect(configs[0]?.logPrefix).toBe('Ghostty')
  })
})
