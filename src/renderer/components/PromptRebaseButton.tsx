import { useStore } from 'zustand'
import type { WorkspaceStore } from '../types'

export function PromptRebaseButton({ workspace }: { workspace: WorkspaceStore }): JSX.Element | null {
  const { gitController, promptHarness } = useStore(workspace)
  const { hasConflictsWithParent } = useStore(gitController)

  if (!hasConflictsWithParent) return null

  return (
    <button
      className="review-comments-button"
      onClick={() => promptHarness('rebase')}
      title="Prompt AI to rebase onto parent branch"
    >
      Prompt Rebase
    </button>
  )
}
