import { createContext, useContext } from 'react'
import type { StoreApi } from 'zustand'
import type { SessionState } from '../store/createSessionStore'

export const SessionStoreContext = createContext<StoreApi<SessionState> | null>(null)

export function useSessionApi(): StoreApi<SessionState> {
  const store = useContext(SessionStoreContext)
  if (!store) {
    throw new Error('useSessionApi must be used within a SessionStoreContext.Provider')
  }
  return store
}
