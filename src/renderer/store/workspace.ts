import { create } from 'zustand'
import { persist } from 'zustand/middleware'
import type { Workspace, GitInfo, TerminalTab, FilesystemTab, WorkspaceTab, SandboxConfig } from '../types'

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
  // Tab management (terminals and filesystem browsers)
  addTerminal: (workspaceId: string) => string
  addFilesystemTab: (workspaceId: string) => string
  removeTab: (workspaceId: string, tabId: string) => void
  setActiveTab: (workspaceId: string, tabId: string) => void
  updateTerminalTitle: (workspaceId: string, terminalId: string, title: string) => void
  setPtyId: (workspaceId: string, terminalId: string, ptyId: string) => void
  // Filesystem browser state
  setSelectedPath: (workspaceId: string, tabId: string, path: string | null) => void
  toggleExpandedDir: (workspaceId: string, tabId: string, dirPath: string) => void
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
        const filesTabId = `files-${id}`

        // Get git info for the path
        const gitInfo = await window.electron.git.getInfo(path)

        const filesTab: FilesystemTab = {
          type: 'filesystem',
          id: filesTabId,
          title: 'Files',
          selectedPath: null,
          expandedDirs: []
        }

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
          tabs: [filesTab, { type: 'terminal', id: terminalId, title: 'Terminal 1', ptyId: null }],
          activeTabId: terminalId,
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
        const filesTabId = `files-${id}`

        const filesTab: FilesystemTab = {
          type: 'filesystem',
          id: filesTabId,
          title: 'Files',
          selectedPath: null,
          expandedDirs: []
        }

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
          tabs: [filesTab, { type: 'terminal', id: terminalId, title: 'Terminal 1', ptyId: null }],
          activeTabId: terminalId,
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

        // Kill all PTYs for this workspace's terminal tabs
        for (const tab of workspace.tabs) {
          if (tab.type === 'terminal' && tab.ptyId) {
            window.electron.terminal.kill(tab.ptyId)
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

      // Tab management (terminals and filesystem browsers)
      addTerminal: (workspaceId: string) => {
        const terminalId = generateTerminalId()
        set((state) => {
          const workspace = state.workspaces[workspaceId]
          if (!workspace) return state

          const terminalCount = workspace.tabs.filter((t) => t.type === 'terminal').length
          const newTerminal: TerminalTab = {
            type: 'terminal',
            id: terminalId,
            title: `Terminal ${terminalCount + 1}`,
            ptyId: null
          }

          return {
            workspaces: {
              ...state.workspaces,
              [workspaceId]: {
                ...workspace,
                tabs: [...workspace.tabs, newTerminal],
                activeTabId: terminalId
              }
            }
          }
        })
        return terminalId
      },

      addFilesystemTab: (workspaceId: string) => {
        const tabId = `fs-${Date.now()}-${Math.random().toString(36).slice(2, 9)}`
        set((state) => {
          const workspace = state.workspaces[workspaceId]
          if (!workspace) return state

          const fsCount = workspace.tabs.filter((t) => t.type === 'filesystem').length
          const newTab: FilesystemTab = {
            type: 'filesystem',
            id: tabId,
            title: `Files ${fsCount + 1}`,
            selectedPath: null,
            expandedDirs: []
          }

          return {
            workspaces: {
              ...state.workspaces,
              [workspaceId]: {
                ...workspace,
                tabs: [...workspace.tabs, newTab],
                activeTabId: tabId
              }
            }
          }
        })
        return tabId
      },

      removeTab: (workspaceId: string, tabId: string) => {
        const workspace = get().workspaces[workspaceId]
        if (!workspace) return

        // Find the tab to remove
        const tab = workspace.tabs.find((t) => t.id === tabId)
        if (!tab) return

        // Prevent removing filesystem tabs (they are always available)
        if (tab.type === 'filesystem') return

        // Keep at least one terminal tab
        const terminalTabs = workspace.tabs.filter((t) => t.type === 'terminal')
        if (terminalTabs.length <= 1) return

        // Kill the PTY if it's a terminal tab
        if (tab.type === 'terminal' && tab.ptyId) {
          window.electron.terminal.kill(tab.ptyId)
        }

        set((state) => {
          const workspace = state.workspaces[workspaceId]
          if (!workspace) return state

          const newTabs = workspace.tabs.filter((t) => t.id !== tabId)
          let newActiveTabId = workspace.activeTabId

          // If we're removing the active tab, switch to another
          if (workspace.activeTabId === tabId) {
            const removedIndex = workspace.tabs.findIndex((t) => t.id === tabId)
            const newIndex = Math.min(removedIndex, newTabs.length - 1)
            newActiveTabId = newTabs[newIndex]?.id || null
          }

          return {
            workspaces: {
              ...state.workspaces,
              [workspaceId]: {
                ...workspace,
                tabs: newTabs,
                activeTabId: newActiveTabId
              }
            }
          }
        })
      },

      setActiveTab: (workspaceId: string, tabId: string) => {
        set((state) => {
          const workspace = state.workspaces[workspaceId]
          if (!workspace) return state

          return {
            workspaces: {
              ...state.workspaces,
              [workspaceId]: {
                ...workspace,
                activeTabId: tabId
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
                tabs: workspace.tabs.map((t) =>
                  t.id === terminalId && t.type === 'terminal' ? { ...t, title } : t
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
                tabs: workspace.tabs.map((t) =>
                  t.id === terminalId && t.type === 'terminal' ? { ...t, ptyId } : t
                )
              }
            }
          }
        })
      },

      // Filesystem browser state
      setSelectedPath: (workspaceId: string, tabId: string, path: string | null) => {
        set((state) => {
          const workspace = state.workspaces[workspaceId]
          if (!workspace) return state

          return {
            workspaces: {
              ...state.workspaces,
              [workspaceId]: {
                ...workspace,
                tabs: workspace.tabs.map((t) =>
                  t.id === tabId && t.type === 'filesystem' ? { ...t, selectedPath: path } : t
                )
              }
            }
          }
        })
      },

      toggleExpandedDir: (workspaceId: string, tabId: string, dirPath: string) => {
        set((state) => {
          const workspace = state.workspaces[workspaceId]
          if (!workspace) return state

          return {
            workspaces: {
              ...state.workspaces,
              [workspaceId]: {
                ...workspace,
                tabs: workspace.tabs.map((t) => {
                  if (t.id === tabId && t.type === 'filesystem') {
                    const isExpanded = t.expandedDirs.includes(dirPath)
                    return {
                      ...t,
                      expandedDirs: isExpanded
                        ? t.expandedDirs.filter((d) => d !== dirPath)
                        : [...t.expandedDirs, dirPath]
                    }
                  }
                  return t
                })
              }
            }
          }
        })
      }
    }),
    {
      name: 'treeterm-workspaces',
      version: 1,
      migrate: (persistedState: unknown, version: number) => {
        const state = persistedState as { workspaces?: Record<string, unknown> }
        if (version === 0 && state.workspaces) {
          // Migrate from old format (terminals/activeTerminalId) to new format (tabs/activeTabId)
          for (const ws of Object.values(state.workspaces) as Array<{
            terminals?: Array<{ id: string; title: string; ptyId: string | null }>
            tabs?: WorkspaceTab[]
            activeTerminalId?: string | null
            activeTabId?: string | null
          }>) {
            if (ws.terminals && !ws.tabs) {
              ws.tabs = ws.terminals.map((t) => ({ type: 'terminal' as const, ...t }))
              delete ws.terminals
            }
            if (ws.activeTerminalId !== undefined && ws.activeTabId === undefined) {
              ws.activeTabId = ws.activeTerminalId
              delete ws.activeTerminalId
            }
          }
        }
        return state as WorkspaceState
      }
    }
  )
)
