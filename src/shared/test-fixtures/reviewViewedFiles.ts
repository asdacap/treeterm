import { createStore } from 'zustand/vanilla'
import type { ReviewViewedFilesState, ReviewViewedFilesStore } from '../../renderer/store/createReviewViewedFilesStore'

/** An inert `reviewViewedFiles` store for tests that only need a WorkspaceStoreState to typecheck. */
export function createMockReviewViewedFilesStore(): ReviewViewedFilesStore {
  return createStore<ReviewViewedFilesState>()(() => ({
    getViewedFiles: () => ({}),
    toggleViewedFile: () => {},
    markFilesViewed: () => {},
    reconcileViewedFiles: () => {},
  }))
}
