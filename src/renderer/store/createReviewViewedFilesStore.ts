import { createStore } from 'zustand/vanilla'
import type { StoreApi } from 'zustand'
import type { FileChangeStats, ViewedFileStats } from '../types'
import { FileStatKind } from '../types'

export interface ReviewViewedFilesDeps {
  getMetadata: () => Record<string, string>
  updateMetadata: (key: string, value: string, reason: string) => void
}

/** The subset of DiffFile / UncommittedFile this store needs to key and invalidate entries. */
export type ViewedFileEntry = { path: string } & (FileChangeStats | { additions: number; deletions: number })

/** Older metadata had text counts without a discriminator. */
function viewedStats(file: ViewedFileEntry): ViewedFileStats {
  if ('kind' in file && file.kind === FileStatKind.Binary) {
    return { kind: FileStatKind.Binary, byteChange: file.byteChange }
  }
  const text = { additions: file.additions, deletions: file.deletions }
  return 'kind' in file ? { kind: FileStatKind.Text, ...text } : text
}

function statsMatch(file: ViewedFileEntry, stats: ViewedFileStats): boolean {
  if ('kind' in file && file.kind === FileStatKind.Binary) return 'kind' in stats && stats.kind === FileStatKind.Binary && file.byteChange === stats.byteChange
  return 'additions' in file && 'additions' in stats && file.additions === stats.additions && file.deletions === stats.deletions
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
        { ...viewed, [file.path]: viewedStats(file) },
        'toggleViewedFile'
      )
    },

    markFilesViewed: (files: ViewedFileEntry[]): void => {
      const viewed = parseViewedFiles(deps.getMetadata())
      const next = { ...viewed }
      let changed = false
      for (const file of files) {
        if (!next[file.path]) {
          next[file.path] = viewedStats(file)
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
        if (statsMatch(file, stats)) {
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
