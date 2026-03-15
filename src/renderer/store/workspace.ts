import { create } from 'zustand'
import type { Workspace, GitInfo, Tab, WorktreeSettings } from '../types'
import type { Application } from '../types'
import { applicationRegistry } from '../registry/applicationRegistry'
import { useSettingsStore } from '../store/settings'

interface WorkspaceState {
  workspaces: Record<string, Workspace>
  activeWorkspaceId: string | null
  sessionId: string | null  // Daemon session ID for syncing
  isRestoring: boolean  // Flag to prevent syncing during restoration
  windowUuid: string | null  // This window's UUID for session sync deduplication
  addWorkspace: (path: string, options?: { skipDefaultTabs?: boolean; settings?: WorktreeSettings }) => Promise<string>
  addChildWorkspace: (parentId: string, name: string, isDetached?: boolean, settings?: WorktreeSettings) => Promise<{ success: boolean; error?: string }>
  adoptExistingWorktree: (parentId: string, worktreePath: string, branch: string, name: string, settings?: WorktreeSettings) => Promise<{ success: boolean; error?: string }>
  createWorktreeFromBranch: (parentId: string, branch: string, isDetached: boolean, settings?: WorktreeSettings) => Promise<{ success: boolean; error?: string }>
  createWorktreeFromRemote: (parentId: string, remoteBranch: string, isDetached: boolean, settings?: WorktreeSettings) => Promise<{ success: boolean; error?: string }>
  removeWorkspace: (id: string) => Promise<void>
  removeWorkspaceKeepBranch: (id: string) => Promise<void>
  removeWorkspaceKeepWorktree: (id: string) => Promise<void>
  mergeAndRemoveWorkspace: (id: string, squash: boolean) => Promise<{ success: boolean; error?: string }>
  closeAndCleanWorkspace: (id: string) => Promise<{ success: boolean; error?: string }>
  setActiveWorkspace: (id: string | null) => void
  updateGitInfo: (id: string, gitInfo: GitInfo) => void
  refreshGitInfo: (id: string) => Promise<void>
  updateWorkspaceStatus: (id: string, status: Workspace['status']) => void
  // Tab management (application-agnostic)
  addTab: (workspaceId: string, applicationId: string) => string
  addTabWithState: <T>(workspaceId: string, applicationId: string, initialState: Partial<T>, existingTabId?: string) => string
  removeTab: (workspaceId: string, tabId: string) => Promise<void>
  setActiveTab: (workspaceId: string, tabId: string) => void
  updateTabTitle: (workspaceId: string, tabId: string, title: string) => void
  updateTabState: <T>(workspaceId: string, tabId: string, updater: (state: T) => T) => void
  syncToDaemon: () => Promise<void>
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

// Helper to get default application based on worktree settings
// Priority: worktree settings > parent settings > global settings > fallback
function getDefaultAppForWorktree(
  settings?: WorktreeSettings,
  parentSettings?: WorktreeSettings
): Application | null {
  // Check worktree's own settings first
  if (settings?.defaultApplicationId) {
    const app = applicationRegistry.get(settings.defaultApplicationId)
    if (app) return app
  }
  
  // Fall back to parent's settings
  if (parentSettings?.defaultApplicationId) {
    const app = applicationRegistry.get(parentSettings.defaultApplicationId)
    if (app) return app
  }
  
  // Fall back to global settings
  const { settings: globalSettings } = useSettingsStore.getState()
  if (globalSettings.globalDefaultApplicationId) {
    const app = applicationRegistry.get(globalSettings.globalDefaultApplicationId)
    if (app) return app
  }
  
  // Final fallback: first available app
  return applicationRegistry.getDefaultApp()
}

// Helper to sync entire workspace tree to daemon as a session (if daemon is enabled)
async function syncSessionToDaemon(
  sessionId: string | null,
  workspaces: Record<string, Workspace>,
  setSessionId: (id: string) => void,
  isRestoring: boolean = false
) {
  try {
    const { settings } = useSettingsStore.getState()
    console.log('[workspace] syncSessionToDaemon called - daemon enabled:', settings.daemon.enabled, 'workspaces:', Object.keys(workspaces).length, 'isRestoring:', isRestoring)

    if (isRestoring) {
      console.log('[workspace] currently restoring, skipping sync')
      return
    }

    if (!settings.daemon.enabled) {
      console.log('[workspace] daemon not enabled, skipping sync')
      return
    }

    // Strip daemon-managed fields when syncing to daemon
    const daemonWorkspaces = Object.values(workspaces).map(({ createdAt, lastActivity, attachedClients, ...ws }) => ws)

    console.log('[workspace] syncing to daemon:', daemonWorkspaces.length, 'workspaces')

    if (daemonWorkspaces.length === 0) {
      // No workspaces - delete session if it exists
      if (sessionId) {
        console.log('[workspace] deleting session:', sessionId)
        await window.electron.session.delete(sessionId)
        // Note: sessionId will be set to null by the caller after deletion
      }
      return
    }

    if (sessionId) {
      // Update existing session, passing window UUID so daemon doesn't echo back to us
      const windowUuid = useWorkspaceStore.getState().windowUuid
      console.log('[workspace] updating session:', sessionId, 'senderUuid:', windowUuid)
      const result = await window.electron.session.update(sessionId, daemonWorkspaces, windowUuid || undefined)
      if (!result.success) {
        console.error('[workspace] failed to update session:', result.error)
      } else {
        console.log('[workspace] session updated successfully')
      }
    } else {
      // No sessionId - this should never happen now since daemon always provides one
      console.error('[workspace] no sessionId available - daemon should have provided one at startup')
      // Do not create a new session - the daemon owns session creation
    }
  } catch (error) {
    console.error('[workspace] failed to sync session to daemon:', error)
  }
}

// Debounced sync for frequently-called methods
let syncDebounceTimer: ReturnType<typeof setTimeout> | null = null

function debouncedSyncSessionToDaemon(
  sessionId: string | null,
  workspaces: Record<string, Workspace>,
  setSessionId: (id: string) => void,
  isRestoring: boolean = false
) {
  if (syncDebounceTimer) {
    clearTimeout(syncDebounceTimer)
  }
  syncDebounceTimer = setTimeout(() => {
    syncSessionToDaemon(sessionId, workspaces, setSessionId, isRestoring)
  }, 500)
}

export const useWorkspaceStore = create<WorkspaceState>()(
  (set, get) => ({
      workspaces: {},
      activeWorkspaceId: null,
      sessionId: null,
      isRestoring: false,
      windowUuid: null,

      addWorkspace: async (path: string, options?: { skipDefaultTabs?: boolean; settings?: WorktreeSettings }) => {
        console.log('[workspace] addWorkspace called for path:', path)
        const id = generateId()

        // Get git info for the path
        const gitInfo = await window.electron.git.getInfo(path)

        // Create tabs based on default applications from registry
        const tabs: Tab[] = []
        let activeTabId: string | null = null

        // Only create default tabs if not skipped (e.g., when restoring from daemon)
        if (!options?.skipDefaultTabs) {
          // Get default app based on provided settings (for root worktrees)
          const defaultApp = getDefaultAppForWorktree(options?.settings, undefined)

          if (defaultApp) {
            const tabId = generateTabId()
            tabs.push({
              id: tabId,
              applicationId: defaultApp.id,
              title: defaultApp.name,
              state: defaultApp.createInitialState()
            })
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
          activeTabId,
          settings: options?.settings,
          createdAt: Date.now(),
          lastActivity: Date.now(),
          attachedClients: 0
        }

        set((state) => ({
          workspaces: { ...state.workspaces, [id]: workspace },
          activeWorkspaceId: id
        }))

        // Sync to daemon
        console.log('[workspace] about to sync to daemon')
        const state = get()
        console.log('[workspace] current sessionId:', state.sessionId, 'workspaces count:', Object.keys(state.workspaces).length)
        await syncSessionToDaemon(state.sessionId, state.workspaces, (sid) =>
          set({ sessionId: sid }), state.isRestoring
        )
        console.log('[workspace] sync complete')

        return id
      },

      addChildWorkspace: async (parentId: string, name: string, isDetached: boolean = false, settings?: WorktreeSettings) => {
        const state = get()
        const parent = state.workspaces[parentId]

        if (!parent) {
          return { success: false, error: 'Parent workspace not found' }
        }

        if (!parent.isGitRepo || !parent.gitRootPath) {
          return { success: false, error: 'Parent workspace is not a git repository' }
        }

        // Get current branch from git (in case it changed via terminal)
        const currentGitInfo = await window.electron.git.getInfo(parent.path)
        const currentBranch = currentGitInfo.branch

        // Create worktree
        const result = await window.electron.git.createWorktree(
          parent.gitRootPath,
          name,
          currentBranch || undefined
        )

        if (!result.success) {
          return { success: false, error: result.error }
        }

        // Update parent's stored branch if it changed
        if (currentBranch && currentBranch !== parent.gitBranch) {
          get().updateGitInfo(parentId, currentGitInfo)
        }

        // Create child workspace
        const id = generateId()

        // Create tabs based on default applications from registry
        const tabs: Tab[] = []
        let activeTabId: string | null = null

        // Get default app based on settings inheritance
        const defaultApp = getDefaultAppForWorktree(settings, parent.settings)

        if (defaultApp) {
          const tabId = generateTabId()
          tabs.push({
            id: tabId,
            applicationId: defaultApp.id,
            title: defaultApp.name,
            state: defaultApp.createInitialState()
          })
          activeTabId = tabId
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
          isDetached,
          tabs,
          activeTabId,
          settings,
          createdAt: Date.now(),
          lastActivity: Date.now(),
          attachedClients: 0
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

        // Sync to daemon
        const currentState = get()
        await syncSessionToDaemon(currentState.sessionId, currentState.workspaces, (sid) =>
          set({ sessionId: sid }), currentState.isRestoring
        )

        return { success: true }
      },

      adoptExistingWorktree: async (parentId: string, worktreePath: string, branch: string, name: string, settings?: WorktreeSettings) => {
        const state = get()
        const parent = state.workspaces[parentId]

        if (!parent) {
          return { success: false, error: 'Parent workspace not found' }
        }

        // Check if this worktree is already open in TreeTerm
        const existingWorkspace = Object.values(state.workspaces).find(
          ws => ws.path === worktreePath
        )
        if (existingWorkspace) {
          return { success: false, error: 'This worktree is already open' }
        }

        // Create workspace for existing worktree
        const id = generateId()

        // Create tabs based on default applications from registry
        const tabs: Tab[] = []
        let activeTabId: string | null = null

        // Get default app based on settings inheritance
        const defaultApp = getDefaultAppForWorktree(settings, parent.settings)

        if (defaultApp) {
          const tabId = generateTabId()
          tabs.push({
            id: tabId,
            applicationId: defaultApp.id,
            title: defaultApp.name,
            state: defaultApp.createInitialState()
          })
          activeTabId = tabId
        }

        const childWorkspace: Workspace = {
          id,
          name,
          path: worktreePath,
          parentId,
          children: [],
          status: 'active',
          isGitRepo: true,
          gitBranch: branch,
          gitRootPath: parent.gitRootPath,
          isWorktree: true,
          tabs,
          activeTabId,
          settings,
          createdAt: Date.now(),
          lastActivity: Date.now(),
          attachedClients: 0
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

        // Sync to daemon
        const currentState = get()
        await syncSessionToDaemon(currentState.sessionId, currentState.workspaces, (sid) =>
          set({ sessionId: sid }), currentState.isRestoring
        )

        return { success: true }
      },

      createWorktreeFromBranch: async (parentId: string, branch: string, isDetached: boolean, settings?: WorktreeSettings) => {
        console.log('[workspace] createWorktreeFromBranch called:', { parentId, branch, isDetached })
        const state = get()
        const parent = state.workspaces[parentId]

        if (!parent) {
          console.error('[workspace] Parent workspace not found:', parentId)
          return { success: false, error: 'Parent workspace not found' }
        }

        if (!parent.isGitRepo || !parent.gitRootPath) {
          console.error('[workspace] Parent workspace is not a git repository')
          return { success: false, error: 'Parent workspace is not a git repository' }
        }

        // Extract simple name from branch for worktree naming
        const worktreeName = branch.split('/').pop() || branch
        console.log('[workspace] Creating worktree with name:', worktreeName)

        // Create worktree from existing branch
        const result = await window.electron.git.createWorktreeFromBranch(
          parent.gitRootPath,
          branch,
          worktreeName
        )
        console.log('[workspace] createWorktreeFromBranch result:', result)

        if (!result.success) {
          return { success: false, error: result.error }
        }

        // Create child workspace
        const id = generateId()

        // Create tabs based on default applications from registry
        const tabs: Tab[] = []
        let activeTabId: string | null = null

        // Get default app based on settings inheritance
        const defaultApp = getDefaultAppForWorktree(settings, parent.settings)

        if (defaultApp) {
          const tabId = generateTabId()
          tabs.push({
            id: tabId,
            applicationId: defaultApp.id,
            title: defaultApp.name,
            state: defaultApp.createInitialState()
          })
          activeTabId = tabId
        }

        const childWorkspace: Workspace = {
          id,
          name: worktreeName,
          path: result.path!,
          parentId,
          children: [],
          status: 'active',
          isGitRepo: true,
          gitBranch: result.branch!,
          gitRootPath: parent.gitRootPath,
          isWorktree: true,
          isDetached,
          tabs,
          activeTabId,
          settings,
          createdAt: Date.now(),
          lastActivity: Date.now(),
          attachedClients: 0
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

        // Sync to daemon
        const currentState = get()
        await syncSessionToDaemon(currentState.sessionId, currentState.workspaces, (sid) =>
          set({ sessionId: sid }), currentState.isRestoring
        )

        return { success: true }
      },

      createWorktreeFromRemote: async (parentId: string, remoteBranch: string, isDetached: boolean, settings?: WorktreeSettings) => {
        console.log('[workspace] createWorktreeFromRemote called:', { parentId, remoteBranch, isDetached })
        const state = get()
        const parent = state.workspaces[parentId]

        if (!parent) {
          console.error('[workspace] Parent workspace not found:', parentId)
          return { success: false, error: 'Parent workspace not found' }
        }

        if (!parent.isGitRepo || !parent.gitRootPath) {
          console.error('[workspace] Parent workspace is not a git repository')
          return { success: false, error: 'Parent workspace is not a git repository' }
        }

        // Extract simple name from remote branch for worktree naming
        const worktreeName = remoteBranch.split('/').pop() || remoteBranch
        console.log('[workspace] Creating worktree from remote with name:', worktreeName)

        // Create worktree from remote branch
        const result = await window.electron.git.createWorktreeFromRemote(
          parent.gitRootPath,
          remoteBranch,
          worktreeName
        )
        console.log('[workspace] createWorktreeFromRemote result:', result)

        if (!result.success) {
          return { success: false, error: result.error }
        }

        // Create child workspace
        const id = generateId()

        // Create tabs based on default applications from registry
        const tabs: Tab[] = []
        let activeTabId: string | null = null

        // Get default app based on settings inheritance
        const defaultApp = getDefaultAppForWorktree(settings, parent.settings)

        if (defaultApp) {
          const tabId = generateTabId()
          tabs.push({
            id: tabId,
            applicationId: defaultApp.id,
            title: defaultApp.name,
            state: defaultApp.createInitialState()
          })
          activeTabId = tabId
        }

        const childWorkspace: Workspace = {
          id,
          name: worktreeName,
          path: result.path!,
          parentId,
          children: [],
          status: 'active',
          isGitRepo: true,
          gitBranch: result.branch!,
          gitRootPath: parent.gitRootPath,
          isWorktree: true,
          isDetached,
          tabs,
          activeTabId,
          settings,
          createdAt: Date.now(),
          lastActivity: Date.now(),
          attachedClients: 0
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

        // Sync to daemon
        const currentState = get()
        await syncSessionToDaemon(currentState.sessionId, currentState.workspaces, (sid) =>
          set({ sessionId: sid }), currentState.isRestoring
        )

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
          // For detached worktrees, keep the branch. For normal worktrees, delete it.
          const deleteBranch = !workspace.isDetached
          await window.electron.git.removeWorktree(
            workspace.gitRootPath,
            workspace.path,
            deleteBranch
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

        // Sync to daemon (after removing workspace)
        const currentState = get()
        await syncSessionToDaemon(currentState.sessionId, currentState.workspaces, (sid) =>
          set({ sessionId: sid }), currentState.isRestoring
        )
      },

      removeWorkspaceKeepBranch: async (id: string) => {
        const state = get()
        const workspace = state.workspaces[id]

        if (!workspace) return

        // Recursively remove children first
        for (const childId of workspace.children) {
          await get().removeWorkspaceKeepBranch(childId)
        }

        // Run cleanup for all tabs using the application registry
        for (const tab of workspace.tabs) {
          const app = applicationRegistry.get(tab.applicationId)
          if (app?.cleanup) {
            await app.cleanup(tab, workspace)
          }
        }

        // If this is a worktree, remove it from git but keep the branch
        if (workspace.isWorktree && workspace.gitRootPath) {
          await window.electron.git.removeWorktree(
            workspace.gitRootPath,
            workspace.path,
            false  // Always keep the branch
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

        // Sync to daemon (after removing workspace)
        const currentState = get()
        await syncSessionToDaemon(currentState.sessionId, currentState.workspaces, (sid) =>
          set({ sessionId: sid }), currentState.isRestoring
        )
      },

      removeWorkspaceKeepWorktree: async (id: string) => {
        const state = get()
        const workspace = state.workspaces[id]

        if (!workspace) return

        // Recursively remove children first
        for (const childId of workspace.children) {
          await get().removeWorkspaceKeepWorktree(childId)
        }

        // Run cleanup for all tabs using the application registry
        for (const tab of workspace.tabs) {
          const app = applicationRegistry.get(tab.applicationId)
          if (app?.cleanup) {
            await app.cleanup(tab, workspace)
          }
        }

        // DO NOT remove from git - just remove from TreeTerm's tracking

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

        // Sync to daemon (after removing workspace)
        const currentState = get()
        await syncSessionToDaemon(currentState.sessionId, currentState.workspaces, (sid) =>
          set({ sessionId: sid }), currentState.isRestoring
        )
      },

      setActiveWorkspace: (id: string | null) => {
        set({ activeWorkspaceId: id })
        // Don't sync active workspace changes - they're too frequent
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
        const state = get()
        syncSessionToDaemon(state.sessionId, state.workspaces, (sid) =>
          set({ sessionId: sid }), state.isRestoring
        ).catch(console.error)
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

        // Sync to daemon
        const state = get()
        syncSessionToDaemon(state.sessionId, state.workspaces, (sid) =>
          set({ sessionId: sid }), state.isRestoring
        ).catch(console.error)
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

        // Update workspace status
        get().updateWorkspaceStatus(id, 'merged')

        // Remove the workspace
        await get().removeWorkspace(id)

        return { success: true }
      },

      closeAndCleanWorkspace: async (id: string) => {
        const state = get()
        const workspace = state.workspaces[id]

        if (!workspace) {
          return { success: false, error: 'Workspace not found' }
        }

        if (!workspace.isWorktree || !workspace.parentId) {
          return { success: false, error: 'Not a worktree workspace' }
        }

        const parent = state.workspaces[workspace.parentId]
        if (!parent || !parent.gitRootPath) {
          return { success: false, error: 'Parent workspace not found or not a git repo' }
        }

        // Remove the workspace (this will also remove the worktree but keep the branch)
        await get().removeWorkspace(id)

        return { success: true }
      },

      // Tab management (application-agnostic)
      addTab: (workspaceId: string, applicationId: string) => {
        const tabId = generateTabId()
        const app = applicationRegistry.get(applicationId)

        if (!app) return tabId

        set((state) => {
          const workspace = state.workspaces[workspaceId]
          if (!workspace) return state

          // Check if app allows multiple instances
          if (!app.canHaveMultiple) {
            const existing = workspace.tabs.find((t) => t.applicationId === applicationId)
            if (existing) return state
          }

          const existingCount = workspace.tabs.filter(
            (t) => t.applicationId === applicationId
          ).length

          const newTab: Tab = {
            id: tabId,
            applicationId,
            title: `${app.name} ${existingCount + 1}`,
            state: app.createInitialState()
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

        // Sync to daemon
        const state = get()
        syncSessionToDaemon(state.sessionId, state.workspaces, (sid) =>
          set({ sessionId: sid }), state.isRestoring
        ).catch(console.error)

        return tabId
      },

      addTabWithState: <T>(workspaceId: string, applicationId: string, initialState: Partial<T>, existingTabId?: string) => {
        // Use existing tab ID if provided (for session restoration), otherwise generate new one
        const tabId = existingTabId || generateTabId()
        const app = applicationRegistry.get(applicationId)

        if (!app) return tabId

        set((state) => {
          const workspace = state.workspaces[workspaceId]
          if (!workspace) return state

          // Check if this tab already exists (by ID) - prevents duplicates during session restoration
          if (existingTabId && workspace.tabs.some((t) => t.id === existingTabId)) {
            // Tab already exists, update its state
            const updatedTabs = workspace.tabs.map((t) =>
              t.id === existingTabId ? { ...t, state: { ...(t.state || {}), ...(initialState || {}) } } : t
            )
            return {
              workspaces: {
                ...state.workspaces,
                [workspaceId]: {
                  ...workspace,
                  tabs: updatedTabs,
                  activeTabId: existingTabId
                }
              }
            }
          }

          // Check if app allows multiple instances
          if (!app.canHaveMultiple) {
            const existing = workspace.tabs.find((t) => t.applicationId === applicationId)
            if (existing) {
              // If tab exists, update its state and activate it
              const updatedTabs = workspace.tabs.map((t) =>
                t.id === existing.id ? { ...t, state: { ...(t.state || {}), ...(initialState || {}) } } : t
              )
              return {
                workspaces: {
                  ...state.workspaces,
                  [workspaceId]: {
                    ...workspace,
                    tabs: updatedTabs,
                    activeTabId: existing.id
                  }
                }
              }
            }
          }

          const existingCount = workspace.tabs.filter(
            (t) => t.applicationId === applicationId
          ).length

          const newTab: Tab = {
            id: tabId,
            applicationId,
            title: `${app.name} ${existingCount + 1}`,
            state: { ...(app.createInitialState() || {}), ...(initialState || {}) }
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

        // Sync to daemon
        const state = get()
        syncSessionToDaemon(state.sessionId, state.workspaces, (sid) =>
          set({ sessionId: sid }), state.isRestoring
        ).catch(console.error)

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

        // Sync to daemon
        const state = get()
        syncSessionToDaemon(state.sessionId, state.workspaces, (sid) =>
          set({ sessionId: sid }), state.isRestoring
        ).catch(console.error)
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

        // Sync to daemon
        const state = get()
        syncSessionToDaemon(state.sessionId, state.workspaces, (sid) =>
          set({ sessionId: sid }), state.isRestoring
        ).catch(console.error)
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
        const state = get()
        syncSessionToDaemon(state.sessionId, state.workspaces, (sid) =>
          set({ sessionId: sid }), state.isRestoring
        ).catch(console.error)
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
        // Only sync to daemon when a ptyId is present — never sync with empty pty
        const updated = get()
        const tab = updated.workspaces[workspaceId]?.tabs.find((t) => t.id === tabId)
        if (tab?.state && (tab.state as { ptyId?: string }).ptyId) {
          syncSessionToDaemon(updated.sessionId, updated.workspaces, (sid) =>
            set({ sessionId: sid }), updated.isRestoring
          ).catch(console.error)
        }
      },

      syncToDaemon: async () => {
        const state = get()
        await syncSessionToDaemon(state.sessionId, state.workspaces, (sid) =>
          set({ sessionId: sid }), state.isRestoring
        )
      }
  })
)

/**
 * Helper function to find unmerged sub-workspaces (worktrees with status 'active')
 */
export function getUnmergedSubWorkspaces(workspaces: Record<string, Workspace>): Workspace[] {
  return Object.values(workspaces).filter(
    (ws) => ws.isWorktree && ws.status === 'active'
  )
}
