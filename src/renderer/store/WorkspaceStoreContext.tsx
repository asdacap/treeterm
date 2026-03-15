import { createContext, useContext } from 'react'
import type { StoreApi } from 'zustand'
import { useStore } from 'zustand'
import type { WorkspaceState } from './createWorkspaceStore'

export const WorkspaceStoreContext = createContext<StoreApi<WorkspaceState> | null>(null)

// Drop-in replacement hook — same selector API as before.
// Call as useWorkspaceStore() to get full state, or useWorkspaceStore(s => s.foo) for a slice.
export function useWorkspaceStore(): WorkspaceState
export function useWorkspaceStore<T>(selector: (s: WorkspaceState) => T): T
export function useWorkspaceStore<T = WorkspaceState>(selector?: (s: WorkspaceState) => T): T | WorkspaceState {
  const store = useContext(WorkspaceStoreContext)
  if (!store) {
    throw new Error('useWorkspaceStore must be used within a WorkspaceStoreContext.Provider')
  }
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  return useStore(store, selector || ((s) => s as any))
}
