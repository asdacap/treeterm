import { useState, useCallback, useEffect } from 'react'
import TreePane from './components/TreePane'
import WorkspacePane from './components/WorkspacePane'
import SettingsDialog from './components/SettingsDialog'
import CloseConfirmDialog from './components/CloseConfirmDialog'
import WorkspacePickerDialog from './components/WorkspacePickerDialog'
import { ErrorBoundary } from './components/ErrorBoundary'
import AppErrorFallback from './components/AppErrorFallback'
import { useAppStore } from './store/app'
import { WorkspaceStoreContext } from './store/WorkspaceStoreContext'
import { ElectronContext } from './store/ElectronContext'

// One-time migration: clear localStorage since daemon is now source of truth
if (typeof localStorage !== 'undefined') {
  localStorage.removeItem('treeterm-workspaces')
}

export default function App() {
  console.log('[App] Component rendering')
  const [treeWidth, setTreeWidth] = useState(250)
  const [isResizing, setIsResizing] = useState(false)

  const {
    electron,
    isSettingsOpen,
    showCloseConfirm,
    unmergedWorkspaces,
    showWorkspacePicker,
    daemonSessions,
    daemonDisconnected,
    getActiveWorkspaceStore,
    handleSessionRestore,
    initialize
  } = useAppStore()

  const activeStore = getActiveWorkspaceStore()

  useEffect(() => {
    let cleanup: (() => void) | null = null
    initialize().then((fn) => { cleanup = fn })
    return () => { cleanup?.() }
  }, [initialize])

  const handleConfirmClose = () => {
    useAppStore.setState({ showCloseConfirm: false })
    electron?.app.confirmClose()
  }

  const handleCancelClose = () => {
    useAppStore.setState({ showCloseConfirm: false })
    electron?.app.cancelClose()
  }

  const handleCreateNewFromPicker = () => {
    useAppStore.setState({ showWorkspacePicker: false })
  }

  const handleOpenInNewWindow = async (session: import('./types').Session) => {
    try {
      const result = await electron!.session.openInNewWindow(session.id)
      if (result.success) {
        useAppStore.setState({ showWorkspacePicker: false })
      } else {
        console.error('Failed to open session in new window:', result.error)
      }
    } catch (error) {
      console.error('Failed to open session in new window:', error)
      alert(`Failed to open session: ${error instanceof Error ? error.message : 'Unknown error'}`)
    }
  }

  const handleMouseDown = useCallback(() => {
    setIsResizing(true)
  }, [])

  const handleMouseMove = useCallback(
    (e: React.MouseEvent) => {
      if (!isResizing) return
      const newWidth = Math.max(150, Math.min(400, e.clientX))
      setTreeWidth(newWidth)
    },
    [isResizing]
  )

  const handleMouseUp = useCallback(() => {
    setIsResizing(false)
  }, [])

  return (
    <ErrorBoundary fallback={<AppErrorFallback />}>
      <ElectronContext.Provider value={electron}>
      <WorkspaceStoreContext.Provider value={activeStore}>
        <div
          className="app"
          onMouseMove={handleMouseMove}
          onMouseUp={handleMouseUp}
          onMouseLeave={handleMouseUp}
        >
          {daemonDisconnected && (
            <div className="daemon-disconnect-banner">
              Daemon disconnected — terminal sessions may be unavailable. Please restart the app.
            </div>
          )}
          {activeStore && (
            <>
              <div className="tree-pane" style={{ width: treeWidth }}>
                <TreePane />
              </div>
              <div
                className={`divider ${isResizing ? 'active' : ''}`}
                onMouseDown={handleMouseDown}
              />
              <div className="workspace-pane">
                <WorkspacePane />
              </div>
            </>
          )}
          <SettingsDialog isOpen={isSettingsOpen} onClose={() => useAppStore.setState({ isSettingsOpen: false })} />
          {showCloseConfirm && (
            <CloseConfirmDialog
              unmergedWorkspaces={unmergedWorkspaces}
              onConfirm={handleConfirmClose}
              onCancel={handleCancelClose}
            />
          )}
          {showWorkspacePicker && (
            <WorkspacePickerDialog
              sessions={daemonSessions}
              onSelect={handleSessionRestore}
              onOpenInNewWindow={handleOpenInNewWindow}
              onCreateNew={handleCreateNewFromPicker}
              onCancel={() => useAppStore.setState({ showWorkspacePicker: false })}
            />
          )}
        </div>
      </WorkspaceStoreContext.Provider>
      </ElectronContext.Provider>
    </ErrorBoundary>
  )
}
