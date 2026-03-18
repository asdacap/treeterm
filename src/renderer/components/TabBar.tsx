import { useState, useRef, useEffect } from 'react'
import { Loader2 } from 'lucide-react'
import type { Tab } from '../types'
import { useAppStore } from '../store/app'
import { useActivityStateStore } from '../store/activityState'

// Small component to subscribe to activity state for a single tab
function TabActivityIndicator({ tabId }: { tabId: string }) {
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

interface TabBarProps {
  tabs: Tab[]
  activeTabId: string | null
  onSelectTab: (id: string) => void
  onCloseTab: (id: string) => void
  onNewTab: (applicationId: string) => void
}

export default function TabBar({
  tabs,
  activeTabId,
  onSelectTab,
  onCloseTab,
  onNewTab
}: TabBarProps): JSX.Element {
  const [menuOpen, setMenuOpen] = useState(false)
  const menuRef = useRef<HTMLDivElement>(null)
  const buttonRef = useRef<HTMLButtonElement>(null)

  const menuApps = useAppStore((s) => s.getMenuApplications())
  const getApplication = useAppStore((s) => s.getApplication)

  // Close menu when clicking outside
  useEffect(() => {
    const handleClickOutside = (e: MouseEvent) => {
      if (
        menuOpen &&
        menuRef.current &&
        !menuRef.current.contains(e.target as Node) &&
        buttonRef.current &&
        !buttonRef.current.contains(e.target as Node)
      ) {
        setMenuOpen(false)
      }
    }

    document.addEventListener('mousedown', handleClickOutside)
    return () => document.removeEventListener('mousedown', handleClickOutside)
  }, [menuOpen])

  const handleSelectApp = (applicationId: string) => {
    onNewTab(applicationId)
    setMenuOpen(false)
  }

  const getTabIcon = (tab: Tab): string => {
    const app = getApplication(tab.applicationId)
    return app?.icon || '?'
  }

  const canCloseTab = (tab: Tab): boolean => {
    const app = getApplication(tab.applicationId)
    if (!app?.canClose) return false

    // Apps that can have multiple instances can always be closed
    if (app.canHaveMultiple) return true

    // Non-default single-instance apps can always be closed (opened on demand)
    if (!app.isDefault) return true

    // For default single-instance apps, require more than one tab of this type
    const sameTypeTabs = tabs.filter((t) => t.applicationId === tab.applicationId)
    return sameTypeTabs.length > 1
  }

  return (
    <div className="tab-bar">
      <div className="tab-list">
        {tabs.map((tab) => (
          <div
            key={tab.id}
            className={`tab ${activeTabId === tab.id ? 'active' : ''}`}
            onClick={() => onSelectTab(tab.id)}
          >
            <span className="tab-icon">{getTabIcon(tab)}</span>
            <span className="tab-title">{tab.title}</span>
            <TabActivityIndicator tabId={tab.id} />
            {canCloseTab(tab) && (
              <button
                className="tab-close"
                onClick={(e) => {
                  e.stopPropagation()
                  onCloseTab(tab.id)
                }}
                title="Close tab"
              >
                x
              </button>
            )}
          </div>
        ))}
      </div>
      <div className="tab-new-container">
        <button
          ref={buttonRef}
          className="tab-new"
          onClick={() => setMenuOpen(!menuOpen)}
          title="New tab"
        >
          +
        </button>
        {menuOpen && (
          <div className="app-menu" ref={menuRef}>
            {menuApps.map((app) => (
              <div
                key={app.id}
                className="app-menu-item"
                onClick={() => handleSelectApp(app.id)}
              >
                <span className="app-menu-icon">{app.icon}</span>
                <span className="app-menu-name">{app.name}</span>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  )
}
