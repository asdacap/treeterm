import { describe, it, expect, vi } from 'vitest'
import { startWorkspaceNotifications } from './startWorkspaceNotifications'
import { useActivityStateStore } from './activityState'
import { useNavigationStore } from './navigation'
import { createStore } from 'zustand/vanilla'
import { ActivityState } from '../types'
import { ConnectionStatus } from '../../shared/types'
import { makeWorkspace } from '../../shared/test-fixtures/workspace'
import { WorkspaceEntryStatus, type SessionState } from './createSessionStore'

describe('notification renderer lifetime', () => {
  it('reads current sound preference and stops reacting after renderer teardown', async () => {
    useActivityStateStore.setState({ states: {} })
    const recordWorkspaceAttention = vi.fn()
    const session = createStore<SessionState>(() => ({
      sessionId: 's1', connection: { status: ConnectionStatus.Connected }, isRestoring: false,
      workspaces: new Map([['w1', { status: WorkspaceEntryStatus.Loaded, attentionPending: false, data: makeWorkspace({ appStates: { t1: { applicationId: 'terminal', title: 'Terminal', state: {} } } }) }]]),
      recordWorkspaceAttention,
    }) as unknown as SessionState)
    let soundEnabled = true
    const player = { playDing: vi.fn().mockResolvedValue(undefined), dispose: vi.fn().mockResolvedValue(undefined) }
    const stop = startWorkspaceNotifications({
      getSessions: () => [session], subscribeSessions: () => () => {},
      activityStore: useActivityStateStore, navigationStore: useNavigationStore,
      soundEnabled: () => soundEnabled, reportError: vi.fn(),
    }, player)
    const transition = (): void => {
      useActivityStateStore.getState().setTabState('t1', ActivityState.Working)
      useActivityStateStore.getState().setTabState('t1', ActivityState.Idle)
    }
    transition()
    expect(player.playDing).toHaveBeenCalledTimes(1)
    soundEnabled = false
    transition()
    expect(player.playDing).toHaveBeenCalledTimes(1)
    expect(recordWorkspaceAttention).toHaveBeenCalledTimes(2)
    stop()
    transition()
    expect(recordWorkspaceAttention).toHaveBeenCalledTimes(2)
    expect(player.dispose).toHaveBeenCalledTimes(1)
    await Promise.resolve()
  })
  it('disposes subscriptions and audio once, reporting async disposal failure', async () => {
    const unsubscribe = vi.fn()
    const reportError = vi.fn()
    const failure = new Error('audio close failed')
    const player = { playDing: vi.fn(), dispose: vi.fn().mockRejectedValue(failure) }
    const stop = startWorkspaceNotifications({
      getSessions: () => [], subscribeSessions: () => unsubscribe,
      activityStore: useActivityStateStore, navigationStore: useNavigationStore,
      soundEnabled: () => true, reportError,
    }, player)
    stop()
    stop()
    await Promise.resolve()
    expect(unsubscribe).toHaveBeenCalledTimes(1)
    expect(player.dispose).toHaveBeenCalledTimes(1)
    expect(reportError).toHaveBeenCalledWith(failure)
    expect(player.playDing).not.toHaveBeenCalled()
  })
})
