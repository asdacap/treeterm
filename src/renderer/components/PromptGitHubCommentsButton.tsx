import { useStore } from 'zustand'
import type { WorkspaceStore } from '../types'

export function PromptGitHubCommentsButton({ workspace }: { workspace: WorkspaceStore }): JSX.Element | null {
  const { gitController, promptHarness } = useStore(workspace)
  const { prInfo } = useStore(gitController)

  if (!prInfo || prInfo.unresolvedThreads.length === 0) return null

  return (
    <button
      className="review-comments-button"
      onClick={() => promptHarness('Pull Github comment and address')}
      title="Prompt AI to pull and address GitHub PR comments"
    >
      Address PR Comments ({prInfo.unresolvedCount})
    </button>
  )
}
