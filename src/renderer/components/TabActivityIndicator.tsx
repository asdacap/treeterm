import { useActivityStateStore } from '../store/activityState'
import { ActivityIndicator } from './ActivityIndicator'

export function TabActivityIndicator({ tabId }: { tabId: string }) {
  const activityState = useActivityStateStore((state) => state.states[tabId] || 'idle')

  return <ActivityIndicator activityState={activityState} className="tab-activity" />
}
