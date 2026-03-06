import { useEffect, useCallback } from 'react'
import { useWorkspaceStore } from '../store/workspace'
import { useSettingsStore } from '../store/settings'
import TabBar from './TabBar'
import Terminal from './Terminal'

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
    removeTerminal,
    setActiveTerminal
  } = useWorkspaceStore()

  const { settings } = useSettingsStore()
  const keybindings = settings.keybindings

  const activeWorkspace = activeWorkspaceId ? workspaces[activeWorkspaceId] : null

  const handleNewTab = useCallback(() => {
    if (activeWorkspaceId) {
      addTerminal(activeWorkspaceId)
    }
  }, [activeWorkspaceId, addTerminal])

  const handleCloseTab = useCallback(
    (terminalId: string) => {
      if (activeWorkspaceId) {
        removeTerminal(activeWorkspaceId, terminalId)
      }
    },
    [activeWorkspaceId, removeTerminal]
  )

  const handleSelectTab = useCallback(
    (terminalId: string) => {
      if (activeWorkspaceId) {
        setActiveTerminal(activeWorkspaceId, terminalId)
      }
    },
    [activeWorkspaceId, setActiveTerminal]
  )

  // Keyboard shortcuts
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (!activeWorkspace) return

      // New tab
      if (matchesKeybinding(e, keybindings.newTab)) {
        e.preventDefault()
        handleNewTab()
        return
      }

      // Close tab
      if (matchesKeybinding(e, keybindings.closeTab)) {
        e.preventDefault()
        if (activeWorkspace.activeTerminalId && activeWorkspace.terminals.length > 1) {
          handleCloseTab(activeWorkspace.activeTerminalId)
        }
        return
      }

      // Cmd+1-9: Switch to tab by number (keep hardcoded as these are standard)
      if ((e.metaKey || e.ctrlKey) && e.key >= '1' && e.key <= '9') {
        e.preventDefault()
        const index = parseInt(e.key) - 1
        if (index < activeWorkspace.terminals.length) {
          handleSelectTab(activeWorkspace.terminals[index].id)
        }
        return
      }

      // Previous tab
      if (matchesKeybinding(e, keybindings.prevTab)) {
        e.preventDefault()
        const currentIndex = activeWorkspace.terminals.findIndex(
          (t) => t.id === activeWorkspace.activeTerminalId
        )
        const newIndex = currentIndex > 0 ? currentIndex - 1 : activeWorkspace.terminals.length - 1
        handleSelectTab(activeWorkspace.terminals[newIndex].id)
        return
      }

      // Next tab
      if (matchesKeybinding(e, keybindings.nextTab)) {
        e.preventDefault()
        const currentIndex = activeWorkspace.terminals.findIndex(
          (t) => t.id === activeWorkspace.activeTerminalId
        )
        const newIndex = currentIndex < activeWorkspace.terminals.length - 1 ? currentIndex + 1 : 0
        handleSelectTab(activeWorkspace.terminals[newIndex].id)
        return
      }
    }

    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [activeWorkspace, keybindings, handleNewTab, handleCloseTab, handleSelectTab])

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

  // Handle legacy workspaces without terminals array
  const terminals = activeWorkspace.terminals || [{ id: 'default', title: 'Terminal 1' }]
  const activeTerminalId = activeWorkspace.activeTerminalId || terminals[0]?.id

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
        tabs={terminals}
        activeTabId={activeTerminalId}
        onSelectTab={handleSelectTab}
        onCloseTab={handleCloseTab}
        onNewTab={handleNewTab}
      />
      <div className="workspace-terminal">
        {terminals.map((terminal) => (
          <div
            key={terminal.id}
            className="terminal-wrapper"
            style={{ display: terminal.id === activeTerminalId ? 'block' : 'none' }}
          >
            <Terminal
              cwd={activeWorkspace.path}
              workspaceId={activeWorkspace.id}
              terminalId={terminal.id}
            />
          </div>
        ))}
      </div>
    </div>
  )
}
