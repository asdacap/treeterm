/**
 * Per-workspace Zustand store for Run Actions.
 * Manages detection and execution of project-defined tasks.
 */

import { createStore } from 'zustand/vanilla'
import type { RunAction } from '../types'

export interface RunActionsDeps {
  detect: (workspacePath: string) => Promise<RunAction[]>
  run: (workspacePath: string, actionId: string) => Promise<string | null>
}

export interface RunActionsState {
  actions: RunAction[]
  detecting: boolean

  detect: () => Promise<void>
  run: (actionId: string) => Promise<string | null>
}

export function createRunActionsStore(
  workspacePath: string,
  deps: RunActionsDeps
) {
  const store = createStore<RunActionsState>()((set, _get) => ({
    actions: [],
    detecting: false,

    detect: async () => {
      set({ detecting: true })
      try {
        const actions = await deps.detect(workspacePath)
        set({ actions })
      } finally {
        set({ detecting: false })
      }
    },

    run: async (actionId: string) => {
      return deps.run(workspacePath, actionId)
    }
  }))

  // Auto-detect on creation
  store.getState().detect()

  return store
}
