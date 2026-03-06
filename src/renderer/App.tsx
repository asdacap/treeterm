import { useState, useCallback, useEffect } from 'react'
import TreePane from './components/TreePane'
import WorkspacePane from './components/WorkspacePane'
import SettingsDialog from './components/SettingsDialog'
import { useSettingsStore } from './store/settings'

export default function App() {
  const [treeWidth, setTreeWidth] = useState(250)
  const [isResizing, setIsResizing] = useState(false)
  const [isSettingsOpen, setIsSettingsOpen] = useState(false)
  const { loadSettings } = useSettingsStore()

  useEffect(() => {
    loadSettings()
    const unsubscribe = window.electron.settings.onOpen(() => {
      setIsSettingsOpen(true)
    })
    return () => unsubscribe()
  }, [loadSettings])

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
    </div>
  )
}
