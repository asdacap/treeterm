import { useState } from 'react'
import type { WorkspaceTab } from '../types'

interface TabBarProps {
  tabs: WorkspaceTab[]
  activeTabId: string | null
  onSelectTab: (id: string) => void
  onCloseTab: (id: string) => void
  onNewTerminal: () => void
  onNewFilesystemTab: () => void
}

export default function TabBar({
  tabs,
  activeTabId,
  onSelectTab,
  onCloseTab,
  onNewTerminal,
  onNewFilesystemTab
}: TabBarProps) {
  const [showNewMenu, setShowNewMenu] = useState(false)

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
            <span className="tab-title">{tab.title}</span>
            {tabs.length > 1 && (
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
          className="tab-new"
          onClick={() => setShowNewMenu(!showNewMenu)}
          title="New tab"
        >
          +
        </button>
        {showNewMenu && (
          <div className="tab-new-menu">
            <button
              className="tab-new-menu-item"
              onClick={() => {
                onNewTerminal()
                setShowNewMenu(false)
              }}
            >
              <span className="tab-icon">{'>'}</span> Terminal
            </button>
            <button
              className="tab-new-menu-item"
              onClick={() => {
                onNewFilesystemTab()
                setShowNewMenu(false)
              }}
            >
              <span className="tab-icon">{'\uD83D\uDCC2'}</span> Files
            </button>
          </div>
        )}
      </div>
    </div>
  )
}
