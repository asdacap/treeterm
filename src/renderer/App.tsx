import { useState, useCallback } from 'react'
import TreePane from './components/TreePane'
import WorkspacePane from './components/WorkspacePane'
import SettingsDialog from './components/SettingsDialog'
import CloseConfirmDialog from './components/CloseConfirmDialog'
import WorkspacePickerDialog from './components/WorkspacePickerDialog'
import ActiveProcessesDialog from './components/ActiveProcessesDialog'
import ConnectionPicker from './components/ConnectionPicker'
import SessionInfoPane from './components/SessionInfoPane'
import { ErrorBoundary } from './components/ErrorBoundary'
import AppErrorFallback from './components/AppErrorFallback'
import { useAppStore } from './store/app'
import { useNavigationStore } from './store/navigation'
import { useSettingsStore } from './store/settings'
import { STTApiContext } from './contexts/STTApiContext'

// One-time migration: clear localStorage since daemon is now source of truth
if (typeof localStorage !== 'undefined') {
  localStorage.removeItem('treeterm-workspaces')
}

export default function App() {
  console.log('[App] Component rendering')
  const isSettingsLoaded = useSettingsStore(s => s.isLoaded)
  const [treeWidth, setTreeWidth] = useState(250)
  const [isResizing, setIsResizing] = useState(false)

  const {
    platform,
    stt,
    sandbox,
    appApi,
    sessionApi,
    selectFolder,
    isSettingsOpen,
    isActiveProcessesOpen,
    showCloseConfirm,
    unmergedWorkspaces,
    showWorkspacePicker,
    daemonSessions,
    daemonDisconnected,
    showConnectionPicker,
  } = useAppStore()

  const activeView = useNavigationStore(s => s.activeView)
  const sessionStores = useAppStore(s => s.sessionStores)

  // Derive active session store from activeView
  const activeSessionId = activeView?.type === 'workspace' ? activeView.sessionId
    : activeView?.type === 'session' ? activeView.sessionId
    : null
  const activeSessionStore = activeSessionId
    ? sessionStores[activeSessionId] || null
    : null

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

  if (!isSettingsLoaded) {
    return (
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', height: '100vh', color: 'var(--text-secondary)' }}>
        <span>Loading settings…</span>
      </div>
    )
  }

  return (
    <ErrorBoundary fallback={<AppErrorFallback />}>
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
          <div className="tree-pane" style={{ width: treeWidth }}>
            <TreePane
              selectFolder={selectFolder}
            />
          </div>
          <div
            className={`divider ${isResizing ? 'active' : ''}`}
            onMouseDown={handleMouseDown}
          />
          <div className="workspace-pane">
            {activeSessionStore ? (
              <>
                <div style={{ display: activeView?.type === 'workspace' ? 'contents' : 'none' }}>
                  <WorkspacePane sessionStore={activeSessionStore} platform={platform} />
                </div>
                {activeView?.type === 'session' && (
                  <SessionInfoPane sessionId={activeView.sessionId} sessionStore={activeSessionStore} />
                )}
              </>
            ) : (
              <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', height: '100%', color: 'var(--text-secondary)' }}>
                <span>Select a workspace to get started</span>
              </div>
            )}
          </div>
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
              workspaces={activeSessionStore?.getState().workspaces ?? {}}
              connectionId={activeSessionStore?.getState().connection?.id ?? 'local'}
              onClose={() => useAppStore.setState({ isActiveProcessesOpen: false })}
            />
          )}
          <ConnectionPicker
            isOpen={showConnectionPicker}
            onClose={() => useAppStore.setState({ showConnectionPicker: false })}
          />
          {showWorkspacePicker && (
            <WorkspacePickerDialog
              sessions={daemonSessions}
              onSelect={(session) => {
                const store = useAppStore.getState()
                // If the session store already exists, restore directly
                const sessionStore = store.sessionStores[session.id]
                if (sessionStore) {
                  sessionStore.getState().handleRestore(session)
                }
                // Navigate to first workspace if available
                if (session.workspaces && session.workspaces.length > 0) {
                  useNavigationStore.getState().setActiveView({ type: 'workspace', workspaceId: session.workspaces[0].id, sessionId: session.id })
                }
                useAppStore.setState({ showWorkspacePicker: false })
              }}
              onOpenInNewWindow={handleOpenInNewWindow}
              onCreateNew={handleCreateNewFromPicker}
              onCancel={() => useAppStore.setState({ showWorkspacePicker: false })}
            />
          )}
        </div>
      </STTApiContext.Provider>
    </ErrorBoundary>
  )
}
