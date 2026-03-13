import { useState, useCallback, useEffect, useRef } from 'react'
import TreePane from './components/TreePane'
import WorkspacePane from './components/WorkspacePane'
import SettingsDialog from './components/SettingsDialog'
import CloseConfirmDialog from './components/CloseConfirmDialog'
import WorkspacePickerDialog from './components/WorkspacePickerDialog'
import { ErrorBoundary } from './components/ErrorBoundary'
import AppErrorFallback from './components/AppErrorFallback'
import { useSettingsStore } from './store/settings'
import { useWorkspaceStore, getUnmergedSubWorkspaces } from './store/workspace'
import type { Workspace, Session, TerminalState, Tab, SessionInfo } from './types'

// Helper types for session restoration
type SessionMap = Map<string, SessionInfo>
type AddTabWithStateFn = <T>(workspaceId: string, applicationId: string, initialState: Partial<T>, existingTabId?: string) => string
type SetActiveTabFn = (workspaceId: string, tabId: string) => void

export default function App() {
  console.log('[App] Component rendering')
  const [treeWidth, setTreeWidth] = useState(250)
  const [isResizing, setIsResizing] = useState(false)
  const [isSettingsOpen, setIsSettingsOpen] = useState(false)
  const [showCloseConfirm, setShowCloseConfirm] = useState(false)
  const [unmergedWorkspaces, setUnmergedWorkspaces] = useState<Workspace[]>([])
  const [daemonSessions, setDaemonSessions] = useState<Session[]>([])
  const [showWorkspacePicker, setShowWorkspacePicker] = useState(false)
  const [daemonDisconnected, setDaemonDisconnected] = useState(false)
  const { loadSettings } = useSettingsStore()
  const { workspaces } = useWorkspaceStore()

  console.log('[App] Current workspaces:', Object.keys(workspaces).length)

  // One-time migration: clear localStorage since daemon is now source of truth
  if (typeof localStorage !== 'undefined') {
    localStorage.removeItem('treeterm-workspaces')
  }

  // Use ref to access latest workspaces in the close confirm callback
  const workspacesRef = useRef(workspaces)
  workspacesRef.current = workspaces

  useEffect(() => {
    loadSettings()

    // Fetch and store this window's UUID for session sync deduplication
    window.electron.getWindowUuid().then((uuid) => {
      if (uuid) {
        useWorkspaceStore.setState({ windowUuid: uuid })
        console.log('[App] Window UUID:', uuid)
      }
    }).catch((error) => {
      console.error('[App] Failed to fetch window UUID:', error)
    })

    const unsubSettings = window.electron.settings.onOpen(() => {
      setIsSettingsOpen(true)
    })

    const unsubClose = window.electron.app.onCloseConfirm(() => {
      const unmerged = getUnmergedSubWorkspaces(workspacesRef.current)
      if (unmerged.length > 0) {
        setUnmergedWorkspaces(unmerged)
        setShowCloseConfirm(true)
      } else {
        window.electron.app.confirmClose()
      }
    })

    // Listen for app ready with the default session from daemon
    const unsubReady = window.electron.app.onReady((session) => {
      console.log('[App] Received app:ready with session:', session?.id)
      if (session) {
        // Set the sessionId in the workspace store
        useWorkspaceStore.setState({ sessionId: session.id })

        // If the session has workspaces, restore them
        if (session.workspaces && session.workspaces.length > 0) {
          handleSessionRestore(session)
        }
      }
    })

    // Listen for session sync events from other windows sharing the same session
    const unsubSync = window.electron.session.onSync((session) => {
      console.log('[App] Received session:sync with', session.workspaces.length, 'workspaces')
      handleExternalSessionUpdate(session)
    })

    // Listen for daemon disconnection to show a warning banner
    const unsubDisconnect = window.electron.daemon.onDisconnected(() => {
      console.error('[App] Daemon disconnected')
      setDaemonDisconnected(true)
    })

    return () => {
      unsubSettings()
      unsubClose()
      unsubReady()
      unsubSync()
      unsubDisconnect()
    }
  }, [loadSettings])

  // Handle initial workspace from CLI
  useEffect(() => {
    const handleInitialWorkspace = async () => {
      const initialPath = await window.electron.getInitialWorkspace()
      if (!initialPath) return

      const { workspaces, addWorkspace, setActiveWorkspace } = useWorkspaceStore.getState()

      // Check if workspace already exists for this path
      const existingWorkspace = Object.values(workspaces).find(
        (ws) => ws.path === initialPath
      )

      if (existingWorkspace) {
        setActiveWorkspace(existingWorkspace.id)
      } else {
        await addWorkspace(initialPath)
      }
    }

    handleInitialWorkspace()
  }, [])

  // Session is now received via onReady callback from main process
  // The daemon always provides a default session, so no need to list sessions here

  const handleSessionRestore = async (daemonSession: Session) => {
    console.log('[App] Restoring session', daemonSession.id, 'with', daemonSession.workspaces.length, 'workspaces')

    // IMPORTANT: Set the sessionId and isRestoring flag FIRST
    // This prevents creating a new session and prevents intermediate syncs during restoration
    useWorkspaceStore.setState({ sessionId: daemonSession.id, isRestoring: true })
    console.log('[App] Set sessionId to', daemonSession.id, 'and isRestoring to true')

    const { workspaces, addWorkspace, addTabWithState, setActiveWorkspace, setActiveTab } = useWorkspaceStore.getState()

    // Get list of active PTY sessions (once for all workspaces)
    const sessions = await window.electron.terminal.list()
    const sessionMap = new Map(sessions.map(s => [s.id, s]))

    // Build a mapping from path to workspaceId
    const pathToIdMap = new Map<string, string>()

    // First pass: Restore root workspaces (those without parents)
    const rootWorkspaces = daemonSession.workspaces.filter(w => !w.parentId)
    const childWorkspaces = daemonSession.workspaces.filter(w => w.parentId)

    console.log('[App] Restoring', rootWorkspaces.length, 'root workspaces and', childWorkspaces.length, 'child workspaces')

    // Restore root workspaces first
    for (const daemonWorkspace of rootWorkspaces) {
      const existingWorkspace = Object.values(workspaces).find(
        (ws) => ws.path === daemonWorkspace.path
      )

      let workspaceId: string | null = null
      if (existingWorkspace) {
        workspaceId = existingWorkspace.id
        setActiveWorkspace(workspaceId)
      } else {
        workspaceId = await addWorkspace(daemonWorkspace.path, { skipDefaultTabs: true })
      }

      if (workspaceId) {
        pathToIdMap.set(daemonWorkspace.path, workspaceId)
        await restoreWorkspaceTabs(workspaceId, daemonWorkspace, sessionMap, addTabWithState, setActiveTab)
      }
    }

    // Second pass: Restore child workspaces using the restored workspace store
    const store = useWorkspaceStore.getState()
    for (const daemonWorkspace of childWorkspaces) {
      // Use parentId directly from the daemon workspace
      const parentId = daemonWorkspace.parentId

      if (!parentId) {
        console.warn('[App] Child workspace missing parentId:', daemonWorkspace.path)
        // Create as root if no parentId
        const workspaceId = await addWorkspace(daemonWorkspace.path, { skipDefaultTabs: true })
        if (workspaceId) {
          pathToIdMap.set(daemonWorkspace.path, workspaceId)
          await restoreWorkspaceTabs(workspaceId, daemonWorkspace, sessionMap, addTabWithState, setActiveTab)
        }
        continue
      }

      // Check if already exists
      const existingWorkspace = Object.values(store.workspaces).find(
        (ws) => ws.path === daemonWorkspace.path
      )

      if (existingWorkspace) {
        pathToIdMap.set(daemonWorkspace.path, existingWorkspace.id)
        await restoreWorkspaceTabs(existingWorkspace.id, daemonWorkspace, sessionMap, addTabWithState, setActiveTab)
      } else {
        // Directly reconstruct child workspace with parent relationship
        const workspaceId = await reconstructChildWorkspace(daemonWorkspace, parentId, addTabWithState, setActiveTab, sessionMap)
        if (workspaceId) {
          pathToIdMap.set(daemonWorkspace.path, workspaceId)
        }
      }
    }

    // Re-enable syncing and do a final sync to ensure everything is saved
    useWorkspaceStore.setState({ isRestoring: false })
    console.log('[App] Set isRestoring to false, performing final sync')

    // Force a final sync with the complete restored state
    const finalState = useWorkspaceStore.getState()
    console.log('[App] Session restore complete, final workspace count:', Object.keys(finalState.workspaces).length)

    // Do a final sync to save the complete restored state
    await finalState.syncToDaemon()
    console.log('[App] Final sync complete')

    setShowWorkspacePicker(false)
  }

  // Handle session updates pushed from other windows (via daemon broadcast)
  const handleExternalSessionUpdate = async (daemonSession: Session) => {
    // Set isRestoring to prevent this window from syncing back (would create a loop)
    useWorkspaceStore.setState({ isRestoring: true })

    const currentState = useWorkspaceStore.getState()
    const currentPaths = new Set(Object.values(currentState.workspaces).map(ws => ws.path))
    const incomingPaths = new Set(daemonSession.workspaces.map(ws => ws.path))

    // Add workspaces that exist in the session but not locally
    const sessions = await window.electron.terminal.list()
    const sessionMap = new Map(sessions.map(s => [s.id, s]))
    const pathToIdMap = new Map<string, string>()

    // First pass: roots
    const rootWorkspaces = daemonSession.workspaces.filter(w => !w.parentId)
    const childWorkspaces = daemonSession.workspaces.filter(w => w.parentId)

    const { addWorkspace, addTabWithState, setActiveTab } = useWorkspaceStore.getState()

    for (const daemonWorkspace of rootWorkspaces) {
      const existing = Object.values(currentState.workspaces).find(ws => ws.path === daemonWorkspace.path)
      if (!existing) {
        const workspaceId = await addWorkspace(daemonWorkspace.path, { skipDefaultTabs: true })
        if (workspaceId) {
          pathToIdMap.set(daemonWorkspace.path, workspaceId)
          await restoreWorkspaceTabs(workspaceId, daemonWorkspace, sessionMap, addTabWithState, setActiveTab)
        }
      } else {
        pathToIdMap.set(daemonWorkspace.path, existing.id)
      }
    }

    for (const daemonWorkspace of childWorkspaces) {
      const existing = Object.values(useWorkspaceStore.getState().workspaces).find(ws => ws.path === daemonWorkspace.path)
      if (!existing && daemonWorkspace.parentId) {
        const parentId = daemonWorkspace.parentId
        const workspaceId = await reconstructChildWorkspace(daemonWorkspace, parentId, addTabWithState, setActiveTab, sessionMap)
        if (workspaceId) {
          pathToIdMap.set(daemonWorkspace.path, workspaceId)
        }
      } else if (existing) {
        pathToIdMap.set(daemonWorkspace.path, existing.id)
      }
    }

    // Remove workspaces that no longer exist in the session
    const updatedState = useWorkspaceStore.getState()
    for (const [id, ws] of Object.entries(updatedState.workspaces)) {
      if (!incomingPaths.has(ws.path)) {
        // Remove workspace from store state directly (no sync back)
        useWorkspaceStore.setState((state) => {
          const newWorkspaces = { ...state.workspaces }
          delete newWorkspaces[id]
          // Remove from parent's children list too
          if (ws.parentId && newWorkspaces[ws.parentId]) {
            newWorkspaces[ws.parentId] = {
              ...newWorkspaces[ws.parentId],
              children: newWorkspaces[ws.parentId].children.filter(c => c !== id)
            }
          }
          return { workspaces: newWorkspaces }
        })
      }
    }

    // Re-enable syncing (but don't sync — the daemon already has the correct state)
    useWorkspaceStore.setState({ isRestoring: false })
    console.log('[App] External session update applied')
  }

  // Helper function to restore workspace tabs
  const restoreWorkspaceTabs = async (
    workspaceId: string,
    daemonWorkspace: Workspace,
    sessionMap: SessionMap,
    addTabWithState: AddTabWithStateFn,
    setActiveTab: SetActiveTabFn
  ) => {
    for (const daemonTab of daemonWorkspace.tabs) {
      if (daemonTab.applicationId === 'terminal' || daemonTab.applicationId === 'ai-harness') {
        const terminalState = daemonTab.state as TerminalState
        const ptyId = terminalState?.ptyId

        if (ptyId && sessionMap.has(ptyId)) {
          addTabWithState<TerminalState>(workspaceId, daemonTab.applicationId, { ...(daemonTab.state as Record<string, unknown>), ptyId }, daemonTab.id)
        } else {
          addTabWithState<TerminalState>(workspaceId, daemonTab.applicationId, { ...(daemonTab.state as Record<string, unknown>), ptyId: null }, daemonTab.id)
        }
      } else {
        addTabWithState(workspaceId, daemonTab.applicationId, daemonTab.state as Record<string, unknown>, daemonTab.id)
      }
    }

    if (daemonWorkspace.activeTabId) {
      setActiveTab(workspaceId, daemonWorkspace.activeTabId)
    }
  }

  // Helper function to reconstruct child workspace with parent link
  const reconstructChildWorkspace = async (
    daemonWorkspace: Workspace,
    parentId: string,
    addTabWithState: AddTabWithStateFn,
    setActiveTab: SetActiveTabFn,
    sessionMap: SessionMap
  ): Promise<string | null> => {
    const { workspaces } = useWorkspaceStore.getState()
    const parent = workspaces[parentId]

    if (!parent) {
      console.error('[App] Parent workspace not found:', parentId)
      return null
    }

    // Use the daemon's workspace ID directly for stable cross-session identity
    const id = daemonWorkspace.id

    // Create tabs - clean up stale pty IDs
    const tabs: Tab[] = []
    for (const daemonTab of daemonWorkspace.tabs) {
      if (daemonTab.applicationId === 'terminal' || daemonTab.applicationId === 'ai-harness') {
        const terminalState = daemonTab.state as TerminalState
        const ptyId = terminalState?.ptyId

        tabs.push({
          id: daemonTab.id,
          applicationId: daemonTab.applicationId,
          title: daemonTab.title,
          state: { ...(daemonTab.state as Record<string, unknown>), ptyId: ptyId && sessionMap.has(ptyId) ? ptyId : null }
        })
      } else {
        tabs.push({
          id: daemonTab.id,
          applicationId: daemonTab.applicationId,
          title: daemonTab.title,
          state: daemonTab.state
        })
      }
    }

    // Build the workspace object using the daemon workspace as the source of truth
    const workspace: Workspace = {
      ...daemonWorkspace,
      id,
      parentId,
      children: [],
      tabs,
      activeTabId: daemonWorkspace.activeTabId || (tabs.length > 0 ? tabs[0].id : null)
    }

    // Directly update the store state to add this workspace and link it to parent
    useWorkspaceStore.setState((state) => ({
      workspaces: {
        ...state.workspaces,
        [id]: workspace,
        [parentId]: {
          ...state.workspaces[parentId],
          children: [...state.workspaces[parentId].children, id]
        }
      },
      activeWorkspaceId: id
    }))

    console.log('[App] Reconstructed child workspace:', daemonWorkspace.name, 'under parent:', parent.name)
    return id
  }

  const handleCreateNewFromPicker = () => {
    setShowWorkspacePicker(false)
    // Normal startup - workspace will be created via folder selection or CLI
  }

  const handleOpenInNewWindow = async (session: Session) => {
    try {
      const result = await window.electron.session.openInNewWindow(session.id)
      if (result.success) {
        setShowWorkspacePicker(false)
      } else {
        console.error('Failed to open session in new window:', result.error)
      }
    } catch (error) {
      console.error('Failed to open session in new window:', error)
      alert(`Failed to open session: ${error instanceof Error ? error.message : 'Unknown error'}`)
    }
  }

  // Handle workspace menu commands
  useEffect(() => {
    const unsubNewTerminal = window.electron.terminal.onNewTerminal(() => {
      const { activeWorkspaceId, addTab } = useWorkspaceStore.getState()
      if (activeWorkspaceId) {
        addTab(activeWorkspaceId, 'terminal')
      }
    })

    const unsubShowSessions = window.electron.session.onShowSessions(async () => {
      try {
        const result = await window.electron.session.list()
        if (result.success && result.sessions && result.sessions.length > 0) {
          setDaemonSessions(result.sessions)
          setShowWorkspacePicker(true)
        }
      } catch (error) {
        console.error('Failed to list daemon sessions:', error)
        alert(`Failed to list sessions: ${error instanceof Error ? error.message : 'Unknown error'}`)
      }
    })

    return () => {
      unsubNewTerminal()
      unsubShowSessions()
    }
  }, [])

  const handleConfirmClose = () => {
    setShowCloseConfirm(false)
    window.electron.app.confirmClose()
  }

  const handleCancelClose = () => {
    setShowCloseConfirm(false)
    window.electron.app.cancelClose()
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
          <TreePane />
        </div>
        <div
          className={`divider ${isResizing ? 'active' : ''}`}
          onMouseDown={handleMouseDown}
        />
        <div className="workspace-pane">
          <WorkspacePane />
        </div>
        <SettingsDialog isOpen={isSettingsOpen} onClose={() => setIsSettingsOpen(false)} />
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
            onCancel={() => setShowWorkspacePicker(false)}
          />
        )}
      </div>
    </ErrorBoundary>
  )
}
