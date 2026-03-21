import { useStore } from 'zustand'
import type { WorkspaceHandle } from '../types'
import { generateReviewPrompt } from '../utils/reviewPrompt'

interface ReviewCommentsButtonProps {
  workspace: WorkspaceHandle
}

export function ReviewCommentsButton({ workspace }: ReviewCommentsButtonProps): JSX.Element | null {
  const { getReviewComments, promptHarness } = useStore(workspace)
  const comments = getReviewComments()

  if (comments.length === 0) return null

  const handleClick = () => {
    const prompt = generateReviewPrompt(comments)
    if (prompt) {
      promptHarness(prompt)
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
