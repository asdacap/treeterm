import { create } from 'zustand'
import { persist } from 'zustand/middleware'

interface RecentDirectoriesState {
  directories: Map<string, string[]>
  addRecent: (connectionKey: string, path: string) => void
  getRecent: (connectionKey: string) => string[]
}

const MAX_RECENT = 10
const EMPTY_ARRAY: string[] = []

export const useRecentDirectoriesStore = create<RecentDirectoriesState>()(
  persist(
    (set, get) => ({
      directories: new Map<string, string[]>(),
      addRecent: (connectionKey: string, path: string) => {
        set((state) => {
          const existing = state.directories.get(connectionKey)
          const filtered = (existing ?? []).filter(d => d !== path)
          const updated = [path, ...filtered].slice(0, MAX_RECENT)
          return { directories: new Map(state.directories).set(connectionKey, updated) }
        })
      },
      getRecent: (connectionKey: string) => {
        return get().directories.get(connectionKey) ?? EMPTY_ARRAY
      },
    }),
    {
      name: 'treeterm-recent-directories',
      storage: {
        getItem: (name) => {
          const raw = globalThis.localStorage?.getItem(name)
          if (!raw) return null
          const parsed = JSON.parse(raw) as { state: { directories: Record<string, string[]> }; version: number }
          return {
            ...parsed,
            state: {
              ...parsed.state,
              directories: new Map(Object.entries(parsed.state.directories)),
            },
          }
        },
        setItem: (name, value) => {
          const serializable = {
            ...value,
            state: {
              ...value.state,
              directories: Object.fromEntries(value.state.directories),
            },
          }
          globalThis.localStorage?.setItem(name, JSON.stringify(serializable))
        },
        removeItem: (name) => {
          globalThis.localStorage?.removeItem(name)
        },
      },
    }
  )
)
