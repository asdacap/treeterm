import { useMemo } from 'react'
import { useStore } from 'zustand'
import type { StoreApi } from 'zustand'
import type { WorkspaceState, WorkspaceHandle } from '../store/createWorkspaceStore'
import type { ReviewComment, Workspace } from '../types'

/**
 * React hook that returns a WorkspaceHandle with bound methods and reactive data.
 * Components re-render when the workspace data changes.
 */
export function useWorkspace(
  workspaceStore: StoreApi<WorkspaceState>,
  workspaceId: string
): WorkspaceHandle {
  const workspace = useStore(workspaceStore, s => s.workspaces[workspaceId])

  return useMemo((): WorkspaceHandle => ({
    get id() { return workspaceId },
    get data() { return workspaceStore.getState().workspaces[workspaceId] },
    addTab: <T,>(applicationId: string, initialState?: Partial<T>) =>
      workspaceStore.getState().addTab(workspaceId, applicationId, initialState),
    removeTab: (tabId: string) =>
      workspaceStore.getState().removeTab(workspaceId, tabId),
    setActiveTab: (tabId: string) =>
      workspaceStore.getState().setActiveTab(workspaceId, tabId),
    updateTabTitle: (tabId: string, title: string) =>
      workspaceStore.getState().updateTabTitle(workspaceId, tabId, title),
    updateTabState: <T,>(tabId: string, updater: (state: T) => T) =>
      workspaceStore.getState().updateTabState(workspaceId, tabId, updater),
    getReviewComments: () =>
      workspaceStore.getState().getReviewComments(workspaceId),
    addReviewComment: (comment: Omit<ReviewComment, 'id' | 'createdAt'>) =>
      workspaceStore.getState().addReviewComment(workspaceId, comment),
    deleteReviewComment: (commentId: string) =>
      workspaceStore.getState().deleteReviewComment(workspaceId, commentId),
    toggleReviewCommentAddressed: (commentId: string) =>
      workspaceStore.getState().toggleReviewCommentAddressed(workspaceId, commentId),
    updateOutdatedReviewComments: (currentCommitHash: string) =>
      workspaceStore.getState().updateOutdatedReviewComments(workspaceId, currentCommitHash),
    clearReviewComments: () =>
      workspaceStore.getState().clearReviewComments(workspaceId),
    promptHarness: (text: string) =>
      workspaceStore.getState().promptHarness(workspaceId, text),
    quickForkWorkspace: () =>
      workspaceStore.getState().quickForkWorkspace(workspaceId),
    updateMetadata: (key: string, value: string) =>
      workspaceStore.getState().updateWorkspaceMetadata(workspaceId, key, value),
    updateStatus: (status: Workspace['status']) =>
      workspaceStore.getState().updateWorkspaceStatus(workspaceId, status),
    refreshGitInfo: () =>
      workspaceStore.getState().refreshGitInfo(workspaceId),
    mergeAndRemove: (squash: boolean) =>
      workspaceStore.getState().mergeAndRemoveWorkspace(workspaceId, squash),
    closeAndClean: () =>
      workspaceStore.getState().closeAndCleanWorkspace(workspaceId),
    lookupWorkspace: (otherId: string) =>
      workspaceStore.getState().workspaces[otherId],
    remove: () =>
      workspaceStore.getState().removeWorkspace(workspaceId),
    removeKeepBranch: () =>
      workspaceStore.getState().removeWorkspaceKeepBranch(workspaceId),
    removeKeepWorktree: () =>
      workspaceStore.getState().removeWorkspaceKeepWorktree(workspaceId),
    removeKeepBoth: () =>
      workspaceStore.getState().removeWorkspaceKeepBoth(workspaceId),
  // workspace in deps ensures re-creation when workspace data changes (for reactive `data`)
  }), [workspaceStore, workspaceId, workspace])
}
