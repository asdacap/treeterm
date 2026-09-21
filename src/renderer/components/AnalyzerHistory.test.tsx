// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, fireEvent, act } from '@testing-library/react'
import { createStore } from 'zustand/vanilla'
import AnalyzerHistory from './AnalyzerHistory'
import { ActivityTransitionKind, useActivityStateStore, type ActivityTransitionDetail } from '../store/activityState'
import { ActivityState, type ApplicationRenderProps } from '../types'
import type { AnalyzerHistoryEntry } from '../store/createAnalyzerStore'

beforeEach(() => { useActivityStateStore.setState({ states: {}, transitions: {} }) })

function makeWorkspaceStore(ref: unknown) {
  return createStore<Record<string, unknown>>()(() => ({
    addTab: vi.fn(),
    getTabRef: () => ref,
  }))
}

function makeAnalyzer(history: AnalyzerHistoryEntry[]) {
  return createStore(() => ({ getHistory: () => history }))
}

function renderHistory(workspace: ReturnType<typeof makeWorkspaceStore>, state: unknown = { sourceTabId: 'src' }) {
  const props = { tab: { id: 'hist', state }, workspace, isVisible: true } as unknown as ApplicationRenderProps
  return render(<AnalyzerHistory {...props} />)
}

const record = (to: ActivityState, detail: ActivityTransitionDetail): void => {
  useActivityStateStore.getState().setTabState('src', to, detail)
}

describe('AnalyzerHistory', () => {
  it('rejects a tab without a source', () => {
    const { container } = renderHistory(makeWorkspaceStore(null), {})
    expect(container.textContent).toContain('Invalid state: missing sourceTabId')
  })

  it('shows an empty transition list for a plain terminal and no analyzer section', () => {
    const { container } = renderHistory(makeWorkspaceStore({ cachedTerminal: null }))
    expect(container.textContent).toContain('Activity Transitions')
    expect(container.textContent).toContain('0 entries')
    expect(container.textContent).toContain('No transitions yet.')
    expect(container.textContent).not.toContain('Analyzer History')
  })

  it('lists transitions newest first with each kind\'s label and extra', () => {
    record(ActivityState.Working, { kind: ActivityTransitionKind.ViewportChanged, snapshot: 'one' })
    record(ActivityState.Idle, { kind: ActivityTransitionKind.ViewportIdle, snapshot: 'two', idleTimeoutMs: 2000 })
    record(ActivityState.Idle, { kind: ActivityTransitionKind.Classification, snapshot: 'three', reason: 'prompt visible' })
    record(ActivityState.Error, { kind: ActivityTransitionKind.Debugger, snapshot: 'four' })

    const { container } = renderHistory(makeWorkspaceStore({ cachedTerminal: null }))

    expect(container.textContent).toContain('4 entries')
    const rows = Array.from(container.querySelectorAll('pre')).map((pre) => pre.parentElement!.textContent)
    expect(rows[0]).toContain('idle → error')
    expect(rows[0]).toContain('debugger')
    expect(rows[1]).toContain('idle → idle')
    expect(rows[1]).toContain('classification')
    expect(rows[1]).toContain('prompt visible')
    expect(rows[2]).toContain('working → idle')
    expect(rows[2]).toContain('viewport idle')
    expect(rows[2]).toContain('after 2000ms')
    expect(rows[3]).toContain('idle → working')
    expect(rows[3]).toContain('viewport changed')
  })

  it('follows the store live, expands a snapshot on click, and seeds the debugger from a row', () => {
    const workspace = makeWorkspaceStore({ cachedTerminal: null })
    const { container, getAllByText } = renderHistory(workspace)
    const long = ['a', 'b', 'c', 'd'].join('\n')

    act(() => { record(ActivityState.Working, { kind: ActivityTransitionKind.ViewportChanged, snapshot: long }) })
    const pre = container.querySelector('pre')!
    expect(pre.textContent).toBe('a\nb\nc\n...')

    fireEvent.click(pre)
    expect(pre.textContent).toBe(long)
    fireEvent.click(pre)
    expect(pre.textContent).toBe('a\nb\nc\n...')

    fireEvent.click(getAllByText('Debug')[0]!)
    expect(workspace.getState().addTab).toHaveBeenCalledWith('system-prompt-debugger', { bufferText: long })
  })

  it('shows the analyzer section under the transitions for an AI harness source', () => {
    const analyzer = makeAnalyzer([{
      timestamp: 1, kind: 'analyzer', model: 'jev', bufferText: 'buf', response: 'idle', durationMs: 12,
    }])
    const { container } = renderHistory(makeWorkspaceStore({ analyzer }))

    expect(container.textContent).toContain('Activity Transitions')
    expect(container.textContent).toContain('Analyzer History')
    expect(container.textContent).toContain('1 entries')
    expect(container.textContent).toContain('jev')
  })
})
