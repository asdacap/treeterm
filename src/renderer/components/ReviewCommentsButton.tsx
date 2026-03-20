import { useStore } from 'zustand'
import type { StoreApi } from 'zustand'
import type { WorkspaceState } from '../store/createWorkspaceStore'
import { generateReviewPrompt } from '../utils/reviewPrompt'

interface ReviewCommentsButtonProps {
  workspaceStore: StoreApi<WorkspaceState>
  workspaceId: string
}

export function ReviewCommentsButton({ workspaceStore, workspaceId }: ReviewCommentsButtonProps): JSX.Element | null {
  const getReviewComments = useStore(workspaceStore, (state) => state.getReviewComments)
  const promptHarness = useStore(workspaceStore, (state) => state.promptHarness)
  const comments = getReviewComments(workspaceId)

  if (comments.length === 0) return null

  const handleClick = () => {
    const prompt = generateReviewPrompt(comments)
    if (prompt) {
      promptHarness(workspaceId, prompt)
    }
  }

  return (
    <button
      className="review-comments-button"
      onClick={handleClick}
      title="Address review comments"
    >
      Address Comments
    </button>
  )
}
