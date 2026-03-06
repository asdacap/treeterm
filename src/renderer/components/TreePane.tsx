import { useWorkspaceStore } from '../store/workspace'

export default function TreePane() {
  const { workspaces, activeWorkspaceId, addWorkspace, removeWorkspace, setActiveWorkspace } =
    useWorkspaceStore()

  const handleAddWorkspace = async () => {
    const path = await window.electron.selectFolder()
    if (path) {
      addWorkspace(path)
    }
  }

  const handleContextMenu = (e: React.MouseEvent, id: string) => {
    e.preventDefault()
    if (confirm('Remove this workspace?')) {
      removeWorkspace(id)
    }
  }

  const workspaceList = Object.values(workspaces)

  return (
    <div className="tree-pane-content">
      <div className="tree-header">
        <span className="tree-title">Workspaces</span>
        <button className="add-button" onClick={handleAddWorkspace} title="Add workspace">
          +
        </button>
      </div>
      <div className="tree-list">
        {workspaceList.length === 0 ? (
          <div className="tree-empty">No workspaces yet. Click + to add one.</div>
        ) : (
          workspaceList.map((ws) => (
            <div
              key={ws.id}
              className={`tree-item ${activeWorkspaceId === ws.id ? 'active' : ''}`}
              onClick={() => setActiveWorkspace(ws.id)}
              onContextMenu={(e) => handleContextMenu(e, ws.id)}
              title={ws.path}
            >
              <span className="tree-item-icon">📁</span>
              <span className="tree-item-name">{ws.name}</span>
            </div>
          ))
        )}
      </div>
    </div>
  )
}
