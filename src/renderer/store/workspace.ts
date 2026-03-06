import { create } from 'zustand'
import { persist } from 'zustand/middleware'
import type { Workspace, GitInfo, TerminalTab } from '../types'

interface WorkspaceState {
  workspaces: Record<string, Workspace>
  activeWorkspaceId: string | null
  addWorkspace: (path: string) => Promise<string>
  addChildWorkspace: (parentId: string, name: string) => Promise<{ success: boolean; error?: string }>
  removeWorkspace: (id: string) => Promise<void>
  setActiveWorkspace: (id: string | null) => void
  updateGitInfo: (id: string, gitInfo: GitInfo) => void
  refreshGitInfo: (id: string) => Promise<void>
  // Terminal tab management
  addTerminal: (workspaceId: string) => string
  removeTerminal: (workspaceId: string, terminalId: string) => void
  setActiveTerminal: (workspaceId: string, terminalId: string) => void
  updateTerminalTitle: (workspaceId: string, terminalId: string, title: string) => void
}

function generateId(): string {
  return `ws-${Date.now()}-${Math.random().toString(36).slice(2, 9)}`
}

function generateTerminalId(): string {
  return `term-${Date.now()}-${Math.random().toString(36).slice(2, 9)}`
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
        const terminalId = generateTerminalId()

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
          isWorktree: false,
          terminals: [{ id: terminalId, title: 'Terminal 1' }],
          activeTerminalId: terminalId
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
        const terminalId = generateTerminalId()
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
          isWorktree: true,
          terminals: [{ id: terminalId, title: 'Terminal 1' }],
          activeTerminalId: terminalId
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
      },

      // Terminal tab management
      addTerminal: (workspaceId: string) => {
        const terminalId = generateTerminalId()
        set((state) => {
          const workspace = state.workspaces[workspaceId]
          if (!workspace) return state

          const terminalNumber = workspace.terminals.length + 1
          const newTerminal: TerminalTab = {
            id: terminalId,
            title: `Terminal ${terminalNumber}`
          }

          return {
            workspaces: {
              ...state.workspaces,
              [workspaceId]: {
                ...workspace,
                terminals: [...workspace.terminals, newTerminal],
                activeTerminalId: terminalId
              }
            }
          }
        })
        return terminalId
      },

      removeTerminal: (workspaceId: string, terminalId: string) => {
        set((state) => {
          const workspace = state.workspaces[workspaceId]
          if (!workspace) return state
          if (workspace.terminals.length <= 1) return state // Keep at least one terminal

          const newTerminals = workspace.terminals.filter((t) => t.id !== terminalId)
          let newActiveTerminalId = workspace.activeTerminalId

          // If we're removing the active terminal, switch to another
          if (workspace.activeTerminalId === terminalId) {
            const removedIndex = workspace.terminals.findIndex((t) => t.id === terminalId)
            const newIndex = Math.min(removedIndex, newTerminals.length - 1)
            newActiveTerminalId = newTerminals[newIndex]?.id || null
          }

          return {
            workspaces: {
              ...state.workspaces,
              [workspaceId]: {
                ...workspace,
                terminals: newTerminals,
                activeTerminalId: newActiveTerminalId
              }
            }
          }
        })
      },

      setActiveTerminal: (workspaceId: string, terminalId: string) => {
        set((state) => {
          const workspace = state.workspaces[workspaceId]
          if (!workspace) return state

          return {
            workspaces: {
              ...state.workspaces,
              [workspaceId]: {
                ...workspace,
                activeTerminalId: terminalId
              }
            }
          }
        })
      },

      updateTerminalTitle: (workspaceId: string, terminalId: string, title: string) => {
        set((state) => {
          const workspace = state.workspaces[workspaceId]
          if (!workspace) return state

          return {
            workspaces: {
              ...state.workspaces,
              [workspaceId]: {
                ...workspace,
                terminals: workspace.terminals.map((t) =>
                  t.id === terminalId ? { ...t, title } : t
                )
              }
            }
          }
        })
      }
    }),
    {
      name: 'treeterm-workspaces'
    }
  )
)
