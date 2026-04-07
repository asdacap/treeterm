import { useStore } from 'zustand'
import type { WorkspaceStore } from '../types'

export function PromptRebaseButton({ workspace }: { workspace: WorkspaceStore }): JSX.Element | null {
  const { gitController, promptHarness, workspace: wsData, lookupWorkspace } = useStore(workspace)
  const { hasConflictsWithParent } = useStore(gitController)

  const parentBranch = wsData.parentId ? lookupWorkspace(wsData.parentId)?.gitBranch : null

  if (!hasConflictsWithParent || !wsData.gitBranch || !parentBranch) return null

  return (
    <button
      className="review-comments-button"
      onClick={() => { void promptHarness(`rebase local branch ${wsData.gitBranch} onto ${parentBranch}`); }}
      title="Prompt AI to rebase onto parent branch"
    >
      Prompt Rebase
    </button>
  )
}
