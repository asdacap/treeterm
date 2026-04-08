import { useActivityStateStore } from '../store/activityState'
import { ActivityIndicator } from './ActivityIndicator'
import { ActivityState } from '../types'

export function TabActivityIndicator({ tabId }: { tabId: string }) {
  const activityState = useActivityStateStore((state) => state.states[tabId] || ActivityState.Idle)

  return <ActivityIndicator activityState={activityState} className="tab-activity" />
}
