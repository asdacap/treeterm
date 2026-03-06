import { create } from 'zustand'
import { persist } from 'zustand/middleware'
import type { Workspace, GitInfo, TerminalTab, SandboxConfig } from '../types'

const defaultSandbox: SandboxConfig = {
  enabled: false,
  allowNetwork: true,
  allowedPaths: []
}

interface WorkspaceState {
  workspaces: Record<string, Workspace>
  activeWorkspaceId: string | null
  addWorkspace: (path: string) => Promise<string>
  addChildWorkspace: (parentId: string, name: string, sandboxed?: boolean) => Promise<{ success: boolean; error?: string }>
  removeWorkspace: (id: string) => Promise<void>
  mergeAndRemoveWorkspace: (id: string, squash: boolean) => Promise<{ success: boolean; error?: string }>
  setActiveWorkspace: (id: string | null) => void
  updateGitInfo: (id: string, gitInfo: GitInfo) => void
  refreshGitInfo: (id: string) => Promise<void>
  updateWorkspaceStatus: (id: string, status: Workspace['status']) => void
  toggleSandbox: (id: string) => void
  updateSandboxConfig: (id: string, config: Partial<SandboxConfig>) => void
  // Terminal tab management
  addTerminal: (workspaceId: string) => string
  removeTerminal: (workspaceId: string, terminalId: string) => void
  setActiveTerminal: (workspaceId: string, terminalId: string) => void
  updateTerminalTitle: (workspaceId: string, terminalId: string, title: string) => void
  setPtyId: (workspaceId: string, terminalId: string, ptyId: string) => void
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
          terminals: [{ id: terminalId, title: 'Terminal 1', ptyId: null }],
          activeTerminalId: terminalId,
          sandbox: { ...defaultSandbox }
        }

        set((state) => ({
          workspaces: { ...state.workspaces, [id]: workspace },
          activeWorkspaceId: id
        }))

        return id
      },

      addChildWorkspace: async (parentId: string, name: string, sandboxed: boolean = false) => {
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
          terminals: [{ id: terminalId, title: 'Terminal 1', ptyId: null }],
          activeTerminalId: terminalId,
          sandbox: { ...defaultSandbox, enabled: sandboxed }
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

        // Kill all PTYs for this workspace's terminals
        for (const terminal of workspace.terminals) {
          if (terminal.ptyId) {
            window.electron.terminal.kill(terminal.ptyId)
          }
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

      updateWorkspaceStatus: (id: string, status: Workspace['status']) => {
        set((state) => {
          const workspace = state.workspaces[id]
          if (!workspace) return state
          return {
            workspaces: {
              ...state.workspaces,
              [id]: { ...workspace, status }
            }
          }
        })
      },

      toggleSandbox: (id: string) => {
        set((state) => {
          const workspace = state.workspaces[id]
          if (!workspace) return state
          return {
            workspaces: {
              ...state.workspaces,
              [id]: {
                ...workspace,
                sandbox: {
                  ...workspace.sandbox,
                  enabled: !workspace.sandbox?.enabled
                }
              }
            }
          }
        })
      },

      updateSandboxConfig: (id: string, config: Partial<SandboxConfig>) => {
        set((state) => {
          const workspace = state.workspaces[id]
          if (!workspace) return state
          return {
            workspaces: {
              ...state.workspaces,
              [id]: {
                ...workspace,
                sandbox: { ...workspace.sandbox, ...config }
              }
            }
          }
        })
      },

      mergeAndRemoveWorkspace: async (id: string, squash: boolean) => {
        const state = get()
        const workspace = state.workspaces[id]

        if (!workspace) {
          return { success: false, error: 'Workspace not found' }
        }

        if (!workspace.isWorktree || !workspace.parentId) {
          return { success: false, error: 'Not a worktree workspace' }
        }

        const parent = state.workspaces[workspace.parentId]
        if (!parent || !parent.gitRootPath || !parent.gitBranch) {
          return { success: false, error: 'Parent workspace not found or not a git repo' }
        }

        // Check for uncommitted changes
        const hasChanges = await window.electron.git.hasUncommittedChanges(workspace.path)
        if (hasChanges) {
          // Auto-commit changes before merge
          const commitResult = await window.electron.git.commitAll(
            workspace.path,
            `WIP: Auto-commit before merge from ${workspace.name}`
          )
          if (!commitResult.success) {
            return { success: false, error: `Failed to commit changes: ${commitResult.error}` }
          }
        }

        // Perform merge
        const mergeResult = await window.electron.git.merge(
          parent.gitRootPath,
          workspace.gitBranch!,
          parent.gitBranch,
          squash
        )

        if (!mergeResult.success) {
          return { success: false, error: `Merge failed: ${mergeResult.error}` }
        }

        // Delete the merged branch
        if (workspace.gitBranch) {
          await window.electron.git.deleteBranch(parent.gitRootPath, workspace.gitBranch)
        }

        // Update workspace status
        get().updateWorkspaceStatus(id, 'merged')

        // Remove the workspace
        await get().removeWorkspace(id)

        return { success: true }
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
            title: `Terminal ${terminalNumber}`,
            ptyId: null
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
        const workspace = get().workspaces[workspaceId]
        if (!workspace) return
        if (workspace.terminals.length <= 1) return // Keep at least one terminal

        // Kill the PTY before removing from state
        const terminal = workspace.terminals.find((t) => t.id === terminalId)
        if (terminal?.ptyId) {
          window.electron.terminal.kill(terminal.ptyId)
        }

        set((state) => {
          const workspace = state.workspaces[workspaceId]
          if (!workspace) return state

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
      },

      setPtyId: (workspaceId: string, terminalId: string, ptyId: string) => {
        set((state) => {
          const workspace = state.workspaces[workspaceId]
          if (!workspace) return state

          return {
            workspaces: {
              ...state.workspaces,
              [workspaceId]: {
                ...workspace,
                terminals: workspace.terminals.map((t) =>
                  t.id === terminalId ? { ...t, ptyId } : t
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
