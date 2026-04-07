import { create } from 'zustand'
import { persist } from 'zustand/middleware'

interface RecentDirectoriesState {
  directories: Record<string, string[]>
  addRecent: (connectionKey: string, path: string) => void
  getRecent: (connectionKey: string) => string[]
}

const MAX_RECENT = 10
const EMPTY_ARRAY: string[] = []

export const useRecentDirectoriesStore = create<RecentDirectoriesState>()(
  persist(
    (set, get) => ({
      directories: {},
      addRecent: (connectionKey: string, path: string) => {
        set((state) => {
          const existing = state.directories[connectionKey]
          // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition
          const filtered = (existing ?? []).filter(d => d !== path)
          const updated = [path, ...filtered].slice(0, MAX_RECENT)
          return { directories: { ...state.directories, [connectionKey]: updated } }
        })
      },
      getRecent: (connectionKey: string) => {
        return get().directories[connectionKey] ?? EMPTY_ARRAY
      },
    }),
    {
      name: 'treeterm-recent-directories',
    }
  )
)
