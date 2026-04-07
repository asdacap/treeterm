import { create } from 'zustand'
import { persist, type PersistStorage, type StorageValue } from 'zustand/middleware'

interface SessionNameEntry {
  name: string
  lastUsed: number
}

interface SessionNamesState {
  names: Map<string, SessionNameEntry>
  setName: (sessionId: string, name: string) => void
  removeName: (sessionId: string) => void
  getName: (sessionId: string) => string | undefined
  cleanupStale: () => void
}

const SEVEN_DAYS_MS = 7 * 24 * 60 * 60 * 1000

export const useSessionNamesStore = create<SessionNamesState>()(
  persist(
    (set, get) => ({
      names: new Map<string, SessionNameEntry>(),
      setName: (sessionId: string, name: string) => {
        set((state) => ({
          names: new Map(state.names).set(sessionId, { name, lastUsed: Date.now() }),
        }))
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
