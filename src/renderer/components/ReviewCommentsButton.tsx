import { useStore } from 'zustand'
import type { StoreApi } from 'zustand'
import type { WorkspaceState } from '../store/createWorkspaceStore'
import { useTerminalApi } from '../contexts/TerminalApiContext'
import { generateReviewPrompt } from '../utils/reviewPrompt'

interface ReviewCommentsButtonProps {
  workspaceStore: StoreApi<WorkspaceState>
  workspaceId: string
  ptyId: string | undefined
}

export function ReviewCommentsButton({ workspaceStore, workspaceId, ptyId }: ReviewCommentsButtonProps): JSX.Element | null {
  const terminal = useTerminalApi()
  const getReviewComments = useStore(workspaceStore, (state) => state.getReviewComments)
  const comments = getReviewComments(workspaceId)

  if (comments.length === 0) return null

  const handleClick = () => {
    if (!ptyId) return
    const prompt = generateReviewPrompt(comments)
    if (prompt) {
      terminal.write(ptyId, prompt + '\r')
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
