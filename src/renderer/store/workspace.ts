import { create } from 'zustand'
import { persist } from 'zustand/middleware'
import type { Workspace } from '../types'

interface WorkspaceState {
  workspaces: Record<string, Workspace>
  activeWorkspaceId: string | null
  addWorkspace: (path: string) => string
  removeWorkspace: (id: string) => void
  setActiveWorkspace: (id: string | null) => void
}

function generateId(): string {
  return `ws-${Date.now()}-${Math.random().toString(36).slice(2, 9)}`
}

function getNameFromPath(path: string): string {
  return path.split('/').pop() || path
}

export const useWorkspaceStore = create<WorkspaceState>()(
  persist(
    (set) => ({
      workspaces: {},
      activeWorkspaceId: null,

      addWorkspace: (path: string) => {
        const id = generateId()
        const workspace: Workspace = {
          id,
          name: getNameFromPath(path),
          path,
          parentId: null,
          children: [],
          status: 'active'
        }
        set((state) => ({
          workspaces: { ...state.workspaces, [id]: workspace },
          activeWorkspaceId: id
        }))
        return id
      },

      removeWorkspace: (id: string) => {
        set((state) => {
          const { [id]: removed, ...rest } = state.workspaces
          return {
            workspaces: rest,
            activeWorkspaceId: state.activeWorkspaceId === id ? null : state.activeWorkspaceId
          }
        })
      },

      setActiveWorkspace: (id: string | null) => {
        set({ activeWorkspaceId: id })
      }
    }),
    {
      name: 'treeterm-workspaces'
    }
  )
)
