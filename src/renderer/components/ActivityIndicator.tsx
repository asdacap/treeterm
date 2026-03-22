import { Loader2 } from 'lucide-react'
import type { ActivityState } from '../types'

const indicators: Record<ActivityState, { icon: React.ReactNode; title: string }> = {
  idle: { icon: '○', title: 'Idle' },
  working: { icon: <Loader2 size="1em" />, title: 'Working...' },
  user_input_required: { icon: '▶', title: 'Input required' },
  permission_request: { icon: '●', title: 'Permission request' },
  safe_permission_requested: { icon: '●', title: 'Safe permission' },
  completed: { icon: '✓', title: 'Completed' },
  error: { icon: '●', title: 'Error' }
}

interface ActivityIndicatorProps {
  activityState: ActivityState
  className: string
}

export function ActivityIndicator({ activityState, className }: ActivityIndicatorProps) {
  const { icon, title } = indicators[activityState]

  return (
    <span
      className={`${className} activity-${activityState}`}
      title={title}
    >
      {icon}
    </span>
  )
}
