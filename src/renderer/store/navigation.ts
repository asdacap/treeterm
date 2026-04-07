import { create } from 'zustand'

export type ActiveView =
  | { type: 'workspace'; workspaceId: string; sessionId: string }
  | { type: 'session'; sessionId: string }

interface NavigationState {
  activeView: ActiveView | null
  setActiveView: (view: ActiveView) => void
}

export const useNavigationStore = create<NavigationState>()((set) => ({
  activeView: null,
  setActiveView: (view: ActiveView) => { set({ activeView: view }); },
}))
