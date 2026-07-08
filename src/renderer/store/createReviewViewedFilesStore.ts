import { createStore } from 'zustand/vanilla'
import type { StoreApi } from 'zustand'
import type { ViewedFileStats } from '../types'

export interface ReviewViewedFilesDeps {
  getMetadata: () => Record<string, string>
  updateMetadata: (key: string, value: string, reason: string) => void
}

/** The subset of DiffFile / UncommittedFile this store needs to key and invalidate entries. */
export interface ViewedFileEntry {
  path: string
  additions: number
  deletions: number
}

export interface ReviewViewedFilesState {
  getViewedFiles: () => Record<string, ViewedFileStats>
  toggleViewedFile: (file: ViewedFileEntry) => void
  markFilesViewed: (files: ViewedFileEntry[]) => void
  /**
   * Drop entries whose diff stats no longer match (the file changed since it was
   * marked viewed). Entries absent from `files` are preserved — they belong to a
   * different view mode (committed / uncommitted / per-commit).
   */
  reconcileViewedFiles: (files: ViewedFileEntry[]) => void
}

export type ReviewViewedFilesStore = StoreApi<ReviewViewedFilesState>

export const REVIEW_VIEWED_FILES_KEY = 'reviewViewedFiles'

export function parseViewedFiles(metadata: Record<string, string>): Record<string, ViewedFileStats> {
  if (!metadata[REVIEW_VIEWED_FILES_KEY]) return {}
  try {
    return JSON.parse(metadata[REVIEW_VIEWED_FILES_KEY]) as Record<string, ViewedFileStats>
  } catch {
    return {}
  }
}

function serializeViewedFiles(viewed: Record<string, ViewedFileStats>): string {
  return JSON.stringify(viewed)
}

export function createReviewViewedFilesStore(deps: ReviewViewedFilesDeps): ReviewViewedFilesStore {
  const persist = (viewed: Record<string, ViewedFileStats>, reason: string): void => {
    deps.updateMetadata(REVIEW_VIEWED_FILES_KEY, serializeViewedFiles(viewed), reason)
  }

  return createStore<ReviewViewedFilesState>()(() => ({
    getViewedFiles: (): Record<string, ViewedFileStats> => parseViewedFiles(deps.getMetadata()),

    toggleViewedFile: (file: ViewedFileEntry): void => {
      const viewed = parseViewedFiles(deps.getMetadata())
      if (viewed[file.path]) {
        // eslint-disable-next-line @typescript-eslint/no-unused-vars
        const { [file.path]: _removed, ...rest } = viewed
        persist(rest, 'toggleViewedFile')
        return
      }
      persist(
        { ...viewed, [file.path]: { additions: file.additions, deletions: file.deletions } },
        'toggleViewedFile'
      )
    },

    markFilesViewed: (files: ViewedFileEntry[]): void => {
      const viewed = parseViewedFiles(deps.getMetadata())
      const next = { ...viewed }
      let changed = false
      for (const file of files) {
        if (!next[file.path]) {
          next[file.path] = { additions: file.additions, deletions: file.deletions }
          changed = true
        }
      }
      if (!changed) return
      persist(next, 'markFilesViewed')
    },

    reconcileViewedFiles: (files: ViewedFileEntry[]): void => {
      const viewed = parseViewedFiles(deps.getMetadata())
      const fileMap = new Map(files.map(f => [f.path, f]))
      const next: Record<string, ViewedFileStats> = {}
      let changed = false
      for (const [path, stats] of Object.entries(viewed)) {
        const file = fileMap.get(path)
        if (!file) {
          next[path] = stats
          continue
        }
        if (file.additions === stats.additions && file.deletions === stats.deletions) {
          next[path] = stats
        } else {
          changed = true
        }
      }
      if (!changed) return
      persist(next, 'reconcileViewedFiles')
    },
  }))
}
