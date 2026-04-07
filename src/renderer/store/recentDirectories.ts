import { create } from 'zustand'
import { persist, type PersistStorage, type StorageValue } from 'zustand/middleware'

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
      storage: ((): PersistStorage<RecentDirectoriesState> => {
        const storage = typeof localStorage !== 'undefined' ? localStorage : undefined
        return {
          getItem: (name: string): StorageValue<RecentDirectoriesState> | null => {
            const raw = storage?.getItem(name)
            if (!raw) return null
            const parsed = JSON.parse(raw) as { state: { directories: Record<string, string[]> }; version?: number }
            return {
              ...parsed,
              state: {
                ...parsed.state,
                directories: new Map(Object.entries(parsed.state.directories)),
              } as RecentDirectoriesState,
            }
          },
          setItem: (name: string, value: StorageValue<RecentDirectoriesState>) => {
            const serializable = {
              ...value,
              state: {
                ...value.state,
                directories: Object.fromEntries(value.state.directories),
              },
            }
            storage?.setItem(name, JSON.stringify(serializable))
          },
          removeItem: (name: string) => {
            storage?.removeItem(name)
          },
        }
      })(),
    }
  )
)
