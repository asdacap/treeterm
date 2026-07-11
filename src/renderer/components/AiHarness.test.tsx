// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render } from '@testing-library/react'
import { createStore } from 'zustand/vanilla'
import AiHarness from './AiHarness'
import type { BaseTerminalConfig } from './BaseTerminal'
import type { TerminalEngine } from '../terminal/engine'
import { ActivityState } from '../types'

// AiHarness is a selector plus a status bar: it picks the xterm engine, taps keystrokes
// into the analyzer, and hands everything else to BaseTerminal. Terminal behaviour lives in
// BaseTerminal.test.tsx and xtermEngine.test.ts.
const { configs } = vi.hoisted(() => ({ configs: [] as BaseTerminalConfig[] }))
vi.mock('./BaseTerminal', () => ({
  default: ({ config }: { config: BaseTerminalConfig }) => {
    configs.push(config)
    return <div data-testid="base-terminal" />
  },
}))
vi.mock('../terminal/xtermEngine', () => ({ createXtermEngine: vi.fn() }))
// The context-menu store pulls in the app store (→ monaco) transitively; mock it so
// this lightweight suite stays free of the editor stack. Same for the prompt buttons.
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
vi.mock('./PromptCommitButton', () => ({ PromptCommitButton: () => <div /> }))
vi.mock('./PromptRebaseButton', () => ({ PromptRebaseButton: () => <div /> }))
vi.mock('./ReviewCommentsButton', () => ({ ReviewCommentsButton: () => <div /> }))
vi.mock('./PromptGitHubCommentsButton', () => ({ PromptGitHubCommentsButton: () => <div /> }))

beforeEach(() => { configs.length = 0 })

function makeAnalyzer() {
  return createStore(() => ({
    aiState: ActivityState.Idle,
    analyzing: false,
    reason: '',
    autoApprove: false,
    setAutoApprove: vi.fn(),
    onUserInput: vi.fn(),
  }))
}

function makeWorkspaceStore(tabId: string, state: unknown, analyzer: unknown) {
  return createStore<Record<string, unknown>>()(() => ({
    workspace: {
      id: 'ws1',
      activeTabId: tabId,
      appStates: state === undefined ? {} : { [tabId]: { applicationId: 'ai-harness', title: 'AI', state } },
    },
    getTabRef: () => (analyzer ? { analyzer } : null),
  }))
}

function renderHarness(workspace: ReturnType<typeof makeWorkspaceStore>) {
  return render(
    <AiHarness
      cwd="/tmp"
      workspace={workspace as never}
      tabId="tab1"
      command="claude"
      backgroundColor="#1e1e1e"
    />
  )
}

describe('AiHarness', () => {
  it('shows a loading gate while the app state is absent', () => {
    const workspace = makeWorkspaceStore('tab1', undefined, makeAnalyzer())

    const { container, queryByTestId } = renderHarness(workspace)

    expect(container.textContent).toContain('Loading AI harness...')
    expect(queryByTestId('base-terminal')).toBeNull()
  })

  it('rejects a state that is not an AI harness state', () => {
    const workspace = makeWorkspaceStore('tab1', { ptyId: 'pty1' }, makeAnalyzer())

    const { container, queryByTestId } = renderHarness(workspace)

    expect(container.textContent).toContain('Error: Invalid AI harness state')
    expect(queryByTestId('base-terminal')).toBeNull()
  })

  it('waits for the PTY rather than booting a terminal with nothing behind it', () => {
    const workspace = makeWorkspaceStore('tab1', { ptyId: null, sandbox: {} }, makeAnalyzer())

    const { container, queryByTestId } = renderHarness(workspace)

    expect(container.textContent).toContain('Starting AI harness...')
    expect(queryByTestId('base-terminal')).toBeNull()
  })

  it('waits for the analyzer before mounting the terminal', () => {
    const workspace = makeWorkspaceStore('tab1', { ptyId: 'pty1', sandbox: {} }, null)

    const { container, queryByTestId } = renderHarness(workspace)

    expect(container.textContent).toContain('Starting AI harness...')
    expect(queryByTestId('base-terminal')).toBeNull()
  })

  it('drives BaseTerminal with the xterm engine', async () => {
    const { createXtermEngine } = await import('../terminal/xtermEngine')
    const workspace = makeWorkspaceStore('tab1', { ptyId: 'pty1', sandbox: {} }, makeAnalyzer())

    const { getByTestId } = renderHarness(workspace)

    expect(getByTestId('base-terminal')).toBeTruthy()
    expect(configs[0]?.createEngine).toBe(createXtermEngine)
    expect(configs[0]?.logPrefix).toBe('AiHarness')
    expect(configs[0]?.disableActivityDetector).toBe(true)
  })

  it('forwards keystrokes from the engine to the analyzer', () => {
    const analyzer = makeAnalyzer()
    const workspace = makeWorkspaceStore('tab1', { ptyId: 'pty1', sandbox: {} }, analyzer)
    renderHarness(workspace)

    const onData = vi.fn((handler: (data: string) => void) => ({ dispose: vi.fn(), handler }))
    const engine = { onData } as unknown as TerminalEngine
    configs[0]?.onTerminalReady?.(engine)
    onData.mock.calls[0]?.[0]('ls\r')

    expect(analyzer.getState().onUserInput).toHaveBeenCalledWith('ls\r')
  })
})
