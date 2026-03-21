import { Loader2 } from 'lucide-react'
import { useActivityStateStore } from '../store/activityState'
import type { ActivityState } from '../types'

export function TabActivityIndicator({ tabId }: { tabId: string }) {
  const activityState = useActivityStateStore((state) => state.states[tabId] || 'idle')

  const indicators: Record<ActivityState, { icon: React.ReactNode; title: string }> = {
    idle: { icon: '○', title: 'Idle' },
    working: { icon: <Loader2 size={10} />, title: 'Working...' },
    user_input_required: { icon: '●', title: 'Input required' },
    permission_request: { icon: '●', title: 'Permission request' },
    safe_permission_requested: { icon: '●', title: 'Safe permission' },
    completed: { icon: '✓', title: 'Completed' },
    error: { icon: '●', title: 'Error' }
  }

  const { icon, title } = indicators[activityState]

  return (
    <span
      className={`tab-activity tab-activity-${activityState}`}
      title={title}
    >
      {icon}
    </span>
  )
}
