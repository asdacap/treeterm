import { create } from 'zustand'
import { persist } from 'zustand/middleware'

interface SessionNameEntry {
  name: string
  lastUsed: number
}

interface SessionNamesState {
  names: Record<string, SessionNameEntry>
  setName: (sessionId: string, name: string) => void
  removeName: (sessionId: string) => void
  getName: (sessionId: string) => string | undefined
  cleanupStale: () => void
}

const SEVEN_DAYS_MS = 7 * 24 * 60 * 60 * 1000

export const useSessionNamesStore = create<SessionNamesState>()(
  persist(
    (set, get) => ({
      names: {},
      setName: (sessionId: string, name: string) => {
        set((state) => ({
          names: {
            ...state.names,
            [sessionId]: { name, lastUsed: Date.now() },
          },
        }))
      },
      removeName: (sessionId: string) => {
        set((state) => {
          const { [sessionId]: _, ...rest } = state.names
          return { names: rest }
        })
      },
      getName: (sessionId: string) => {
        return get().names[sessionId]?.name
      },
      cleanupStale: () => {
        const now = Date.now()
        set((state) => {
          const names: Record<string, SessionNameEntry> = {}
          for (const [id, entry] of Object.entries(state.names)) {
            if (now - entry.lastUsed < SEVEN_DAYS_MS) {
              names[id] = entry
            }
          }
          return { names }
        })
      },
    }),
    {
      name: 'treeterm-session-names',
    }
  )
)

// Cleanup stale entries on store creation
useSessionNamesStore.getState().cleanupStale()
