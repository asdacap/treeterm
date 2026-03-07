import { useState, useRef, useEffect } from 'react'
import type { WorkspaceTab, Application } from '../types'

interface TabBarProps {
  tabs: WorkspaceTab[]
  activeTabId: string | null
  onSelectTab: (id: string) => void
  onCloseTab: (id: string) => void
  applications: Application[]
  onNewApplication: (applicationId: string) => void
}

export default function TabBar({
  tabs,
  activeTabId,
  onSelectTab,
  onCloseTab,
  applications,
  onNewApplication
}: TabBarProps) {
  const [menuOpen, setMenuOpen] = useState(false)
  const menuRef = useRef<HTMLDivElement>(null)
  const buttonRef = useRef<HTMLButtonElement>(null)

  const terminalTabs = tabs.filter((t) => t.type === 'terminal')
  const canCloseTabs = terminalTabs.length > 1

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

  const handleSelectApp = (appId: string) => {
    onNewApplication(appId)
    setMenuOpen(false)
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
            <span className="tab-icon">{tab.type === 'terminal' ? '>' : '\uD83D\uDCC2'}</span>
            <span className="tab-title">
              {tab.title}
              {tab.type === 'terminal' && tab.ptyId && ` [${tab.ptyId}]`}
            </span>
            {tab.type === 'terminal' && canCloseTabs && (
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
          title="New application"
        >
          +
        </button>
        {menuOpen && (
          <div className="app-menu" ref={menuRef}>
            {applications.map((app) => (
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
