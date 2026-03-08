import { useState, useRef, useEffect } from 'react'
import { Loader2 } from 'lucide-react'
import type { Tab, ApplicationInstance } from '../types'
import { applicationRegistry } from '../registry/applicationRegistry'
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
  instances: ApplicationInstance[]
  onNewTab: (instanceId: string) => void
}

export default function TabBar({
  tabs,
  activeTabId,
  onSelectTab,
  onCloseTab,
  instances,
  onNewTab
}: TabBarProps) {
  const [menuOpen, setMenuOpen] = useState(false)
  const menuRef = useRef<HTMLDivElement>(null)
  const buttonRef = useRef<HTMLButtonElement>(null)

  // Filter instances to only show those with showInNewTabMenu
  const menuInstances = instances.filter((inst) => {
    const app = applicationRegistry.get(inst.applicationId)
    return app?.showInNewTabMenu
  })

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

  const handleSelectInstance = (instanceId: string) => {
    onNewTab(instanceId)
    setMenuOpen(false)
  }

  const getTabIcon = (tab: Tab): string => {
    // First check if the tab's config has an icon override
    const instance = instances.find((inst) => inst.id === (tab.config as Record<string, unknown>)?.instanceId)
    if (instance) return instance.icon

    // Fall back to application's default icon
    const app = applicationRegistry.get(tab.applicationId)
    return app?.icon || '?'
  }

  const canCloseTab = (tab: Tab): boolean => {
    const app = applicationRegistry.get(tab.applicationId)
    if (!app?.canClose) return false

    // Check if there's more than one tab of this type
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
            {menuInstances.map((inst) => (
              <div
                key={inst.id}
                className="app-menu-item"
                onClick={() => handleSelectInstance(inst.id)}
              >
                <span className="app-menu-icon">{inst.icon}</span>
                <span className="app-menu-name">{inst.name}</span>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  )
}
