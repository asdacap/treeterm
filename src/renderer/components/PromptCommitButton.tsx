import React from 'react'
import { useStore } from 'zustand'
import type { WorkspaceStore } from '../types'

export function PromptCommitButton({ workspace }: { workspace: WorkspaceStore }): React.JSX.Element | null {
  const gitController = useStore(workspace, s => s.gitController)
  const promptHarness = useStore(workspace, s => s.promptHarness)
  const hasUncommittedChanges = useStore(gitController, s => s.hasUncommittedChanges)
  const hasConflictsWithParent = useStore(gitController, s => s.hasConflictsWithParent)

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
