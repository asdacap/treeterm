import { useState, useCallback, useEffect, useRef } from 'react'
import TreePane from './components/TreePane'
import WorkspacePane from './components/WorkspacePane'
import SettingsDialog from './components/SettingsDialog'
import CloseConfirmDialog from './components/CloseConfirmDialog'
import { useSettingsStore } from './store/settings'
import { useWorkspaceStore, getUnmergedSubWorkspaces } from './store/workspace'
import type { Workspace } from './types'

export default function App() {
  const [treeWidth, setTreeWidth] = useState(250)
  const [isResizing, setIsResizing] = useState(false)
  const [isSettingsOpen, setIsSettingsOpen] = useState(false)
  const [showCloseConfirm, setShowCloseConfirm] = useState(false)
  const [unmergedWorkspaces, setUnmergedWorkspaces] = useState<Workspace[]>([])
  const { loadSettings } = useSettingsStore()
  const { workspaces } = useWorkspaceStore()

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
    </div>
  )
}
