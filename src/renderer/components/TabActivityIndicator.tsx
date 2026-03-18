import { Loader2 } from 'lucide-react'
import { useActivityStateStore } from '../store/activityState'

export function TabActivityIndicator({ tabId }: { tabId: string }) {
  const activityState = useActivityStateStore((state) => state.states[tabId] || 'idle')

  const indicators: Record<string, { icon: React.ReactNode; title: string }> = {
    idle: { icon: '○', title: 'Idle' },
    working: { icon: <Loader2 size={10} />, title: 'Working...' },
    waiting_for_input: { icon: '●', title: 'Waiting for input' }
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
