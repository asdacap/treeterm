import { describe, it, expect } from 'vitest'
import { createStore } from 'zustand/vanilla'
import { createPageMemory } from './runtime'
import type { Workspace } from '../../renderer/types'
import type { WorkspaceStore, WorkspaceStoreState } from '../../renderer/store/createWorkspaceStore'

const TAB_ID = 'tab-1'

function makeStore(state: unknown): WorkspaceStore {
  return createStore<WorkspaceStoreState>()((set, get) => ({
    workspace: { id: 'ws', path: '/p', appStates: { [TAB_ID]: { applicationId: 'browser-x', title: 'X', state } } } as unknown as Workspace,
    updateTabState: <T,>(tabId: string, updater: (s: T) => T) => {
      const ws = get().workspace
      const entry = ws.appStates[tabId]
      if (!entry) return
      set({ workspace: { ...ws, appStates: { ...ws.appStates, [tabId]: { ...entry, state: updater(entry.state as T) } } } })
    },
  }) as unknown as WorkspaceStoreState)
}

const path = (store: WorkspaceStore): unknown => (store.getState().workspace.appStates[TAB_ID]!.state as { path: unknown }).path

describe('createPageMemory', () => {
  it('opens the start page when nothing is remembered, including tabs saved before the field existed', () => {
    expect(createPageMemory(makeStore({ path: '' }), TAB_ID).pageUrl('http://localhost:3000/home?a=1')).toBe('http://localhost:3000/home?a=1')
    expect(createPageMemory(makeStore({}), TAB_ID).pageUrl('http://localhost:3000/home')).toBe('http://localhost:3000/home')
  })

  it('remembers the path, query and hash, and rebases it onto a new origin', () => {
    const store = makeStore({ path: '', localPort: { status: 'unassigned' } })
    const memory = createPageMemory(store, TAB_ID)
    memory.rememberPage('http://localhost:21000/', 'http://localhost:21000/d/x?from=now#panel')
    expect(path(store)).toBe('/d/x?from=now#panel')
    // Other fields are untouched.
    expect(store.getState().workspace.appStates[TAB_ID]!.state).toMatchObject({ localPort: { status: 'unassigned' } })
    // The forward's local port was reassigned.
    expect(memory.pageUrl('http://localhost:22000/')).toBe('http://localhost:22000/d/x?from=now#panel')
  })

  it('skips pages on another origin', () => {
    const store = makeStore({ path: '/kept' })
    createPageMemory(store, TAB_ID).rememberPage('http://localhost:3000/', 'http://localhost:4000/other')
    expect(path(store)).toBe('/kept')
  })

  it('does nothing when the tab is gone', () => {
    const store = makeStore({ path: '' })
    const memory = createPageMemory(store, 'missing')
    memory.rememberPage('http://localhost:3000/', 'http://localhost:3000/x')
    expect(memory.pageUrl('http://localhost:3000/')).toBe('http://localhost:3000/')
    expect(path(store)).toBe('')
  })
})
