import { create } from 'zustand'

export type PrefixModeState = 'idle' | 'active' | 'workspace_focus'

interface PrefixModeStore {
  state: PrefixModeState
  activatedAt: number | null
  focusedWorkspaceIndex: number
  workspaceIds: string[]

  // Actions
  activate: () => void
  deactivate: () => void
  enterWorkspaceFocus: (workspaceIds: string[], currentIndex: number) => void
  navigateWorkspace: (direction: 'up' | 'down') => void
  selectFocusedWorkspace: () => string | null
}

export const usePrefixModeStore = create<PrefixModeStore>((set, get) => ({
  state: 'idle',
  activatedAt: null,
  focusedWorkspaceIndex: 0,
  workspaceIds: [],

  activate: () =>
    set({
      state: 'active',
      activatedAt: Date.now()
    }),

  deactivate: () =>
    set({
      state: 'idle',
      activatedAt: null,
      focusedWorkspaceIndex: 0,
      workspaceIds: []
    }),

  enterWorkspaceFocus: (workspaceIds: string[], currentIndex: number) =>
    set({
      state: 'workspace_focus',
      activatedAt: Date.now(),
      workspaceIds,
      focusedWorkspaceIndex: currentIndex
    }),

  navigateWorkspace: (direction: 'up' | 'down') => {
    const { focusedWorkspaceIndex, workspaceIds } = get()
    if (workspaceIds.length === 0) return

    let newIndex = focusedWorkspaceIndex
    if (direction === 'up') {
      newIndex = focusedWorkspaceIndex > 0 ? focusedWorkspaceIndex - 1 : workspaceIds.length - 1
    } else {
      newIndex = focusedWorkspaceIndex < workspaceIds.length - 1 ? focusedWorkspaceIndex + 1 : 0
    }

    set({ focusedWorkspaceIndex: newIndex })
  },

  selectFocusedWorkspace: () => {
    const { focusedWorkspaceIndex, workspaceIds } = get()
    if (workspaceIds.length === 0) return null
    return workspaceIds[focusedWorkspaceIndex] || null
  }
}))
