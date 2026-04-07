import React from 'react'
import { useStore } from 'zustand'
import type { WorkspaceStore } from '../types'

export function PromptCommitButton({ workspace }: { workspace: WorkspaceStore }): React.JSX.Element | null {
  const { gitController, promptHarness } = useStore(workspace)
  const { hasUncommittedChanges, hasConflictsWithParent } = useStore(gitController)

  if (!hasUncommittedChanges || hasConflictsWithParent) return null

  return (
    <button
      className="review-comments-button"
      onClick={() => { void promptHarness('commit') }}
      title="Prompt AI to commit changes"
    >
      Prompt Commit
    </button>
  )
}
