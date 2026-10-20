import React, { useState } from 'react'
import { useStore } from 'zustand'
import type { WorkspaceStore } from '../types'

/** Shown in a fork with no diff from its parent: addresses the parent's review comments here. */
export function ParentReviewCommentsButton({ workspace }: { workspace: WorkspaceStore }): React.JSX.Element | null {
  const gitController = useStore(workspace, s => s.gitController)
  const isDiffCleanFromParent = useStore(gitController, s => s.isDiffCleanFromParent)
  const parentId = useStore(workspace, s => s.workspace.parentId)
  const lookupWorkspaceStore = useStore(workspace, s => s.lookupWorkspaceStore)

  if (!isDiffCleanFromParent || !parentId) return null
  const parent = lookupWorkspaceStore(parentId)
  if (!parent) return null

  return <ParentReviewCommentsButtonInner key={parentId} workspace={workspace} parent={parent} />
}

function ParentReviewCommentsButtonInner({ workspace, parent }: { workspace: WorkspaceStore; parent: WorkspaceStore }): React.JSX.Element | null {
  const promptParentReviewComments = useStore(workspace, s => s.promptParentReviewComments)
  // Subscribe to the parent's metadata so comment changes there re-render this button.
  useStore(parent, s => s.metadata)
  const reviewCommentStore = useStore(parent, s => s.reviewComments)
  const getReviewComments = useStore(reviewCommentStore, s => s.getReviewComments)
  const [loading, setLoading] = useState(false)

  const unaddressedCount = getReviewComments().filter(c => !c.addressed).length
  if (unaddressedCount === 0) return null

  const handleClick = async () => {
    setLoading(true)
    try {
      await promptParentReviewComments()
    } finally {
      setLoading(false)
    }
  }

  return (
    <button
      className="review-comments-button"
      onClick={() => { void handleClick() }}
      disabled={loading}
      title="Prompt AI to address the parent workspace's review comments"
    >
      Address Parent Comments ({String(unaddressedCount)})
    </button>
  )
}
