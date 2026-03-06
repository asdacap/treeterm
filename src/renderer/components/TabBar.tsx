import type { WorkspaceTab } from '../types'

interface TabBarProps {
  tabs: WorkspaceTab[]
  activeTabId: string | null
  onSelectTab: (id: string) => void
  onCloseTab: (id: string) => void
  onNewTerminal: () => void
}

export default function TabBar({
  tabs,
  activeTabId,
  onSelectTab,
  onCloseTab,
  onNewTerminal
}: TabBarProps) {
  const terminalTabs = tabs.filter((t) => t.type === 'terminal')
  const canCloseTabs = terminalTabs.length > 1

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
      <button
        className="tab-new"
        onClick={onNewTerminal}
        title="New terminal"
      >
        +
      </button>
    </div>
  )
}
