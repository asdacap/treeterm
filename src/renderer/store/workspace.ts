import { create } from 'zustand'
import { persist } from 'zustand/middleware'
import type { Workspace, GitInfo } from '../types'

interface WorkspaceState {
  workspaces: Record<string, Workspace>
  activeWorkspaceId: string | null
  addWorkspace: (path: string) => Promise<string>
  addChildWorkspace: (parentId: string, name: string) => Promise<{ success: boolean; error?: string }>
  removeWorkspace: (id: string) => Promise<void>
  setActiveWorkspace: (id: string | null) => void
  updateGitInfo: (id: string, gitInfo: GitInfo) => void
  refreshGitInfo: (id: string) => Promise<void>
}

function generateId(): string {
  return `ws-${Date.now()}-${Math.random().toString(36).slice(2, 9)}`
}

function getNameFromPath(path: string): string {
  return path.split('/').pop() || path
}

export const useWorkspaceStore = create<WorkspaceState>()(
  persist(
    (set, get) => ({
      workspaces: {},
      activeWorkspaceId: null,

      addWorkspace: async (path: string) => {
        const id = generateId()

        // Get git info for the path
        const gitInfo = await window.electron.git.getInfo(path)

        const workspace: Workspace = {
          id,
          name: getNameFromPath(path),
          path,
          parentId: null,
          children: [],
          status: 'active',
          isGitRepo: gitInfo.isRepo,
          gitBranch: gitInfo.branch,
          gitRootPath: gitInfo.rootPath,
          isWorktree: false
        }

        set((state) => ({
          workspaces: { ...state.workspaces, [id]: workspace },
          activeWorkspaceId: id
        }))

        return id
      },

      addChildWorkspace: async (parentId: string, name: string) => {
        const state = get()
        const parent = state.workspaces[parentId]

        if (!parent) {
          return { success: false, error: 'Parent workspace not found' }
        }

        if (!parent.isGitRepo || !parent.gitRootPath) {
          return { success: false, error: 'Parent workspace is not a git repository' }
        }

        // Create worktree
        const result = await window.electron.git.createWorktree(
          parent.gitRootPath,
          name,
          parent.gitBranch || undefined
        )

        if (!result.success) {
          return { success: false, error: result.error }
        }

        // Create child workspace
        const id = generateId()
        const childWorkspace: Workspace = {
          id,
          name,
          path: result.path!,
          parentId,
          children: [],
          status: 'active',
          isGitRepo: true,
          gitBranch: result.branch!,
          gitRootPath: parent.gitRootPath,
          isWorktree: true
        }

        set((state) => ({
          workspaces: {
            ...state.workspaces,
            [id]: childWorkspace,
            [parentId]: {
              ...state.workspaces[parentId],
              children: [...state.workspaces[parentId].children, id]
            }
          },
          activeWorkspaceId: id
        }))

        return { success: true }
      },

      removeWorkspace: async (id: string) => {
        const state = get()
        const workspace = state.workspaces[id]

        if (!workspace) return

        // Recursively remove children first
        for (const childId of workspace.children) {
          await get().removeWorkspace(childId)
        }

        // If this is a worktree, remove it from git
        if (workspace.isWorktree && workspace.gitRootPath) {
          await window.electron.git.removeWorktree(
            workspace.gitRootPath,
            workspace.path,
            true // delete branch
          )
        }

        // Remove from parent's children array
        if (workspace.parentId) {
          set((state) => {
            const parent = state.workspaces[workspace.parentId!]
            if (parent) {
              return {
                workspaces: {
                  ...state.workspaces,
                  [workspace.parentId!]: {
                    ...parent,
                    children: parent.children.filter((cid) => cid !== id)
                  }
                }
              }
            }
            return state
          })
        }

        // Remove workspace
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
      },

      updateGitInfo: (id: string, gitInfo: GitInfo) => {
        set((state) => {
          const workspace = state.workspaces[id]
          if (!workspace) return state
          return {
            workspaces: {
              ...state.workspaces,
              [id]: {
                ...workspace,
                isGitRepo: gitInfo.isRepo,
                gitBranch: gitInfo.branch,
                gitRootPath: gitInfo.rootPath
              }
            }
          }
        })
      },

      refreshGitInfo: async (id: string) => {
        const state = get()
        const workspace = state.workspaces[id]
        if (!workspace) return

        const gitInfo = await window.electron.git.getInfo(workspace.path)
        get().updateGitInfo(id, gitInfo)
      }
    }),
    {
      name: 'treeterm-workspaces'
    }
  )
)
