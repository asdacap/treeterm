import { useState, useCallback } from 'react'
import { useStore } from 'zustand'
import type { StoreApi } from 'zustand'
import type { WorkspaceState } from '../store/createWorkspaceStore'
import { FileTree } from './FileTree'
import { FileViewer } from './FileViewer'
import type { FilesystemState } from '../types'

interface FilesystemBrowserProps {
  workspacePath: string
  workspaceId: string
  tabId: string
  workspaceStore: StoreApi<WorkspaceState>
}

export function FilesystemBrowser({
  workspacePath,
  workspaceId,
  tabId,
  workspaceStore
}: FilesystemBrowserProps): JSX.Element {
  const { workspaces, updateTabState } = useStore(workspaceStore)
  const workspace = workspaces[workspaceId]
  const tab = workspace?.tabs.find((t) => t.id === tabId)
  const state = tab?.state as FilesystemState | undefined

  // Resize state
  const [treeWidth, setTreeWidth] = useState(250)
  const [isResizing, setIsResizing] = useState(false)

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

  // Resize handlers
  const handleMouseDown = useCallback(() => {
    setIsResizing(true)
  }, [])

  const handleMouseMove = useCallback(
    (e: React.MouseEvent) => {
      if (!isResizing) return
      const container = e.currentTarget as HTMLElement
      const rect = container.getBoundingClientRect()
      const newWidth = Math.max(150, Math.min(500, e.clientX - rect.left))
      setTreeWidth(newWidth)
    },
    [isResizing]
  )

  const handleMouseUp = useCallback(() => {
    setIsResizing(false)
  }, [])

  return (
    <div className="filesystem-browser" onMouseMove={handleMouseMove} onMouseUp={handleMouseUp} onMouseLeave={handleMouseUp}>
      <div style={{ width: treeWidth }}>
        <FileTree
          workspacePath={workspacePath}
          selectedPath={state.selectedPath}
          expandedDirs={state.expandedDirs}
          onSelectFile={setSelectedPath}
          onToggleDir={toggleExpandedDir}
        />
      </div>
      <div className={`divider ${isResizing ? 'active' : ''}`} onMouseDown={handleMouseDown} />
      <FileViewer workspacePath={workspacePath} workspaceId={workspaceId} filePath={state.selectedPath} workspaceStore={workspaceStore} />
    </div>
  )
}
