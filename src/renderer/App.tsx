import { useState, useCallback } from 'react'
import TreePane from './components/TreePane'
import WorkspacePane from './components/WorkspacePane'
import SettingsDialog from './components/SettingsDialog'
import CloseConfirmDialog from './components/CloseConfirmDialog'
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
    selectFolder,
    isSettingsOpen,
    isActiveProcessesOpen,
    showCloseConfirm,
    unmergedWorkspaces,
    daemonDisconnected,
    showConnectionPicker,
  } = useAppStore()

  const activeView = useNavigationStore(s => s.activeView)
  const sessionStores = useAppStore(s => s.sessionStores)

  // Derive active session entry from activeView
  const activeSessionId = activeView && 'sessionId' in activeView ? activeView.sessionId : null
  const activeSessionEntry = activeSessionId
    ? sessionStores[activeSessionId] ?? null
    : null
  const activeSessionStore = activeSessionEntry?.status === 'connected' ? activeSessionEntry.store : null

  const handleConfirmClose = () => {
    useAppStore.setState({ showCloseConfirm: false })
    appApi.confirmClose()
  }

  const handleCancelClose = () => {
    useAppStore.setState({ showCloseConfirm: false })
    appApi.cancelClose()
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
            {activeSessionEntry?.status === 'connecting' ? (
              <SessionInfoPane status="connecting" sessionId={activeSessionEntry.connectionId} connectionId={activeSessionEntry.connectionId} config={activeSessionEntry.config} />
            ) : activeSessionEntry?.status === 'error' ? (
              <SessionInfoPane status="error" sessionId={activeSessionEntry.connectionId} connectionId={activeSessionEntry.connectionId} config={activeSessionEntry.config} error={activeSessionEntry.error} />
            ) : activeSessionStore ? (
              <>
                <div style={{ display: activeView?.type === 'workspace' ? 'contents' : 'none' }}>
                  <WorkspacePane sessionStore={activeSessionStore} platform={platform} />
                </div>
                {activeView?.type === 'session' && (
                  <SessionInfoPane status="connected" sessionId={activeView.sessionId} sessionStore={activeSessionStore} />
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
          {isActiveProcessesOpen && activeSessionStore && (
            <ActiveProcessesDialog
              workspaces={Object.fromEntries(
                Object.entries(activeSessionStore.getState().workspaces)
                  .filter(([, e]) => e.status === 'loaded' || e.status === 'operation-error')
                  .map(([id, e]) => [id, (e as Extract<typeof e, { status: 'loaded' | 'operation-error' }>).data])
              )}
              connectionId={activeSessionStore.getState().connection?.id ?? 'local'}
              onClose={() => useAppStore.setState({ isActiveProcessesOpen: false })}
            />
          )}
          <ConnectionPicker
            isOpen={showConnectionPicker}
            onClose={() => useAppStore.setState({ showConnectionPicker: false })}
          />
        </div>
      </STTApiContext.Provider>
    </ErrorBoundary>
  )
}
