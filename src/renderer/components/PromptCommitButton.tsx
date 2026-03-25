import { useStore } from 'zustand'
import type { WorkspaceStore } from '../types'

export function PromptCommitButton({ workspace }: { workspace: WorkspaceStore }): JSX.Element | null {
  const { hasUncommittedChanges, hasConflictsWithParent, promptHarness } = useStore(workspace)

  if (!hasUncommittedChanges || hasConflictsWithParent) return null

  return (
    <button
      className="review-comments-button"
      onClick={() => promptHarness('commit')}
      title="Prompt AI to commit changes"
    >
      Prompt Commit
    </button>
  )
}
