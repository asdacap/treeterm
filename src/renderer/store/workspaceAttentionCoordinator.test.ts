import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createStore, type StoreApi } from 'zustand/vanilla'
import { ConnectionStatus, ConnectionTargetType } from '../../shared/types'
import { makeWorkspace } from '../../shared/test-fixtures/workspace'
import { ActivityState, type Workspace } from '../types'
import { useActivityStateStore } from './activityState'
import { useNavigationStore } from './navigation'
import { WorkspaceEntryStatus, type SessionState, type WorkspaceEntry } from './createSessionStore'
import { createWorkspaceAttentionCoordinator } from './workspaceAttentionCoordinator'
import { WORKSPACE_ATTENTION_KEY } from './workspaceAttention'

function workspace(tabIds: string[] = ['tab-1'], metadata: Record<string, string> = {}): Workspace {
  return makeWorkspace({ appStates: Object.fromEntries(tabIds.map(id => [id, { applicationId: 'terminal', title: id, state: {} }])), metadata })
}

function entry(data: Workspace): WorkspaceEntry {
  return { status: WorkspaceEntryStatus.Loaded, attentionPending: false, data } as WorkspaceEntry
}

function attention(revision: string, acknowledgedRevision = ''): Record<string, string> {
  return { [WORKSPACE_ATTENTION_KEY]: JSON.stringify({ revision, acknowledgedRevision }) }
}

function setup(initialWorkspace = workspace()): {
  session: StoreApi<SessionState>
  record: ReturnType<typeof vi.fn>
  acknowledge: ReturnType<typeof vi.fn>
  playDing: ReturnType<typeof vi.fn>
  reportError: ReturnType<typeof vi.fn>
  soundEnabled: ReturnType<typeof vi.fn>
  dispose: () => void
  setSessions: (stores: StoreApi<SessionState>[]) => void
  updateWorkspace: (data: Workspace) => void
} {
  const record = vi.fn()
  const acknowledge = vi.fn()
  const session = createStore<SessionState>(() => ({
    sessionId: 'session-1',
    connection: { id: 'connection', status: ConnectionStatus.Connected, target: { type: ConnectionTargetType.Local } },
    isRestoring: false,
    workspaces: new Map([['ws-1', entry(initialWorkspace)]]),
    recordWorkspaceAttention: record,
    acknowledgeWorkspaceAttention: acknowledge,
  } as unknown as SessionState))
  let stores = [session]
  const listeners = new Set<() => void>()
  const playDing = vi.fn().mockResolvedValue(undefined)
  const reportError = vi.fn()
  const soundEnabled = vi.fn(() => true)
  const dispose = createWorkspaceAttentionCoordinator({
    getSessions: () => stores,
    subscribeSessions: listener => {
      listeners.add(listener)
      return (): void => { listeners.delete(listener) }
    },
    activityStore: useActivityStateStore,
    navigationStore: useNavigationStore,
    playDing, reportError, soundEnabled,
  })
  cleanups.push(dispose)
  return {
    session, record, acknowledge, playDing, reportError, soundEnabled, dispose,
    setSessions: next => { stores = next; for (const listener of Array.from(listeners)) listener() },
    updateWorkspace: data => { session.setState({ workspaces: new Map([['ws-1', entry(data)]]) }) },
  }
}

const cleanups: (() => void)[] = []
const setTab = (state: ActivityState, id = 'tab-1'): void => { useActivityStateStore.getState().setTabState(id, state) }

beforeEach(() => {
  useActivityStateStore.setState({ states: {} })
  useNavigationStore.setState({ activeView: null })
})
afterEach(() => { for (const cleanup of cleanups.splice(0)) cleanup() })

