import { create } from 'zustand'

export type PrefixModeState = 'idle' | 'active'

interface PrefixModeStore {
  state: PrefixModeState
  activatedAt: number | null

  // Actions
  activate: () => void
  deactivate: () => void
}

export const usePrefixModeStore = create<PrefixModeStore>((set) => ({
  state: 'idle',
  activatedAt: null,

  activate: () =>
    set({
      state: 'active',
      activatedAt: Date.now()
    }),

  deactivate: () =>
    set({
      state: 'idle',
      activatedAt: null
    })
}))
