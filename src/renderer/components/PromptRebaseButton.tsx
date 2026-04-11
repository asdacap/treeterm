import React from 'react'
import { useStore } from 'zustand'
import type { WorkspaceStore } from '../types'

export function PromptRebaseButton({ workspace }: { workspace: WorkspaceStore }): React.JSX.Element | null {
  const gitController = useStore(workspace, s => s.gitController)
  const promptHarness = useStore(workspace, s => s.promptHarness)
  const wsData = useStore(workspace, s => s.workspace)
  const lookupWorkspace = useStore(workspace, s => s.lookupWorkspace)
  const hasConflictsWithParent = useStore(gitController, s => s.hasConflictsWithParent)

  const parentBranch = wsData.parentId ? lookupWorkspace(wsData.parentId)?.gitBranch : null

  if (!hasConflictsWithParent || !wsData.gitBranch || !parentBranch) return null

  return (
    <button
      className="review-comments-button"
      onClick={() => { void promptHarness(`rebase local branch ${wsData.gitBranch ?? 'unknown'} onto ${parentBranch}`); }}
      title="Prompt AI to rebase onto parent branch"
    >
      Prompt Rebase
    </button>
  )
}
