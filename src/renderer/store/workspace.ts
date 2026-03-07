import { create } from 'zustand'
import { persist } from 'zustand/middleware'
import type { Workspace, GitInfo, Tab } from '../types'
import { useSettingsStore } from './settings'
import { applicationRegistry } from '../registry/applicationRegistry'

interface WorkspaceState {
  workspaces: Record<string, Workspace>
  activeWorkspaceId: string | null
  addWorkspace: (path: string) => Promise<string>
  addChildWorkspace: (parentId: string, name: string) => Promise<{ success: boolean; error?: string }>
  removeWorkspace: (id: string) => Promise<void>
  mergeAndRemoveWorkspace: (id: string, squash: boolean) => Promise<{ success: boolean; error?: string }>
  setActiveWorkspace: (id: string | null) => void
  updateGitInfo: (id: string, gitInfo: GitInfo) => void
  refreshGitInfo: (id: string) => Promise<void>
  updateWorkspaceStatus: (id: string, status: Workspace['status']) => void
  // Tab management (application-agnostic)
  addTab: (workspaceId: string, instanceId: string) => string
  removeTab: (workspaceId: string, tabId: string) => Promise<void>
  setActiveTab: (workspaceId: string, tabId: string) => void
  updateTabTitle: (workspaceId: string, tabId: string, title: string) => void
  updateTabState: <T>(workspaceId: string, tabId: string, updater: (state: T) => T) => void
}

function generateId(): string {
  return `ws-${Date.now()}-${Math.random().toString(36).slice(2, 9)}`
}

