import React from 'react'
import { useStore } from 'zustand'
import type { WorkspaceStore } from '../types'
import { generateReviewPrompt } from '../utils/reviewPrompt'

interface ReviewCommentsButtonProps {
  workspace: WorkspaceStore
}

export function ReviewCommentsButton({ workspace }: ReviewCommentsButtonProps): React.JSX.Element | null {
  const reviewCommentStore = useStore(workspace, s => s.reviewComments)
  const promptHarness = useStore(workspace, s => s.promptHarness)
  const getReviewComments = useStore(reviewCommentStore, s => s.getReviewComments)
  const markAllReviewCommentsAddressed = useStore(reviewCommentStore, s => s.markAllReviewCommentsAddressed)
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
