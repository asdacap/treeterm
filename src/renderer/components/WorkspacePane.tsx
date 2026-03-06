import { useWorkspaceStore } from '../store/workspace'
import Terminal from './Terminal'

export default function WorkspacePane() {
  const { workspaces, activeWorkspaceId } = useWorkspaceStore()
  const activeWorkspace = activeWorkspaceId ? workspaces[activeWorkspaceId] : null

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

  return (
    <div className="workspace-content">
      <div className="workspace-header">
        <span className="workspace-title">{activeWorkspace.name}</span>
        <span className="workspace-path">{activeWorkspace.path}</span>
      </div>
      <div className="workspace-terminal">
        <Terminal
          key={activeWorkspace.id}
          cwd={activeWorkspace.path}
          workspaceId={activeWorkspace.id}
        />
      </div>
    </div>
  )
}
