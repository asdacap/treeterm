import { useWorkspaceStore } from '../store/workspace'
import { FileTree } from './FileTree'
import { FileViewer } from './FileViewer'
import type { FilesystemState } from '../types'

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
  const { workspaces, updateTabState } = useWorkspaceStore()
  const workspace = workspaces[workspaceId]
  const tab = workspace?.tabs.find((t) => t.id === tabId)
  const state = tab?.state as FilesystemState | undefined

  if (!tab || !state) {
    return <div className="filesystem-browser-error">Invalid tab</div>
  }

  const setSelectedPath = (path: string | null) => {
    updateTabState<FilesystemState>(workspaceId, tabId, (s) => ({
      ...s,
      selectedPath: path
    }))
  }

  const toggleExpandedDir = (dirPath: string) => {
    updateTabState<FilesystemState>(workspaceId, tabId, (s) => {
      const isExpanded = s.expandedDirs.includes(dirPath)
      return {
        ...s,
        expandedDirs: isExpanded
          ? s.expandedDirs.filter((d) => d !== dirPath)
          : [...s.expandedDirs, dirPath]
      }
    })
  }

  return (
    <div className="filesystem-browser">
      <FileTree
        workspacePath={workspacePath}
        selectedPath={state.selectedPath}
        expandedDirs={state.expandedDirs}
        onSelectFile={setSelectedPath}
        onToggleDir={toggleExpandedDir}
      />
      <FileViewer workspacePath={workspacePath} workspaceId={workspaceId} filePath={state.selectedPath} />
    </div>
  )
}
