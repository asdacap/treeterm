import { describe, it, expect } from 'vitest'
import { findRunningHarness } from './findRunningHarnessPtyId'
import type { Tab } from '../types'

function makeTab(overrides: Partial<Tab> = {}): Tab {
  return {
    id: 'tab-1',
    applicationId: 'terminal',
    title: 'Terminal',
    state: {},
    ...overrides
  }
}

function makeHarnessTab(ptyId: string | null, id: string = 'harness-tab'): Tab {
  return makeTab({
    id,
    applicationId: 'aiharness-claude',
    title: 'Claude',
    state: { ptyId, sandbox: { enabled: false, allowNetwork: false, allowedPaths: [] } }
  })
}

describe('findRunningHarness', () => {
  it('returns null for empty tabs', () => {
    expect(findRunningHarness([])).toBeNull()
  })

  it('returns null when no harness tabs exist', () => {
    const tabs = [
      makeTab(),
      makeTab({ applicationId: 'filesystem', state: { selectedPath: null, expandedDirs: [] } })
    ]
    expect(findRunningHarness(tabs)).toBeNull()
  })

  it('returns null when harness has ptyId null', () => {
    const tabs = [makeHarnessTab(null)]
    expect(findRunningHarness(tabs)).toBeNull()
  })

  it('returns ptyId and tabId for running harness', () => {
    const tabs = [makeTab(), makeHarnessTab('pty-123', 'my-harness-tab')]
    expect(findRunningHarness(tabs)).toEqual({ ptyId: 'pty-123', ptyHandle: null, tabId: 'my-harness-tab' })
  })

  it('returns first when multiple running harnesses exist', () => {
    const tabs = [makeHarnessTab('pty-first', 'tab-first'), makeHarnessTab('pty-second', 'tab-second')]
    expect(findRunningHarness(tabs)).toEqual({ ptyId: 'pty-first', ptyHandle: null, tabId: 'tab-first' })
  })
})
