import { createStore } from 'zustand/vanilla'
import type { StoreApi } from 'zustand'
import { humanId } from 'human-id'
import type { Workspace, GitInfo, Tab, AppState, WorktreeSettings, GitApi, SessionApi, Settings, AppRegistryApi, Application, ReviewComment } from '../types'

export interface WorkspaceDeps {
  git: GitApi
  session: SessionApi
  getSettings: () => Settings
  appRegistry: AppRegistryApi
}

export interface WorkspaceState {
  workspaces: Record<string, Workspace>
  activeWorkspaceId: string | null
  isRestoring: boolean  // Set externally by appStore during restoration; checked by syncSessionToDaemon
  addWorkspace: (path: string, options?: { skipDefaultTabs?: boolean; settings?: WorktreeSettings }) => Promise<string>
  addChildWorkspace: (parentId: string, name: string, isDetached?: boolean, settings?: WorktreeSettings, description?: string) => Promise<{ success: boolean; error?: string }>
  adoptExistingWorktree: (parentId: string, worktreePath: string, branch: string, name: string, settings?: WorktreeSettings, description?: string) => Promise<{ success: boolean; error?: string }>
  createWorktreeFromBranch: (parentId: string, branch: string, isDetached: boolean, settings?: WorktreeSettings, description?: string) => Promise<{ success: boolean; error?: string }>
  createWorktreeFromRemote: (parentId: string, remoteBranch: string, isDetached: boolean, settings?: WorktreeSettings, description?: string) => Promise<{ success: boolean; error?: string }>
  removeWorkspace: (id: string) => Promise<void>
  removeWorkspaceKeepBranch: (id: string) => Promise<void>
  removeWorkspaceKeepWorktree: (id: string) => Promise<void>
  removeWorkspaceKeepBoth: (id: string) => Promise<void>
  removeOrphanWorkspace: (id: string) => void
  mergeAndRemoveWorkspace: (id: string, squash: boolean) => Promise<{ success: boolean; error?: string }>
  closeAndCleanWorkspace: (id: string) => Promise<{ success: boolean; error?: string }>
  setActiveWorkspace: (id: string | null) => void
  updateGitInfo: (id: string, gitInfo: GitInfo) => void
  refreshGitInfo: (id: string) => Promise<void>
  updateWorkspaceStatus: (id: string, status: Workspace['status']) => void
  addTab: (workspaceId: string, applicationId: string) => string
  addTabWithState: <T>(workspaceId: string, applicationId: string, initialState: Partial<T>, existingTabId?: string) => string
  removeTab: (workspaceId: string, tabId: string) => Promise<void>
  setActiveTab: (workspaceId: string, tabId: string) => void
  updateTabTitle: (workspaceId: string, tabId: string, title: string) => void
  updateTabState: <T>(workspaceId: string, tabId: string, updater: (state: T) => T) => void
  updateWorkspaceMetadata: (id: string, key: string, value: string) => void
  getReviewComments: (workspaceId: string) => ReviewComment[]
  addReviewComment: (workspaceId: string, comment: Omit<ReviewComment, 'id' | 'createdAt'>) => void
  deleteReviewComment: (workspaceId: string, commentId: string) => void
  toggleReviewCommentAddressed: (workspaceId: string, commentId: string) => void
  updateOutdatedReviewComments: (workspaceId: string, currentCommitHash: string) => void
  clearReviewComments: (workspaceId: string) => void
  quickForkWorkspace: (workspaceId: string) => Promise<{ success: boolean; error?: string }>
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

export function parseReviewComments(metadata: Record<string, string>): ReviewComment[] {
  if (!metadata.reviewComments) return []
  try {
    return JSON.parse(metadata.reviewComments)
  } catch {
    return []
  }
}

function serializeReviewComments(comments: ReviewComment[]): string {
  return JSON.stringify(comments)
}

function getDefaultAppForWorktree(
  deps: Pick<WorkspaceDeps, 'appRegistry' | 'getSettings'>,
  settings?: WorktreeSettings,
  parentSettings?: WorktreeSettings
): Application | null | undefined {
  if (settings?.defaultApplicationId) {
    const app = deps.appRegistry.get(settings.defaultApplicationId)
    if (app) return app
  }
  if (parentSettings?.defaultApplicationId) {
    const app = deps.appRegistry.get(parentSettings.defaultApplicationId)
    if (app) return app
  }
  const globalSettings = deps.getSettings()
  if (globalSettings.globalDefaultApplicationId) {
    const app = deps.appRegistry.get(globalSettings.globalDefaultApplicationId)
    if (app) return app
  }
  return deps.appRegistry.getDefaultApp()
}

/**
 * Helper function to find unmerged sub-workspaces (worktrees with status 'active')
 */
export function getUnmergedSubWorkspaces(workspaces: Record<string, Workspace>): Workspace[] {
  return Object.values(workspaces).filter(
    (ws) => ws.isWorktree && ws.status === 'active'
  )
}

export function createWorkspaceStore(
  config: { sessionId: string; windowUuid: string | null },
  deps: WorkspaceDeps
): StoreApi<WorkspaceState> {
  let syncDebounceTimer: ReturnType<typeof setTimeout> | null = null

  async function syncSessionToDaemon(
    workspaces: Record<string, Workspace>,
    isRestoring: boolean = false
  ) {
    try {
      const settings = deps.getSettings()
      console.log('[workspace] syncSessionToDaemon called - workspaces:', Object.keys(workspaces).length, 'isRestoring:', isRestoring)

      if (isRestoring) {
        console.log('[workspace] currently restoring, skipping sync')
        return
      }

      const daemonWorkspaces = Object.values(workspaces).map(({ createdAt, lastActivity, ...ws }) => ws)

      console.log('[workspace] syncing to daemon:', daemonWorkspaces.length, 'workspaces')

      if (daemonWorkspaces.length === 0) {
        if (config.sessionId) {
          console.log('[workspace] deleting session:', config.sessionId)
          await deps.session.delete(config.sessionId)
        }
        return
      }

      console.log('[workspace] updating session:', config.sessionId, 'senderUuid:', config.windowUuid)
      const result = await deps.session.update(config.sessionId, daemonWorkspaces, config.windowUuid || undefined)
      if (!result.success) {
        console.error('[workspace] failed to update session:', result.error)
      } else {
        console.log('[workspace] session updated successfully')
      }
    } catch (error) {
      console.error('[workspace] failed to sync session to daemon:', error)
    }
  }

  function debouncedSyncSessionToDaemon(
    workspaces: Record<string, Workspace>,
    isRestoring: boolean = false
  ) {
    if (syncDebounceTimer) {
      clearTimeout(syncDebounceTimer)
    }
    syncDebounceTimer = setTimeout(() => {
      syncSessionToDaemon(workspaces, isRestoring)
    }, 500)
  }

  // Shared helper: creates a child workspace from a git operation result and updates state
  async function addChildWorkspaceFromResult(
    parentId: string,
    name: string,
    path: string,
    branch: string,
    options: { isDetached?: boolean; isWorktree?: boolean; settings?: WorktreeSettings; metadata?: Record<string, string> } = {}
  ): Promise<string> {
    const parent = store.getState().workspaces[parentId]

    const id = generateId()
    const appStates: Record<string, AppState> = {}
    let activeTabId: string | null = null

    const defaultApp = getDefaultAppForWorktree(deps, options.settings, parent?.settings)
    if (defaultApp) {
      const tabId = generateTabId()
      appStates[tabId] = {
        applicationId: defaultApp.id,
        title: defaultApp.name,
        state: defaultApp.createInitialState()
      }
      activeTabId = tabId
    }

    const childWorkspace: Workspace = {
      id,
      name,
      path,
      parentId,
      children: [],
      status: 'active',
      isGitRepo: true,
      gitBranch: branch,
      gitRootPath: parent?.gitRootPath ?? null,
      isWorktree: options.isWorktree ?? true,
      isDetached: options.isDetached,
      appStates,
      activeTabId,
      settings: options.settings,
      metadata: options.metadata ?? {},
      createdAt: Date.now(),
      lastActivity: Date.now(),
    }

    store.setState((state) => ({
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

    const currentState = store.getState()
    await syncSessionToDaemon(currentState.workspaces, currentState.isRestoring)

    return id
  }

  // Shared helper: removes a workspace with configurable git cleanup behavior
  async function removeWorkspaceInternal(
    id: string,
    options: { keepBranch: boolean; keepWorktree: boolean }
  ): Promise<void> {
    const state = store.getState()
    const workspace = state.workspaces[id]

    if (!workspace) return

    for (const childId of workspace.children) {
      await removeWorkspaceInternal(childId, options)
    }

    for (const [tabId, appState] of Object.entries(workspace.appStates)) {
      const tab: Tab = { ...appState, id: tabId }
      const app = deps.appRegistry.get(tab.applicationId)
      if (app?.cleanup) {
        await app.cleanup(tab, workspace)
      }
    }

    if (workspace.isWorktree && workspace.gitRootPath) {
      if (!options.keepWorktree) {
        const deleteBranch = !options.keepBranch && !workspace.isDetached
        await deps.git.removeWorktree(
          workspace.gitRootPath,
          workspace.path,
          deleteBranch
        )
      } else if (!options.keepBranch && !workspace.isDetached && workspace.gitBranch) {
        await deps.git.deleteBranch(workspace.gitRootPath, workspace.gitBranch)
      }
    }

    if (workspace.parentId) {
      store.setState((state) => {
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

    store.setState((state) => {
      const { [id]: removed, ...rest } = state.workspaces
      return {
        workspaces: rest,
        activeWorkspaceId: state.activeWorkspaceId === id ? null : state.activeWorkspaceId
      }
    })

    const currentState = store.getState()
    await syncSessionToDaemon(currentState.workspaces, currentState.isRestoring)
  }

  const store = createStore<WorkspaceState>()((set, get) => ({
    workspaces: {},
    activeWorkspaceId: null,
    isRestoring: false,

    addWorkspace: async (path: string, options?: { skipDefaultTabs?: boolean; settings?: WorktreeSettings }) => {
      console.log('[workspace] addWorkspace called for path:', path)
      const id = generateId()

      const gitInfo = await deps.git.getInfo(path)

      const appStates: Record<string, AppState> = {}
      let activeTabId: string | null = null

      if (!options?.skipDefaultTabs) {
        const defaultApp = getDefaultAppForWorktree(deps, options?.settings, undefined)
        if (defaultApp) {
          const tabId = generateTabId()
          appStates[tabId] = {
            applicationId: defaultApp.id,
            title: defaultApp.name,
            state: defaultApp.createInitialState()
          }
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
        appStates,
        activeTabId,
        settings: options?.settings,
        metadata: {},
        createdAt: Date.now(),
        lastActivity: Date.now(),
        }

      set((state) => ({
        workspaces: { ...state.workspaces, [id]: workspace },
        activeWorkspaceId: id
      }))

      const state = get()
      await syncSessionToDaemon(state.workspaces, state.isRestoring)

      return id
    },

    addChildWorkspace: async (parentId: string, name: string, isDetached: boolean = false, settings?: WorktreeSettings, description?: string) => {
      const state = get()
      const parent = state.workspaces[parentId]

      if (!parent) {
        return { success: false, error: 'Parent workspace not found' }
      }

      if (!parent.isGitRepo || !parent.gitRootPath) {
        return { success: false, error: 'Parent workspace is not a git repository' }
      }

      const currentGitInfo = await deps.git.getInfo(parent.path)
      const currentBranch = currentGitInfo.branch

      const result = await deps.git.createWorktree(
        parent.gitRootPath,
        name,
        currentBranch || undefined
      )

      if (!result.success) {
        return { success: false, error: result.error }
      }

      if (currentBranch && currentBranch !== parent.gitBranch) {
        get().updateGitInfo(parentId, currentGitInfo)
      }

      const metadata = description ? { description } : undefined
      await addChildWorkspaceFromResult(parentId, name, result.path!, result.branch!, { isDetached, settings, metadata })
      return { success: true }
    },

    adoptExistingWorktree: async (parentId: string, worktreePath: string, branch: string, name: string, settings?: WorktreeSettings, description?: string) => {
      const state = get()
      const parent = state.workspaces[parentId]

      if (!parent) {
        return { success: false, error: 'Parent workspace not found' }
      }

      const existingWorkspace = Object.values(state.workspaces).find(
        ws => ws.path === worktreePath
      )
      if (existingWorkspace) {
        return { success: false, error: 'This worktree is already open' }
      }

      const metadata = description ? { description } : undefined
      await addChildWorkspaceFromResult(parentId, name, worktreePath, branch, { settings, metadata })
      return { success: true }
    },

    createWorktreeFromBranch: async (parentId: string, branch: string, isDetached: boolean, settings?: WorktreeSettings, description?: string) => {
      console.log('[workspace] createWorktreeFromBranch called:', { parentId, branch, isDetached })
      const state = get()
      const parent = state.workspaces[parentId]

      if (!parent) {
        return { success: false, error: 'Parent workspace not found' }
      }

      if (!parent.isGitRepo || !parent.gitRootPath) {
        return { success: false, error: 'Parent workspace is not a git repository' }
      }

      const worktreeName = branch.split('/').pop() || branch
      const result = await deps.git.createWorktreeFromBranch(
        parent.gitRootPath,
        branch,
        worktreeName
      )

      if (!result.success) {
        return { success: false, error: result.error }
      }

      const metadata = description ? { description } : undefined
      await addChildWorkspaceFromResult(parentId, worktreeName, result.path!, result.branch!, { isDetached, settings, metadata })
      return { success: true }
    },

    createWorktreeFromRemote: async (parentId: string, remoteBranch: string, isDetached: boolean, settings?: WorktreeSettings, description?: string) => {
      console.log('[workspace] createWorktreeFromRemote called:', { parentId, remoteBranch, isDetached })
      const state = get()
      const parent = state.workspaces[parentId]

      if (!parent) {
        return { success: false, error: 'Parent workspace not found' }
      }

      if (!parent.isGitRepo || !parent.gitRootPath) {
        return { success: false, error: 'Parent workspace is not a git repository' }
      }

      const worktreeName = remoteBranch.split('/').pop() || remoteBranch
      const result = await deps.git.createWorktreeFromRemote(
        parent.gitRootPath,
        remoteBranch,
        worktreeName
      )

      if (!result.success) {
        return { success: false, error: result.error }
      }

      const metadata = description ? { description } : undefined
      await addChildWorkspaceFromResult(parentId, worktreeName, result.path!, result.branch!, { isDetached, settings, metadata })
      return { success: true }
    },

    removeWorkspace: async (id: string) => {
      await removeWorkspaceInternal(id, { keepBranch: false, keepWorktree: false })
    },

    removeWorkspaceKeepBranch: async (id: string) => {
      await removeWorkspaceInternal(id, { keepBranch: true, keepWorktree: false })
    },

    removeWorkspaceKeepWorktree: async (id: string) => {
      await removeWorkspaceInternal(id, { keepBranch: false, keepWorktree: true })
    },

    removeWorkspaceKeepBoth: async (id: string) => {
      await removeWorkspaceInternal(id, { keepBranch: true, keepWorktree: true })
    },

    // Remove workspace from local state only — no git ops, no daemon sync.
    // Called when receiving a daemon update (e.g. another window merged/abandoned this worktree).
    removeOrphanWorkspace: (id: string) => {
      const state = get()
      const workspace = state.workspaces[id]
      if (!workspace) return

      if (workspace.parentId) {
        const parent = state.workspaces[workspace.parentId]
        if (parent) {
          set({
            workspaces: {
              ...state.workspaces,
              [workspace.parentId]: {
                ...parent,
                children: parent.children.filter((cid) => cid !== id)
              }
            }
          })
        }
      }

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
      const state = get()
      syncSessionToDaemon(state.workspaces, state.isRestoring).catch(console.error)
    },

    refreshGitInfo: async (id: string) => {
      const state = get()
      const workspace = state.workspaces[id]
      if (!workspace) return

      const gitInfo = await deps.git.getInfo(workspace.path)
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

      const state = get()
      syncSessionToDaemon(state.workspaces, state.isRestoring).catch(console.error)
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

      const hasChanges = await deps.git.hasUncommittedChanges(workspace.path)
      if (hasChanges) {
        const commitResult = await deps.git.commitAll(
          workspace.path,
          `WIP: Auto-commit before merge from ${workspace.name}`
        )
        if (!commitResult.success) {
          return { success: false, error: `Failed to commit changes: ${commitResult.error}` }
        }
      }

      const mergeResult = await deps.git.merge(
        parent.gitRootPath,
        workspace.gitBranch!,
        parent.gitBranch,
        squash
      )

      if (!mergeResult.success) {
        return { success: false, error: `Merge failed: ${mergeResult.error}` }
      }

      get().updateWorkspaceStatus(id, 'merged')
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

      await get().removeWorkspace(id)

      return { success: true }
    },

    addTab: (workspaceId: string, applicationId: string) => {
      const tabId = generateTabId()
      const app = deps.appRegistry.get(applicationId)

      if (!app) return tabId

      set((state) => {
        const workspace = state.workspaces[workspaceId]
        if (!workspace) return state

        if (!app.canHaveMultiple) {
          const existing = Object.values(workspace.appStates).find((s) => s.applicationId === applicationId)
          if (existing) return state
        }

        const existingCount = Object.values(workspace.appStates).filter(
          (s) => s.applicationId === applicationId
        ).length

        return {
          workspaces: {
            ...state.workspaces,
            [workspaceId]: {
              ...workspace,
              appStates: {
                ...workspace.appStates,
                [tabId]: {
                  applicationId,
                  title: `${app.name} ${existingCount + 1}`,
                  state: app.createInitialState()
                }
              },
              activeTabId: tabId
            }
          }
        }
      })

      const state = get()
      syncSessionToDaemon(state.workspaces, state.isRestoring).catch(console.error)

      return tabId
    },

    addTabWithState: <T>(workspaceId: string, applicationId: string, initialState: Partial<T>, existingTabId?: string) => {
      const tabId = existingTabId || generateTabId()
      const app = deps.appRegistry.get(applicationId)

      if (!app) return tabId

      set((state) => {
        const workspace = state.workspaces[workspaceId]
        if (!workspace) return state

        if (existingTabId && workspace.appStates[existingTabId]) {
          const existing = workspace.appStates[existingTabId]
          return {
            workspaces: {
              ...state.workspaces,
              [workspaceId]: {
                ...workspace,
                appStates: {
                  ...workspace.appStates,
                  [existingTabId]: { ...existing, state: { ...(existing.state || {}), ...(initialState || {}) } }
                },
                activeTabId: existingTabId
              }
            }
          }
        }

        if (!app.canHaveMultiple) {
          const existingEntry = Object.entries(workspace.appStates).find(([, s]) => s.applicationId === applicationId)
          if (existingEntry) {
            const [existingId, existingState] = existingEntry
            return {
              workspaces: {
                ...state.workspaces,
                [workspaceId]: {
                  ...workspace,
                  appStates: {
                    ...workspace.appStates,
                    [existingId]: { ...existingState, state: { ...(existingState.state || {}), ...(initialState || {}) } }
                  },
                  activeTabId: existingId
                }
              }
            }
          }
        }

        const existingCount = Object.values(workspace.appStates).filter(
          (s) => s.applicationId === applicationId
        ).length

        return {
          workspaces: {
            ...state.workspaces,
            [workspaceId]: {
              ...workspace,
              appStates: {
                ...workspace.appStates,
                [tabId]: {
                  applicationId,
                  title: `${app.name} ${existingCount + 1}`,
                  state: { ...(app.createInitialState() || {}), ...(initialState || {}) }
                }
              },
              activeTabId: tabId
            }
          }
        }
      })

      const state = get()
      syncSessionToDaemon(state.workspaces, state.isRestoring).catch(console.error)

      return tabId
    },

    removeTab: async (workspaceId: string, tabId: string) => {
      const workspace = get().workspaces[workspaceId]
      if (!workspace) return

      const appState = workspace.appStates[tabId]
      if (!appState) return

      const app = deps.appRegistry.get(appState.applicationId)
      if (!app) return

      if (!app.canClose) return

      if (app.cleanup) {
        const tab: Tab = { ...appState, id: tabId }
        await app.cleanup(tab, workspace)
      }

      set((state) => {
        const workspace = state.workspaces[workspaceId]
        if (!workspace) return state

        const { [tabId]: removed, ...remainingStates } = workspace.appStates
        const remainingIds = Object.keys(remainingStates)
        let newActiveTabId = workspace.activeTabId

        if (workspace.activeTabId === tabId) {
          const allIds = Object.keys(workspace.appStates)
          const removedIndex = allIds.indexOf(tabId)
          const newIndex = Math.min(removedIndex, remainingIds.length - 1)
          newActiveTabId = remainingIds[newIndex] || null
        }

        return {
          workspaces: {
            ...state.workspaces,
            [workspaceId]: {
              ...workspace,
              appStates: remainingStates,
              activeTabId: newActiveTabId
            }
          }
        }
      })

      const state = get()
      syncSessionToDaemon(state.workspaces, state.isRestoring).catch(console.error)
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

      const state = get()
      debouncedSyncSessionToDaemon(state.workspaces, state.isRestoring)
    },

    updateTabTitle: (workspaceId: string, tabId: string, title: string) => {
      set((state) => {
        const workspace = state.workspaces[workspaceId]
        if (!workspace || !workspace.appStates[tabId]) return state

        return {
          workspaces: {
            ...state.workspaces,
            [workspaceId]: {
              ...workspace,
              appStates: {
                ...workspace.appStates,
                [tabId]: { ...workspace.appStates[tabId], title }
              }
            }
          }
        }
      })
      const state = get()
      syncSessionToDaemon(state.workspaces, state.isRestoring).catch(console.error)
    },

    updateTabState: <T>(workspaceId: string, tabId: string, updater: (state: T) => T) => {
      set((state) => {
        const workspace = state.workspaces[workspaceId]
        if (!workspace || !workspace.appStates[tabId]) return state

        const appState = workspace.appStates[tabId]
        return {
          workspaces: {
            ...state.workspaces,
            [workspaceId]: {
              ...workspace,
              appStates: {
                ...workspace.appStates,
                [tabId]: { ...appState, state: updater(appState.state as T) }
              }
            }
          }
        }
      })
      const updated = get()
      const appState = updated.workspaces[workspaceId]?.appStates[tabId]
      if (appState?.state && (appState.state as { ptyId?: string }).ptyId) {
        syncSessionToDaemon(updated.workspaces, updated.isRestoring).catch(console.error)
      }
    },

    updateWorkspaceMetadata: (id: string, key: string, value: string) => {
      set((state) => {
        const workspace = state.workspaces[id]
        if (!workspace) return state
        return {
          workspaces: {
            ...state.workspaces,
            [id]: {
              ...workspace,
              metadata: { ...workspace.metadata, [key]: value }
            }
          }
        }
      })
      const state = get()
      syncSessionToDaemon(state.workspaces, state.isRestoring).catch(console.error)
    },

    getReviewComments: (workspaceId: string): ReviewComment[] => {
      const workspace = get().workspaces[workspaceId]
      if (!workspace) return []
      return parseReviewComments(workspace.metadata)
    },

    addReviewComment: (workspaceId: string, comment: Omit<ReviewComment, 'id' | 'createdAt'>) => {
      const workspace = get().workspaces[workspaceId]
      if (!workspace) return
      const comments = parseReviewComments(workspace.metadata)
      const newComment: ReviewComment = {
        ...comment,
        id: crypto.randomUUID(),
        createdAt: Date.now(),
      }
      comments.push(newComment)
      get().updateWorkspaceMetadata(workspaceId, 'reviewComments', serializeReviewComments(comments))
    },

    deleteReviewComment: (workspaceId: string, commentId: string) => {
      const workspace = get().workspaces[workspaceId]
      if (!workspace) return
      const comments = parseReviewComments(workspace.metadata)
      const filtered = comments.filter(c => c.id !== commentId)
      get().updateWorkspaceMetadata(workspaceId, 'reviewComments', serializeReviewComments(filtered))
    },

    toggleReviewCommentAddressed: (workspaceId: string, commentId: string) => {
      const workspace = get().workspaces[workspaceId]
      if (!workspace) return
      const comments = parseReviewComments(workspace.metadata)
      const updated = comments.map(c =>
        c.id === commentId ? { ...c, addressed: !c.addressed } : c
      )
      get().updateWorkspaceMetadata(workspaceId, 'reviewComments', serializeReviewComments(updated))
    },

    updateOutdatedReviewComments: (workspaceId: string, currentCommitHash: string) => {
      const workspace = get().workspaces[workspaceId]
      if (!workspace) return
      const comments = parseReviewComments(workspace.metadata)
      if (comments.length === 0) return
      const updated = comments.map(comment => {
        const shouldBeOutdated = comment.commitHash !== currentCommitHash
        if (comment.isOutdated !== shouldBeOutdated) {
          return { ...comment, isOutdated: shouldBeOutdated }
        }
        return comment
      })
      get().updateWorkspaceMetadata(workspaceId, 'reviewComments', serializeReviewComments(updated))
    },

    clearReviewComments: (workspaceId: string) => {
      get().updateWorkspaceMetadata(workspaceId, 'reviewComments', serializeReviewComments([]))
    },

    quickForkWorkspace: async (workspaceId: string) => {
      const state = get()
      const ws = state.workspaces[workspaceId]

      if (!ws) {
        return { success: false, error: 'Workspace not found' }
      }

      if (!ws.gitRootPath) {
        return { success: false, error: 'Workspace has no git root path' }
      }

      const existingBranches = await deps.git.listLocalBranches(ws.gitRootPath)
      const parentBranch = ws.gitBranch || ''

      let name: string | null = null
      for (let i = 0; i < 3; i++) {
        const candidate = humanId({ separator: '-', capitalize: false })
        const fullBranch = parentBranch ? `${parentBranch}/${candidate}` : candidate
        if (!existingBranches.includes(fullBranch)) {
          name = candidate
          break
        }
      }

      if (!name) {
        return { success: false, error: 'Failed to generate unique branch name' }
      }

      return get().addChildWorkspace(workspaceId, name, false)
    },

    syncToDaemon: async () => {
      const state = get()
      await syncSessionToDaemon(state.workspaces, state.isRestoring)
    }
  }))

  return store
}
