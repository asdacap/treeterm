import { useState, useCallback } from 'react'
import TreePane from './components/TreePane'
import WorkspacePane from './components/WorkspacePane'
import SettingsDialog from './components/SettingsDialog'
import CloseConfirmDialog from './components/CloseConfirmDialog'
import WorkspacePickerDialog from './components/WorkspacePickerDialog'
import ActiveProcessesDialog from './components/ActiveProcessesDialog'
import { ErrorBoundary } from './components/ErrorBoundary'
import AppErrorFallback from './components/AppErrorFallback'
import { useAppStore } from './store/app'
import { TerminalApiContext } from './contexts/TerminalApiContext'
import { FilesystemApiContext } from './contexts/FilesystemApiContext'
import { GitApiContext } from './contexts/GitApiContext'
import { STTApiContext } from './contexts/STTApiContext'

// One-time migration: clear localStorage since daemon is now source of truth
if (typeof localStorage !== 'undefined') {
  localStorage.removeItem('treeterm-workspaces')
}

export default function App() {
  console.log('[App] Component rendering')
  const [treeWidth, setTreeWidth] = useState(250)
  const [isResizing, setIsResizing] = useState(false)

  const {
    platform,
    terminal,
    filesystem,
    git,
    stt,
    sandbox,
    appApi,
    sessionApi,
    selectFolder,
    getRecentDirectories,
    isSettingsOpen,
    isActiveProcessesOpen,
    showCloseConfirm,
    unmergedWorkspaces,
    showWorkspacePicker,
    daemonSessions,
    daemonDisconnected,
    getActiveWorkspaceStore,
    handleSessionRestore,
  } = useAppStore()

  const activeStore = getActiveWorkspaceStore()

  const handleConfirmClose = () => {
    useAppStore.setState({ showCloseConfirm: false })
    appApi.confirmClose()
  }

  const handleCancelClose = () => {
    useAppStore.setState({ showCloseConfirm: false })
    appApi.cancelClose()
  }

  const handleCreateNewFromPicker = () => {
    useAppStore.getState().createNewSession()
  }

  const handleOpenInNewWindow = async (session: import('./types').Session) => {
    try {
      const result = await sessionApi.openInNewWindow(session.id)
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
      <TerminalApiContext.Provider value={terminal}>
      <FilesystemApiContext.Provider value={filesystem}>
      <GitApiContext.Provider value={git}>
      <STTApiContext.Provider value={stt}>
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
                <TreePane
                  workspaceStore={activeStore}
                  selectFolder={selectFolder}
                  getRecentDirectories={getRecentDirectories}
                />
              </div>
              <div
                className={`divider ${isResizing ? 'active' : ''}`}
                onMouseDown={handleMouseDown}
              />
              <div className="workspace-pane">
                <WorkspacePane workspaceStore={activeStore} platform={platform} />
              </div>
            </>
          )}
          <SettingsDialog
            isOpen={isSettingsOpen}
            onClose={() => useAppStore.setState({ isSettingsOpen: false })}
            sandbox={sandbox}
            platform={platform}
          />
          {showCloseConfirm && (
            <CloseConfirmDialog
              unmergedWorkspaces={unmergedWorkspaces}
              onConfirm={handleConfirmClose}
              onCancel={handleCancelClose}
            />
          )}
          {isActiveProcessesOpen && (
            <ActiveProcessesDialog
              terminalApi={terminal}
              workspaces={activeStore?.getState().workspaces ?? {}}
              onClose={() => useAppStore.setState({ isActiveProcessesOpen: false })}
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
      </STTApiContext.Provider>
      </GitApiContext.Provider>
      </FilesystemApiContext.Provider>
      </TerminalApiContext.Provider>
    </ErrorBoundary>
  )
}