describe('workspace attention coordinator', () => {
  it.each([
    ActivityState.Idle, ActivityState.Completed, ActivityState.Error,
    ActivityState.PermissionRequest, ActivityState.SafePermissionRequested, ActivityState.UserInputRequired,
  ])('records and dings once for Working -> %s', destination => {
    const h = setup()
    setTab(ActivityState.Working)
    setTab(destination)
    setTab(destination)
    expect(h.record).toHaveBeenCalledExactlyOnceWith('ws-1')
    expect(h.playDing).toHaveBeenCalledTimes(1)
  })

  it('waits until the last working tab stops and ignores subsequent nonworking changes', () => {
    const h = setup(workspace(['tab-1', 'tab-2']))
    setTab(ActivityState.Working)
    setTab(ActivityState.Working, 'tab-2')
    setTab(ActivityState.Completed)
    expect(h.record).not.toHaveBeenCalled()
    setTab(ActivityState.PermissionRequest, 'tab-2')
    setTab(ActivityState.Idle)
    expect(h.record).toHaveBeenCalledTimes(1)
  })

  it('records while muted and reads the current setting for the next transition', () => {
    const h = setup()
    h.soundEnabled.mockReturnValue(false)
    setTab(ActivityState.Working)
    setTab(ActivityState.Idle)
    expect(h.record).toHaveBeenCalledTimes(1)
    expect(h.playDing).not.toHaveBeenCalled()
    h.soundEnabled.mockReturnValue(true)
    setTab(ActivityState.Working)
    setTab(ActivityState.Completed)
    expect(h.playDing).toHaveBeenCalledTimes(1)
  })

  it('does not suppress sound in the active view, acknowledges only watched unread metadata', () => {
    useNavigationStore.setState({ activeView: { type: 'workspace', sessionId: 'session-1', workspaceId: 'ws-1' } })
    const h = setup()
    setTab(ActivityState.Working)
    setTab(ActivityState.Completed)
    expect(h.playDing).toHaveBeenCalledTimes(1)
    expect(h.acknowledge).not.toHaveBeenCalled()
    h.updateWorkspace(workspace(['tab-1'], attention('rev-1')))
    expect(h.acknowledge).toHaveBeenCalledExactlyOnceWith('ws-1', 'rev-1')
    setTab(ActivityState.Idle)
    expect(h.acknowledge).toHaveBeenCalledTimes(1)
    h.updateWorkspace(workspace(['tab-1'], attention('rev-2')))
    expect(h.acknowledge).toHaveBeenLastCalledWith('ws-1', 'rev-2')
  })

  it('requires the exact workspace view, not a session, another workspace or another session', () => {
    const h = setup(workspace(['tab-1'], attention('rev-1')))
    useNavigationStore.setState({ activeView: { type: 'session', sessionId: 'session-1' } })
    useNavigationStore.setState({ activeView: { type: 'workspace', sessionId: 'session-1', workspaceId: 'other' } })
    useNavigationStore.setState({ activeView: { type: 'workspace', sessionId: 'other', workspaceId: 'ws-1' } })
    expect(h.acknowledge).not.toHaveBeenCalled()
    useNavigationStore.setState({ activeView: { type: 'workspace', sessionId: 'session-1', workspaceId: 'ws-1' } })
    expect(h.acknowledge).toHaveBeenCalledExactlyOnceWith('ws-1', 'rev-1')
    expect(h.playDing).not.toHaveBeenCalled()
  })

  it('baselines initial snapshots and session/workspace membership changes', () => {
    setTab(ActivityState.Working)
    const h = setup()
    expect(h.record).not.toHaveBeenCalled()
    h.updateWorkspace(workspace([]))
    h.updateWorkspace(workspace(['tab-2']))
    setTab(ActivityState.Idle, 'tab-2')
    expect(h.record).not.toHaveBeenCalled()
    h.setSessions([])
    setTab(ActivityState.Completed)
    h.setSessions([h.session])
    expect(h.record).not.toHaveBeenCalled()
    h.updateWorkspace(workspace())
    setTab(ActivityState.Working)
    setTab(ActivityState.Completed)
    expect(h.record).toHaveBeenCalledTimes(1)
  })

  it('rebaselines activity-state cleanup without ringing', () => {
    const h = setup()
    setTab(ActivityState.Working)
    useActivityStateStore.getState().removeTabState('tab-1')
    setTab(ActivityState.Idle)
    expect(h.record).not.toHaveBeenCalled()
    setTab(ActivityState.Working)
    setTab(ActivityState.Completed)
    expect(h.record).toHaveBeenCalledTimes(1)
  })

  it('ignores restoration and disconnected changes and baselines reconnect', () => {
    const h = setup()
    setTab(ActivityState.Working)
    h.session.setState({ connection: { id: 'connection', status: ConnectionStatus.Disconnected, target: { type: ConnectionTargetType.Local } } })
    setTab(ActivityState.Completed)
    h.session.setState({ connection: { id: 'connection', status: ConnectionStatus.Connected, target: { type: ConnectionTargetType.Local } } })
    h.session.setState({ isRestoring: true })
    setTab(ActivityState.Working)
    setTab(ActivityState.Completed)
    h.session.setState({ isRestoring: false })
    expect(h.record).not.toHaveBeenCalled()
    setTab(ActivityState.Working)
    setTab(ActivityState.Idle)
    expect(h.record).toHaveBeenCalledTimes(1)
  })

  it('disposes all subscriptions, including removed sessions', () => {
    const h = setup()
    h.setSessions([])
    useNavigationStore.setState({ activeView: { type: 'workspace', sessionId: 'session-1', workspaceId: 'ws-1' } })
    h.updateWorkspace(workspace(['tab-1'], attention('rev-1')))
    expect(h.acknowledge).not.toHaveBeenCalled()
    h.dispose()
    h.setSessions([h.session])
    setTab(ActivityState.Working)
    setTab(ActivityState.Completed)
    h.updateWorkspace(workspace(['tab-1'], attention('rev-2')))
    expect(h.record).not.toHaveBeenCalled()
    expect(h.acknowledge).not.toHaveBeenCalled()
  })

  it('commits all baselines before synchronous record publications can re-enter', () => {
    const h = setup()
    const other = makeWorkspace({ ...workspace(['tab-2']), id: 'ws-2' })
    h.session.setState({ workspaces: new Map([['ws-1', entry(workspace())], ['ws-2', entry(other)]]) })
    h.record.mockImplementation(() => { h.session.setState({ sessionVersion: 1 }) })
    useActivityStateStore.setState({ states: { 'tab-1': ActivityState.Working, 'tab-2': ActivityState.Working } })
    useActivityStateStore.setState({ states: { 'tab-1': ActivityState.Completed, 'tab-2': ActivityState.Completed } })
    expect(h.record.mock.calls).toEqual([['ws-1'], ['ws-2']])
    expect(h.playDing).toHaveBeenCalledTimes(2)
  })

  it('removes baselines for unavailable workspaces and handles operation errors as loaded', () => {
    const h = setup()
    setTab(ActivityState.Working)
    h.session.setState({ workspaces: new Map([['ws-1', { status: WorkspaceEntryStatus.Loading, name: 'test', message: '', output: [] }]]) })
    setTab(ActivityState.Completed)
    h.updateWorkspace(workspace())
    expect(h.record).not.toHaveBeenCalled()
    h.session.setState({ workspaces: new Map([['ws-1', { ...entry(workspace()), status: WorkspaceEntryStatus.OperationError, attentionPending: false, error: 'operation failed' } as WorkspaceEntry]]) })
    setTab(ActivityState.Working)
    setTab(ActivityState.Completed)
    expect(h.record).toHaveBeenCalledTimes(1)
  })

  it('acknowledges initial unread metadata but never already acknowledged metadata', () => {
    useNavigationStore.setState({ activeView: { type: 'workspace', sessionId: 'session-1', workspaceId: 'ws-1' } })
    const h = setup(workspace(['tab-1'], attention('rev-1')))
    expect(h.acknowledge).toHaveBeenCalledExactlyOnceWith('ws-1', 'rev-1')
    h.updateWorkspace(workspace(['tab-1'], attention('rev-1', 'rev-1')))
    h.updateWorkspace(workspace(['tab-1'], attention('rev-2', 'rev-2')))
    expect(h.acknowledge).toHaveBeenCalledTimes(1)
  })

  it('reports rejected sound playback without undoing attention', async () => {
    const h = setup()
    const error = new Error('audio blocked')
    h.playDing.mockRejectedValue(error)
    setTab(ActivityState.Working)
    setTab(ActivityState.Completed)
    await Promise.resolve()
    expect(h.reportError).toHaveBeenCalledWith(error)
    expect(h.record).toHaveBeenCalledTimes(1)
  })
})
