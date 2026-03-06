import { useWorkspaceStore } from '../store/workspace'
import { FileTree } from './FileTree'
import { FileViewer } from './FileViewer'
import type { FilesystemTab } from '../types'

interface FilesystemBrowserProps {
  workspacePath: string
  workspaceId: string
  tabId: string
}

export function FilesystemBrowser({
  workspacePath,
  workspaceId,
  tabId
}: FilesystemBrowserProps): JSX.Element {
  const { workspaces, setSelectedPath, toggleExpandedDir } = useWorkspaceStore()
  const workspace = workspaces[workspaceId]
  const tab = workspace?.tabs.find((t) => t.id === tabId) as FilesystemTab | undefined

  if (!tab || tab.type !== 'filesystem') {
    return <div className="filesystem-browser-error">Invalid tab</div>
  }

  return (
    <div className="filesystem-browser">
      <FileTree
        workspacePath={workspacePath}
        selectedPath={tab.selectedPath}
        expandedDirs={tab.expandedDirs}
        onSelectFile={(path) => setSelectedPath(workspaceId, tabId, path)}
        onToggleDir={(path) => toggleExpandedDir(workspaceId, tabId, path)}
      />
      <FileViewer workspacePath={workspacePath} filePath={tab.selectedPath} />
    </div>
  )
}
