import { useState, useEffect } from 'react'
import type { ReviewsData } from '../types'
import { useReviewsApi } from '../contexts/ReviewsApiContext'
import { useTerminalApi } from '../contexts/TerminalApiContext'

interface ReviewCommentsButtonProps {
  workspacePath: string
  ptyId: string | undefined
}

export function ReviewCommentsButton({ workspacePath, ptyId }: ReviewCommentsButtonProps): JSX.Element | null {
  const reviews = useReviewsApi()
  const terminal = useTerminalApi()
  const [hasComments, setHasComments] = useState(false)

  useEffect(() => {
    checkForComments()
  }, [workspacePath])

  const checkForComments = async () => {
    try {
      const result = await reviews.load(workspacePath)
      if (result.success && result.reviews) {
        const reviews = result.reviews as ReviewsData
        setHasComments(reviews.comments.length > 0)
      } else {
        setHasComments(false)
      }
    } catch (error) {
      console.warn('[ReviewCommentsButton] failed to check for comments:', error)
      setHasComments(false)
    }
  }

  const handleClick = async () => {
    if (!ptyId) return

    // Submit command to Claude terminal
    const command = 'read .treeterm/reviews.json and address the comments\r'
    terminal.write(ptyId, command)
  }

  if (!hasComments) {
    return null
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
