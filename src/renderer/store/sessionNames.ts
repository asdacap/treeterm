/* eslint-disable custom/no-string-literal-comparison -- TODO: migrate existing string-literal comparisons to enums */
import { create } from 'zustand'
import { persist, type PersistStorage, type StorageValue } from 'zustand/middleware'

interface SessionNameEntry {
  name: string
  lastUsed: number
  sortOrder: number
}

interface SessionNamesState {
  names: Map<string, SessionNameEntry>
  setName: (sessionId: string, name: string) => void
  removeName: (sessionId: string) => void
  getName: (sessionId: string) => string | undefined
  reorderSession: (dragId: string, targetId: string, position: 'before' | 'after') => void
  getSortedIds: (ids: string[]) => string[]
  cleanupStale: () => void
}

const SEVEN_DAYS_MS = 7 * 24 * 60 * 60 * 1000

function nextSortOrder(names: Map<string, SessionNameEntry>): number {
  let max = -1
  for (const [, entry] of Array.from(names.entries())) {
    if (entry.sortOrder > max) max = entry.sortOrder
  }
  return max + 1
}

export const useSessionNamesStore = create<SessionNamesState>()(
  persist(
    (set, get) => ({
      names: new Map<string, SessionNameEntry>(),
      setName: (sessionId: string, name: string) => {
        set((state) => {
          const existing = state.names.get(sessionId)
          const sortOrder = existing?.sortOrder ?? nextSortOrder(state.names)
          return {
            names: new Map(state.names).set(sessionId, { name, lastUsed: Date.now(), sortOrder }),
          }
        })
      },
      removeName: (sessionId: string) => {
        set((state) => {
          const updated = new Map(state.names)
          updated.delete(sessionId)
          return { names: updated }
        })
      },
      getName: (sessionId: string) => {
        return get().names.get(sessionId)?.name
      },
      reorderSession: (dragId: string, targetId: string, position: 'before' | 'after') => {
        if (dragId === targetId) return
        set((state) => {
          const { names } = state
          if (!names.has(dragId) || !names.has(targetId)) return state

          const sorted = Array.from(names.entries())
            .sort(([, a], [, b]) => a.sortOrder - b.sortOrder)

          const ordered = sorted.filter(([id]) => id !== dragId)
          const targetIdx = ordered.findIndex(([id]) => id === targetId)
          const insertIdx = position === 'before' ? targetIdx : targetIdx + 1
          const dragEntry = sorted.find(([id]) => id === dragId)
          if (dragEntry) ordered.splice(insertIdx, 0, dragEntry)

          const updated = new Map<string, SessionNameEntry>()
          for (let i = 0; i < ordered.length; i++) {
            // eslint-disable-next-line @typescript-eslint/no-non-null-assertion -- index within bounds
            const [id, entry] = ordered[i]!
            updated.set(id, { ...entry, sortOrder: i })
          }
          return { names: updated }
        })
      },
      getSortedIds: (ids: string[]) => {
        const { names } = get()
        return [...ids].sort((a, b) => {
          const aOrder = names.get(a)?.sortOrder ?? Infinity
          const bOrder = names.get(b)?.sortOrder ?? Infinity
          return aOrder - bOrder
        })
      },
      cleanupStale: () => {
        const now = Date.now()
        set((state) => {
          const names = new Map<string, SessionNameEntry>()
          for (const [id, entry] of Array.from(state.names.entries())) {
            if (now - entry.lastUsed < SEVEN_DAYS_MS) {
              names.set(id, entry)
            }
          }
          return { names }
        })
      },
    }),
    {
      name: 'treeterm-session-names',
      storage: ((): PersistStorage<SessionNamesState> => {
        const storage = typeof localStorage !== 'undefined' ? localStorage : undefined
        return {
          getItem: (name: string): StorageValue<SessionNamesState> | null => {
            const raw = storage?.getItem(name)
            if (!raw) return null
            const parsed = JSON.parse(raw) as { state: { names: Record<string, SessionNameEntry> }; version?: number }
            return {
              ...parsed,
              state: {
                ...parsed.state,
                names: new Map(Object.entries(parsed.state.names)),
              } as SessionNamesState,
            }
          },
          setItem: (name: string, value: StorageValue<SessionNamesState>) => {
            const serializable = {
              ...value,
              state: {
                ...value.state,
                names: Object.fromEntries(value.state.names),
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

// Cleanup stale entries on store creation
useSessionNamesStore.getState().cleanupStale()
