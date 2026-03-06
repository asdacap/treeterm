import { useEffect, useCallback } from 'react'
import { useWorkspaceStore } from '../store/workspace'
import { useSettingsStore } from '../store/settings'
import TabBar from './TabBar'
import Terminal from './Terminal'
import { FilesystemBrowser } from './FilesystemBrowser'
import type { TerminalTab } from '../types'

function matchesKeybinding(e: KeyboardEvent, keybinding: string): boolean {
  const parts = keybinding.split('+')
  const key = parts[parts.length - 1]
  const hasCmd = parts.includes('CommandOrControl')
  const hasShift = parts.includes('Shift')
  const hasAlt = parts.includes('Alt')

  const cmdMatch = hasCmd ? (e.metaKey || e.ctrlKey) : (!e.metaKey && !e.ctrlKey)
  const shiftMatch = hasShift ? e.shiftKey : !e.shiftKey
  const altMatch = hasAlt ? e.altKey : !e.altKey

  const pressedKey = e.key.length === 1 ? e.key.toUpperCase() : e.key
  const targetKey = key.length === 1 ? key.toUpperCase() : key
  const keyMatch = pressedKey === targetKey || e.key === key

  return cmdMatch && shiftMatch && altMatch && keyMatch
}

export default function WorkspacePane() {
  const {
    workspaces,
    activeWorkspaceId,
    addTerminal,
    addFilesystemTab,
    removeTab,
    setActiveTab
  } = useWorkspaceStore()

  const { settings } = useSettingsStore()
  const keybindings = settings.keybindings

  const activeWorkspace = activeWorkspaceId ? workspaces[activeWorkspaceId] : null

  const handleNewTerminal = useCallback(() => {
    if (activeWorkspaceId) {
      addTerminal(activeWorkspaceId)
    }
  }, [activeWorkspaceId, addTerminal])

  const handleNewFilesystemTab = useCallback(() => {
    if (activeWorkspaceId) {
      addFilesystemTab(activeWorkspaceId)
    }
  }, [activeWorkspaceId, addFilesystemTab])

  const handleCloseTab = useCallback(
    (tabId: string) => {
      if (activeWorkspaceId) {
        removeTab(activeWorkspaceId, tabId)
      }
    },
    [activeWorkspaceId, removeTab]
  )

  const handleSelectTab = useCallback(
    (tabId: string) => {
      if (activeWorkspaceId) {
        setActiveTab(activeWorkspaceId, tabId)
      }
    },
    [activeWorkspaceId, setActiveTab]
  )

  // Keyboard shortcuts
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (!activeWorkspace) return

      // New terminal tab
      if (matchesKeybinding(e, keybindings.newTab)) {
        e.preventDefault()
        handleNewTerminal()
        return
      }

      // Close tab
      if (matchesKeybinding(e, keybindings.closeTab)) {
        e.preventDefault()
        if (activeWorkspace.activeTabId && activeWorkspace.tabs.length > 1) {
          handleCloseTab(activeWorkspace.activeTabId)
        }
        return
      }

      // Cmd+1-9: Switch to tab by number (keep hardcoded as these are standard)
      if ((e.metaKey || e.ctrlKey) && e.key >= '1' && e.key <= '9') {
        e.preventDefault()
        const index = parseInt(e.key) - 1
        if (index < activeWorkspace.tabs.length) {
          handleSelectTab(activeWorkspace.tabs[index].id)
        }
        return
      }

      // Previous tab
      if (matchesKeybinding(e, keybindings.prevTab)) {
        e.preventDefault()
        const currentIndex = activeWorkspace.tabs.findIndex(
          (t) => t.id === activeWorkspace.activeTabId
        )
        const newIndex = currentIndex > 0 ? currentIndex - 1 : activeWorkspace.tabs.length - 1
        handleSelectTab(activeWorkspace.tabs[newIndex].id)
        return
      }

      // Next tab
      if (matchesKeybinding(e, keybindings.nextTab)) {
        e.preventDefault()
        const currentIndex = activeWorkspace.tabs.findIndex(
          (t) => t.id === activeWorkspace.activeTabId
        )
        const newIndex = currentIndex < activeWorkspace.tabs.length - 1 ? currentIndex + 1 : 0
        handleSelectTab(activeWorkspace.tabs[newIndex].id)
        return
      }
    }

    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [activeWorkspace, keybindings, handleNewTerminal, handleCloseTab, handleSelectTab])

  if (!activeWorkspace) {
    return (
      <div className="workspace-empty">
        <div className="workspace-empty-content">
          <h2>No workspace selected</h2>
          <p>Select a workspace from the sidebar or add a new one to get started.</p>
        </div>
      </div>
    )
  }

  // Handle legacy workspaces - migrate terminals to tabs format
  const tabs = activeWorkspace.tabs || []
  const activeTabId = activeWorkspace.activeTabId || tabs[0]?.id

  return (
    <div className="workspace-content">
      <div className="workspace-header">
        <span className="workspace-title">{activeWorkspace.name}</span>
        <span className="workspace-path">{activeWorkspace.path}</span>
        {activeWorkspace.gitBranch && (
          <span className="workspace-branch">{activeWorkspace.gitBranch}</span>
        )}
      </div>
      <TabBar
        tabs={tabs}
        activeTabId={activeTabId}
        onSelectTab={handleSelectTab}
        onCloseTab={handleCloseTab}
        onNewTerminal={handleNewTerminal}
        onNewFilesystemTab={handleNewFilesystemTab}
      />
      <div className="workspace-terminal">
        {/* Render tabs for ALL workspaces to keep PTYs alive when switching */}
        {Object.values(workspaces).map((workspace) => {
          const wsTabs = workspace.tabs || []
          const wsActiveTabId = workspace.activeTabId || wsTabs[0]?.id
          const isActiveWorkspace = workspace.id === activeWorkspaceId

          return wsTabs.map((tab) => {
            const isVisible = isActiveWorkspace && tab.id === wsActiveTabId

            if (tab.type === 'terminal') {
              return (
                <div
                  key={`${workspace.id}-${tab.id}`}
                  className="terminal-wrapper"
                  style={{ display: isVisible ? 'block' : 'none' }}
                >
                  <Terminal
                    cwd={workspace.path}
                    workspaceId={workspace.id}
                    terminalId={tab.id}
                  />
                </div>
              )
            } else {
              // Filesystem browser - only render when active workspace (no need to keep alive)
              if (!isActiveWorkspace) return null
              return (
                <div
                  key={`${workspace.id}-${tab.id}`}
                  className="filesystem-wrapper"
                  style={{ display: isVisible ? 'flex' : 'none' }}
                >
                  <FilesystemBrowser
                    workspacePath={workspace.path}
                    workspaceId={workspace.id}
                    tabId={tab.id}
                  />
                </div>
              )
            }
          })
        })}
      </div>
    </div>
  )
}
