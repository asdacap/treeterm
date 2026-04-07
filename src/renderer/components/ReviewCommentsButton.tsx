import React from 'react'
import { useStore } from 'zustand'
import type { WorkspaceStore } from '../types'
import { generateReviewPrompt } from '../utils/reviewPrompt'

interface ReviewCommentsButtonProps {
  workspace: WorkspaceStore
}

export function ReviewCommentsButton({ workspace }: ReviewCommentsButtonProps): React.JSX.Element | null {
  const { reviewComments: reviewCommentStore, promptHarness } = useStore(workspace)
  const { getReviewComments, markAllReviewCommentsAddressed } = useStore(reviewCommentStore)
  const comments = getReviewComments()
  const prompt = generateReviewPrompt(comments)

  if (!prompt) return null

  const handleClick = async () => {
    const sent = await promptHarness(prompt)
    if (sent) {
      markAllReviewCommentsAddressed()
    }
  }

  return (
    <button
      className="review-comments-button"
      onClick={() => { void handleClick(); }}
      title="Address review comments"
    >
      Address Comments
    </button>
  )
}
