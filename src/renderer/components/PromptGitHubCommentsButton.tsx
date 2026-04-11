import React from 'react'
import { useStore } from 'zustand'
import type { WorkspaceStore } from '../types'

export function PromptGitHubCommentsButton({ workspace }: { workspace: WorkspaceStore }): React.JSX.Element | null {
  const gitController = useStore(workspace, s => s.gitController)
  const promptHarness = useStore(workspace, s => s.promptHarness)
  const prInfo = useStore(gitController, s => s.prInfo)

  if (!prInfo || prInfo.unresolvedThreads.length === 0) return null

  return (
    <button
      className="review-comments-button"
      onClick={() => { void promptHarness('Pull Github comment and address') }}
      title="Prompt AI to pull and address GitHub PR comments"
    >
      Address PR Comments ({String(prInfo.unresolvedCount)})
    </button>
  )
}
