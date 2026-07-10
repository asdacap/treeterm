/* eslint-disable custom/no-string-literal-comparison -- TODO: migrate existing string-literal comparisons to enums */
import { create } from 'zustand'
import { persist, type PersistStorage, type StorageValue } from 'zustand/middleware'
import { ConnectionTargetType, type ConnectionInfo } from '../../shared/types'

/**
 * The label shown for a session when the user has not set a custom name.
 *
 * Derived purely from the connection — never from the ephemeral session id.
 * The session id is regenerated on every connect/reconnect, so anything keyed
 * by it (including the persisted custom-name store) can miss; falling back to
 * this keeps the display stable ("LOCAL" / "user@host") instead of leaking the
 * raw `session-<timestamp>-<random>` id.
 */
export function deriveDefaultSessionName(connection: ConnectionInfo): string {
  if (connection.target.type === ConnectionTargetType.Remote) {
    const cfg = connection.target.config
    return cfg.label || `${cfg.user}@${cfg.host}`
  }
  return 'LOCAL'
}

/**
 * Per-session UI state. An entry exists for any session the user has renamed
 * *or* reordered, so `name` is `''` whenever only the ordering was customised.
 */
interface SessionNameEntry {
  name: string
  lastUsed: number
  sortOrder: number
}

export interface SessionNamesState {
  names: Map<string, SessionNameEntry>
  setName: (sessionId: string, name: string) => void
  clearName: (sessionId: string) => void
  getName: (sessionId: string) => string | undefined
  /**
   * `sessionIds` is every session currently on screen, in whatever order the
   * caller holds them. Ordering is assigned across that whole list — not just
   * the sessions that happen to already have an entry — so an unnamed session
   * can be dragged just like a renamed one.
   */
  reorderSession: (sessionIds: string[], dragId: string, targetId: string, position: 'before' | 'after') => void
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

// Sessions without an entry sort last, keeping their incoming relative order.
function sortIds(ids: string[], names: Map<string, SessionNameEntry>): string[] {
  return [...ids].sort((a, b) => {
    const aOrder = names.get(a)?.sortOrder ?? Infinity
    const bOrder = names.get(b)?.sortOrder ?? Infinity
    return aOrder - bOrder
  })
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
      // Drops the custom name but keeps the entry: the session's position in the
      // list survives a rename-back-to-default.
      clearName: (sessionId: string) => {
        set((state) => {
          const existing = state.names.get(sessionId)
          if (!existing) return state
          return {
            names: new Map(state.names).set(sessionId, { ...existing, name: '' }),
          }
        })
      },
      getName: (sessionId: string) => {
        const name = get().names.get(sessionId)?.name
        return name === '' ? undefined : name
      },
      reorderSession: (sessionIds: string[], dragId: string, targetId: string, position: 'before' | 'after') => {
        if (dragId === targetId) return
        set((state) => {
          if (!sessionIds.includes(dragId) || !sessionIds.includes(targetId)) return state

          const ordered = sortIds(sessionIds, state.names).filter((id) => id !== dragId)
          const targetIdx = ordered.indexOf(targetId)
          ordered.splice(position === 'before' ? targetIdx : targetIdx + 1, 0, dragId)

          const now = Date.now()
          const updated = new Map(state.names)
          for (let i = 0; i < ordered.length; i++) {
            // eslint-disable-next-line @typescript-eslint/no-non-null-assertion -- index within bounds
            const id = ordered[i]!
            const existing = updated.get(id)
            updated.set(id, existing ? { ...existing, sortOrder: i } : { name: '', lastUsed: now, sortOrder: i })
          }
          return { names: updated }
        })
      },
      getSortedIds: (ids: string[]) => {
        return sortIds(ids, get().names)
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
