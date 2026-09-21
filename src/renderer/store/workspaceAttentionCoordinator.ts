import type { StoreApi } from 'zustand'
import { ConnectionStatus } from '../../shared/types'
import { ActivityState, getTabs } from '../types'
import type { ActivityStateStore } from './activityState'
import { WorkspaceEntryStatus, type SessionState } from './createSessionStore'
import type { useNavigationStore } from './navigation'
import { getWorkspaceAttention, hasUnreadWorkspaceAttention } from './workspaceAttention'

export interface WorkspaceAttentionCoordinatorDeps {
  getSessions: () => Iterable<StoreApi<SessionState>>
  subscribeSessions: (listener: () => void) => () => void
  activityStore: StoreApi<ActivityStateStore>
  navigationStore: StoreApi<ReturnType<typeof useNavigationStore.getState>>
  soundEnabled: () => boolean
  playDing: () => Promise<void>
  reportError: (error: unknown) => void
}

interface Baseline {
  membership: string
  activity: ActivityState
}

/** Owns attention subscriptions for a renderer, independently of mounted workspace views. */
export function createWorkspaceAttentionCoordinator(deps: WorkspaceAttentionCoordinatorDeps): () => void {
  const sessions = new Map<StoreApi<SessionState>, {
    unsubscribe: () => void
    baselines: Map<string, Baseline>
    acknowledged: Map<string, string>
  }>()
  let disposed = false

  function observe(store: StoreApi<SessionState>, activityChanged: boolean): void {
    const tracked = sessions.get(store)
    if (!tracked || disposed) return
    const session = store.getState()
    if (session.connection.status !== ConnectionStatus.Connected || session.isRestoring) {
      tracked.baselines.clear()
      tracked.acknowledged.clear()
      return
    }
    const activity = deps.activityStore.getState()
    const activeView = deps.navigationStore.getState().activeView
    const present = new Set<string>()
    const actions: (() => void)[] = []
    for (const [id, entry] of Array.from(session.workspaces)) {
      if (entry.status !== WorkspaceEntryStatus.Loaded && entry.status !== WorkspaceEntryStatus.OperationError) continue
      present.add(id)
      const tabIds = getTabs(entry.data).map(tab => tab.id).sort()
      // State removal is cleanup, not completion. Include presence as well as tab
      // membership so cleanup before the workspace watch arrives also rebaselines.
      const membership = JSON.stringify(tabIds.map(tabId => [tabId, Object.hasOwn(activity.states, tabId)]))
      const current = activity.getWorkspaceState(tabIds)
      const previous = tracked.baselines.get(id)
      tracked.baselines.set(id, { membership, activity: current })
      if (activityChanged && previous?.membership === membership && previous.activity === ActivityState.Working && current !== ActivityState.Working) {
        actions.push(() => {
          session.recordWorkspaceAttention(id)
          if (deps.soundEnabled()) {
            void deps.playDing().catch(deps.reportError)
          }
        })
      }
      if (!hasUnreadWorkspaceAttention(entry.data.metadata)) {
        tracked.acknowledged.delete(id)
        continue
      }
      // ActiveView is an existing string-discriminated navigation type.
      // eslint-disable-next-line custom/no-string-literal-comparison
      if (activeView?.type !== 'workspace' || activeView.sessionId !== session.sessionId || activeView.workspaceId !== id) continue
      const { revision } = getWorkspaceAttention(entry.data.metadata)
      if (tracked.acknowledged.get(id) === revision) continue
      tracked.acknowledged.set(id, revision)
      actions.push(() => { session.acknowledgeWorkspaceAttention(id, revision) })
    }
    for (const id of Array.from(tracked.baselines.keys())) {
      if (!present.has(id)) {
        tracked.baselines.delete(id)
        tracked.acknowledged.delete(id)
      }
    }
    // Commit every baseline before actions can synchronously publish session changes.
    for (const action of actions) action()
  }

  function refreshSessions(): void {
    if (disposed) return
    const current = new Set(deps.getSessions())
    for (const [store, tracked] of Array.from(sessions)) {
      if (!current.has(store)) {
        tracked.unsubscribe()
        sessions.delete(store)
      }
    }
    for (const store of Array.from(current)) {
      if (sessions.has(store)) continue
      sessions.set(store, {
        unsubscribe: store.subscribe(() => { observe(store, false) }),
        baselines: new Map(),
        acknowledged: new Map(),
      })
      observe(store, false)
    }
  }

  const unsubscribeSessions = deps.subscribeSessions(refreshSessions)
  const unsubscribeActivity = deps.activityStore.subscribe(() => {
    for (const store of Array.from(sessions.keys())) observe(store, true)
  })
  const unsubscribeNavigation = deps.navigationStore.subscribe(() => {
    for (const store of Array.from(sessions.keys())) observe(store, false)
  })
  refreshSessions()
  return (): void => {
    disposed = true
    unsubscribeSessions()
    unsubscribeActivity()
    unsubscribeNavigation()
    for (const tracked of Array.from(sessions.values())) tracked.unsubscribe()
    sessions.clear()
  }
}