function generateTabId(): string {
  return `tab-${Date.now()}-${Math.random().toString(36).slice(2, 9)}`
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
        const { settings } = useSettingsStore.getState()

        // Get git info for the path
        const gitInfo = await window.electron.git.getInfo(path)

        // Create tabs based on default application instances
        const tabs: Tab[] = []
        let activeTabId: string | null = null

        const defaultInstances = settings.applications.filter((inst) => inst.isDefault)
        for (const instance of defaultInstances) {
          const app = applicationRegistry.get(instance.applicationId)
          if (!app) continue

          const tabId = generateTabId()
          const existingCount = tabs.filter((t) => t.applicationId === instance.applicationId).length

          tabs.push({
            id: tabId,
            applicationId: instance.applicationId,
            title: `${instance.name} ${existingCount + 1}`,
            state: app.createInitialState(),
            config: instance.config
          })

          // Make first closable tab active, or first tab if none are closable
          if (activeTabId === null || (app.canClose && !applicationRegistry.get(tabs.find((t) => t.id === activeTabId)?.applicationId ?? '')?.canClose)) {
            activeTabId = tabId
          }
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
          tabs,
          activeTabId
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
        const { settings } = useSettingsStore.getState()

        // Create tabs based on default application instances
        const tabs: Tab[] = []
        let activeTabId: string | null = null

        const defaultInstances = settings.applications.filter((inst) => inst.isDefault)
        for (const instance of defaultInstances) {
          const app = applicationRegistry.get(instance.applicationId)
          if (!app) continue

          const tabId = generateTabId()
          const existingCount = tabs.filter((t) => t.applicationId === instance.applicationId).length

          tabs.push({
            id: tabId,
            applicationId: instance.applicationId,
            title: `${instance.name} ${existingCount + 1}`,
            state: app.createInitialState(),
            config: instance.config
          })

          // Make first closable tab active, or first tab if none are closable
          if (activeTabId === null || (app.canClose && !applicationRegistry.get(tabs.find((t) => t.id === activeTabId)?.applicationId ?? '')?.canClose)) {
            activeTabId = tabId
          }
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
          tabs,
          activeTabId
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

        // Run cleanup for all tabs using the application registry
        for (const tab of workspace.tabs) {
          const app = applicationRegistry.get(tab.applicationId)
          if (app?.cleanup) {
            await app.cleanup(tab, workspace)
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

      // Tab management (application-agnostic)
      addTab: (workspaceId: string, instanceId: string) => {
        const tabId = generateTabId()
        const { settings } = useSettingsStore.getState()
        const instance = settings.applications.find((a) => a.id === instanceId)

        if (!instance) return tabId

        const app = applicationRegistry.get(instance.applicationId)
        if (!app) return tabId

        set((state) => {
          const workspace = state.workspaces[workspaceId]
          if (!workspace) return state

          // Check if app allows multiple instances
          if (!app.canHaveMultiple) {
            const existing = workspace.tabs.find((t) => t.applicationId === instance.applicationId)
            if (existing) return state
          }

          const existingCount = workspace.tabs.filter(
            (t) => t.applicationId === instance.applicationId
          ).length

          const newTab: Tab = {
            id: tabId,
            applicationId: instance.applicationId,
            title: `${instance.name} ${existingCount + 1}`,
            state: app.createInitialState(),
            config: instance.config
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

      removeTab: async (workspaceId: string, tabId: string) => {
        const workspace = get().workspaces[workspaceId]
        if (!workspace) return

        // Find the tab to remove
        const tab = workspace.tabs.find((t) => t.id === tabId)
        if (!tab) return

        const app = applicationRegistry.get(tab.applicationId)
        if (!app) return

        // Check if app allows closing
        if (!app.canClose) return

        // Keep at least one tab of this type if app doesn't allow multiple
        if (!app.canHaveMultiple) {
          const sameTabs = workspace.tabs.filter((t) => t.applicationId === tab.applicationId)
          if (sameTabs.length <= 1) return
        }

        // Run cleanup
        if (app.cleanup) {
          await app.cleanup(tab, workspace)
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

      updateTabTitle: (workspaceId: string, tabId: string, title: string) => {
        set((state) => {
          const workspace = state.workspaces[workspaceId]
          if (!workspace) return state

          return {
            workspaces: {
              ...state.workspaces,
              [workspaceId]: {
                ...workspace,
                tabs: workspace.tabs.map((t) => (t.id === tabId ? { ...t, title } : t))
              }
            }
          }
        })
      },

      updateTabState: <T>(workspaceId: string, tabId: string, updater: (state: T) => T) => {
        set((state) => {
          const workspace = state.workspaces[workspaceId]
          if (!workspace) return state

          return {
            workspaces: {
              ...state.workspaces,
              [workspaceId]: {
                ...workspace,
                tabs: workspace.tabs.map((t) =>
                  t.id === tabId ? { ...t, state: updater(t.state as T) } : t
                )
              }
            }
          }
        })
      }
    }),
    {
      name: 'treeterm-workspaces',
      version: 2,
      migrate: (persistedState: unknown, version: number) => {
        const state = persistedState as { workspaces?: Record<string, unknown> }

        if (version === 0 && state.workspaces) {
          // Migrate from old format (terminals/activeTerminalId) to new format (tabs/activeTabId)
          for (const ws of Object.values(state.workspaces) as Array<{
            terminals?: Array<{ id: string; title: string; ptyId: string | null }>
            tabs?: Tab[]
            activeTerminalId?: string | null
            activeTabId?: string | null
          }>) {
            if (ws.terminals && !ws.tabs) {
              ws.tabs = ws.terminals.map((t) => ({
                id: t.id,
                applicationId: 'terminal',
                title: t.title,
                state: { ptyId: t.ptyId }
              }))
              delete ws.terminals
            }
            if (ws.activeTerminalId !== undefined && ws.activeTabId === undefined) {
              ws.activeTabId = ws.activeTerminalId
              delete ws.activeTerminalId
            }
          }
        }

        if (version <= 1 && state.workspaces) {
          // Migrate from version 1 (type-based tabs) to version 2 (application-based tabs)
          for (const ws of Object.values(state.workspaces) as Array<{
            tabs?: Array<{
              type?: string
              id: string
              title: string
              ptyId?: string | null
              applicationId?: string
              selectedPath?: string | null
              expandedDirs?: string[]
              state?: unknown
            }>
          }>) {
            if (ws.tabs) {
              ws.tabs = ws.tabs.map((t) => {
                // If already migrated (has state), skip
                if (t.state !== undefined && !t.type) return t as Tab

                if (t.type === 'terminal') {
                  return {
                    id: t.id,
                    applicationId: 'terminal',
                    title: t.title,
                    state: { ptyId: t.ptyId || null },
                    config: t.applicationId ? { instanceId: t.applicationId } : undefined
                  }
                } else if (t.type === 'filesystem') {
                  return {
                    id: t.id,
                    applicationId: 'filesystem',
                    title: t.title,
                    state: { selectedPath: t.selectedPath || null, expandedDirs: t.expandedDirs || [] }
                  }
                }
                return t as Tab
              })
            }
          }
        }

        return state as WorkspaceState
      }
    }
  )
)
