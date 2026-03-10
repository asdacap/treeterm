import { useState, useCallback, useEffect, useRef } from 'react'
import TreePane from './components/TreePane'
import WorkspacePane from './components/WorkspacePane'
import SettingsDialog from './components/SettingsDialog'
import CloseConfirmDialog from './components/CloseConfirmDialog'
import WorkspacePickerDialog from './components/WorkspacePickerDialog'
import { useSettingsStore } from './store/settings'
import { useWorkspaceStore, getUnmergedSubWorkspaces } from './store/workspace'
import type { Workspace, DaemonWorkspace, DaemonSession, TerminalState } from './types'

export default function App() {
  console.log('[App] Component rendering')
  const [treeWidth, setTreeWidth] = useState(250)
  const [isResizing, setIsResizing] = useState(false)
  const [isSettingsOpen, setIsSettingsOpen] = useState(false)
  const [showCloseConfirm, setShowCloseConfirm] = useState(false)
  const [unmergedWorkspaces, setUnmergedWorkspaces] = useState<Workspace[]>([])
  const [daemonSessions, setDaemonSessions] = useState<DaemonSession[]>([])
  const [showWorkspacePicker, setShowWorkspacePicker] = useState(false)
  const { loadSettings } = useSettingsStore()
  const { workspaces } = useWorkspaceStore()

  console.log('[App] Current workspaces:', Object.keys(workspaces).length)

  // Use ref to access latest workspaces in the close confirm callback
  const workspacesRef = useRef(workspaces)
  workspacesRef.current = workspaces

  useEffect(() => {
    loadSettings()
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

    return () => {
      unsubSettings()
      unsubClose()
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

  // Handle daemon session restoration
  useEffect(() => {
    const loadDaemonSessions = async () => {
      try {
        console.log('[App] Loading daemon sessions...')
        // Request sessions from daemon when component mounts (after listeners ready)
        const result = await window.electron.session.list()

        console.log('[App] Session list result:', result)

        if (!result.success || !result.sessions || result.sessions.length === 0) {
          // No sessions, normal startup
          console.log('[App] No sessions found, normal startup')
          return
        }

        console.log('[App] Found', result.sessions.length, 'session(s)')

        if (result.sessions.length === 1) {
          // Auto-restore single session
          console.log('[App] Auto-restoring single session')
          handleSessionRestore(result.sessions[0])
        } else {
          // Multiple sessions - show picker
          console.log('[App] Showing session picker')
          setDaemonSessions(result.sessions)
          setShowWorkspacePicker(true)
        }
      } catch (error) {
        console.error('[App] Failed to list daemon sessions:', error)
      }
    }

    console.log('[App] useEffect triggered - loading sessions')
    loadDaemonSessions()
  }, [])

  const handleSessionRestore = async (daemonSession: DaemonSession) => {
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
    const rootWorkspaces = daemonSession.workspaces.filter(w => w.parentPath === null)
    const childWorkspaces = daemonSession.workspaces.filter(w => w.parentPath !== null)

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
      // Find parent workspace by path
      const parentId = daemonWorkspace.parentPath ? pathToIdMap.get(daemonWorkspace.parentPath) : null

      if (!parentId) {
        console.warn('[App] Parent not found for child workspace:', daemonWorkspace.path, 'parent:', daemonWorkspace.parentPath)
        // Create as root if parent not found
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

  // Helper function to restore workspace tabs
  const restoreWorkspaceTabs = async (
    workspaceId: string,
    daemonWorkspace: DaemonWorkspace,
    sessionMap: Map<string, any>,
    addTabWithState: any,
    setActiveTab: any
  ) => {
    for (const daemonTab of daemonWorkspace.tabs) {
      if (daemonTab.applicationId === 'terminal') {
        const terminalState = daemonTab.state as TerminalState
        const ptyId = terminalState?.ptyId

        if (ptyId && sessionMap.has(ptyId)) {
          addTabWithState<TerminalState>(workspaceId, 'terminal', { ptyId })
        } else {
          addTabWithState<TerminalState>(workspaceId, 'terminal', { ptyId: null })
        }
      } else {
        addTabWithState(workspaceId, daemonTab.applicationId, daemonTab.state)
      }
    }

    if (daemonWorkspace.activeTabId) {
      setActiveTab(workspaceId, daemonWorkspace.activeTabId)
    }
  }

  // Helper function to reconstruct child workspace with parent link
  const reconstructChildWorkspace = async (
    daemonWorkspace: DaemonWorkspace,
    parentId: string,
    addTabWithState: any,
    setActiveTab: any,
    sessionMap: Map<string, any>
  ): Promise<string | null> => {
    const { workspaces } = useWorkspaceStore.getState()
    const parent = workspaces[parentId]

    if (!parent) {
      console.error('[App] Parent workspace not found:', parentId)
      return null
    }

    // Generate a new workspace ID
    const id = `ws-${Date.now()}-${Math.random().toString(36).slice(2, 9)}`

    // Create tabs
    const tabs: any[] = []
    for (const daemonTab of daemonWorkspace.tabs) {
      const tabId = `tab-${Date.now()}-${Math.random().toString(36).slice(2, 9)}`

      if (daemonTab.applicationId === 'terminal') {
        const terminalState = daemonTab.state as TerminalState
        const ptyId = terminalState?.ptyId

        tabs.push({
          id: tabId,
          applicationId: 'terminal',
          title: daemonTab.title,
          state: { ptyId: ptyId && sessionMap.has(ptyId) ? ptyId : null }
        })
      } else {
        tabs.push({
          id: tabId,
          applicationId: daemonTab.applicationId,
          title: daemonTab.title,
          state: daemonTab.state
        })
      }
    }

    // Build the workspace object
    const workspace = {
      id,
      name: daemonWorkspace.name,
      path: daemonWorkspace.path,
      parentId: parentId,
      children: [],
      status: daemonWorkspace.status,
      isGitRepo: daemonWorkspace.isGitRepo,
      gitBranch: daemonWorkspace.gitBranch,
      gitRootPath: daemonWorkspace.gitRootPath,
      isWorktree: daemonWorkspace.isWorktree,
      isDetached: daemonWorkspace.isDetached,
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
    <div
      className="app"
      onMouseMove={handleMouseMove}
      onMouseUp={handleMouseUp}
      onMouseLeave={handleMouseUp}
    >
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
          onCreateNew={handleCreateNewFromPicker}
          onCancel={() => setShowWorkspacePicker(false)}
        />
      )}
    </div>
  )
}
