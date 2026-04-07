import { useActivityStateStore } from '../store/activityState'
import { ActivityIndicator } from './ActivityIndicator'

export function TabActivityIndicator({ tabId }: { tabId: string }) {
  // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- accessed via Record indexing
  const activityState = useActivityStateStore((state) => state.states[tabId] || 'idle')

  return <ActivityIndicator activityState={activityState} className="tab-activity" />
}
