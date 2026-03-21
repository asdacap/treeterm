import { createStore } from 'zustand/vanilla'
import type { StoreApi } from 'zustand'
import type { Workspace, Tab, AppState, ReviewComment, AppRegistryApi, GitApi, FilesystemApi, WorkspaceGitApi, WorkspaceFilesystemApi } from '../types'
import { getTabs, isAiHarnessState } from '../types'
import type { Tty } from './createTtyStore'

export interface WorkspaceStoreDeps {
  appRegistry: AppRegistryApi
  getTty: (ptyId: string) => Tty | null
  git: GitApi
  filesystem: FilesystemApi
  // Session-level callbacks
  syncToDaemon: () => void
  removeWorkspace: (id: string) => Promise<void>
  removeWorkspaceKeepBranch: (id: string) => Promise<void>
  removeWorkspaceKeepWorktree: (id: string) => Promise<void>
  removeWorkspaceKeepBoth: (id: string) => Promise<void>
  mergeAndRemoveWorkspace: (id: string, squash: boolean) => Promise<{ success: boolean; error?: string }>
  closeAndCleanWorkspace: (id: string) => Promise<{ success: boolean; error?: string }>
  quickForkWorkspace: (id: string) => Promise<{ success: boolean; error?: string }>
  refreshGitInfo: (id: string) => Promise<void>
  lookupWorkspace: (id: string) => Workspace | undefined
}

export interface WorkspaceStoreState {
  workspace: Workspace

  // Tab methods
  addTab: <T>(applicationId: string, initialState?: Partial<T>) => string
  removeTab: (tabId: string) => Promise<void>
  setActiveTab: (tabId: string) => void
  updateTabTitle: (tabId: string, title: string) => void
  updateTabState: <T>(tabId: string, updater: (state: T) => T) => void

  // Review comments
  getReviewComments: () => ReviewComment[]
  addReviewComment: (comment: Omit<ReviewComment, 'id' | 'createdAt'>) => void
  deleteReviewComment: (commentId: string) => void
  toggleReviewCommentAddressed: (commentId: string) => void
  updateOutdatedReviewComments: (currentCommitHash: string) => void
  clearReviewComments: () => void

  // Other per-workspace
  promptHarness: (text: string) => boolean
  updateMetadata: (key: string, value: string) => void
  updateStatus: (status: Workspace['status']) => void

  // Git API (workspace-scoped)
  getGitApi: () => WorkspaceGitApi

  // Filesystem API (workspace-scoped)
  getFilesystemApi: () => WorkspaceFilesystemApi

  // Cross-cutting (delegate to session)
  refreshGitInfo: () => Promise<void>
  quickForkWorkspace: () => Promise<{ success: boolean; error?: string }>
  mergeAndRemove: (squash: boolean) => Promise<{ success: boolean; error?: string }>
  closeAndClean: () => Promise<{ success: boolean; error?: string }>
  remove: () => Promise<void>
  removeKeepBranch: () => Promise<void>
  removeKeepWorktree: () => Promise<void>
  removeKeepBoth: () => Promise<void>
  lookupWorkspace: (id: string) => Workspace | undefined
}

export type WorkspaceStore = StoreApi<WorkspaceStoreState>

function generateTabId(): string {
  return `tab-${Date.now()}-${Math.random().toString(36).slice(2, 9)}`
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

export function createWorkspaceStore(
  workspace: Workspace,
  deps: WorkspaceStoreDeps
): WorkspaceStore {
  const id = workspace.id

  function updateWorkspace(updater: (ws: Workspace) => Workspace): void {
    store.setState((state) => ({ workspace: updater(state.workspace) }))
  }

  const store = createStore<WorkspaceStoreState>()((set, get) => ({
    workspace,

    addTab: <T,>(applicationId: string, initialState?: Partial<T>): string => {
      const tabId = generateTabId()
      const app = deps.appRegistry.get(applicationId)
      if (!app) return tabId

      updateWorkspace((ws) => {
        if (!app.canHaveMultiple) {
          const existingEntry = Object.entries(ws.appStates).find(([, s]) => s.applicationId === applicationId)
          if (existingEntry) {
            if (initialState) {
              const [existingId, existingState] = existingEntry
              return {
                ...ws,
                appStates: {
                  ...ws.appStates,
                  [existingId]: { ...existingState, state: { ...(existingState.state || {}), ...initialState } }
                },
                activeTabId: existingId
              }
            }
            return ws
          }
        }

        const existingCount = Object.values(ws.appStates).filter(
          (s) => s.applicationId === applicationId
        ).length

        return {
          ...ws,
          appStates: {
            ...ws.appStates,
            [tabId]: {
              applicationId,
              title: `${app.name} ${existingCount + 1}`,
              state: initialState
                ? { ...(app.createInitialState() || {}), ...initialState }
                : app.createInitialState()
            }
          },
          activeTabId: tabId
        }
      })

      deps.syncToDaemon()
      return tabId
    },

    removeTab: async (tabId: string): Promise<void> => {
      const ws = get().workspace
      const appState = ws.appStates[tabId]
      if (!appState) return

      const app = deps.appRegistry.get(appState.applicationId)
      if (!app) return
      if (!app.canClose) return

      if (app.cleanup) {
        const tab: Tab = { ...appState, id: tabId }
        await app.cleanup(tab, ws)
      }

      updateWorkspace((ws) => {
        const { [tabId]: removed, ...remainingStates } = ws.appStates
        const remainingIds = Object.keys(remainingStates)
        let newActiveTabId = ws.activeTabId

        if (ws.activeTabId === tabId) {
          const allIds = Object.keys(ws.appStates)
          const removedIndex = allIds.indexOf(tabId)
          const newIndex = Math.min(removedIndex, remainingIds.length - 1)
          newActiveTabId = remainingIds[newIndex] || null
        }

        return {
          ...ws,
          appStates: remainingStates,
          activeTabId: newActiveTabId
        }
      })

      deps.syncToDaemon()
    },

    setActiveTab: (tabId: string): void => {
      updateWorkspace((ws) => ({ ...ws, activeTabId: tabId }))
      deps.syncToDaemon()
    },

    updateTabTitle: (tabId: string, title: string): void => {
      updateWorkspace((ws) => {
        if (!ws.appStates[tabId]) return ws
        return {
          ...ws,
          appStates: {
            ...ws.appStates,
            [tabId]: { ...ws.appStates[tabId], title }
          }
        }
      })
      deps.syncToDaemon()
    },

    updateTabState: <T,>(tabId: string, updater: (state: T) => T): void => {
      updateWorkspace((ws) => {
        if (!ws.appStates[tabId]) return ws
        const appState = ws.appStates[tabId]
        return {
          ...ws,
          appStates: {
            ...ws.appStates,
            [tabId]: { ...appState, state: updater(appState.state as T) }
          }
        }
      })
      // Only sync if the tab state contains a ptyId (persisted state)
      const appState = get().workspace.appStates[tabId]
      if (appState?.state && (appState.state as { ptyId?: string }).ptyId) {
        deps.syncToDaemon()
      }
    },

    updateMetadata: (key: string, value: string): void => {
      updateWorkspace((ws) => ({
        ...ws,
        metadata: { ...ws.metadata, [key]: value }
      }))
      deps.syncToDaemon()
    },

    updateStatus: (status: Workspace['status']): void => {
      updateWorkspace((ws) => ({ ...ws, status }))
      deps.syncToDaemon()
    },

    getReviewComments: (): ReviewComment[] => {
      return parseReviewComments(get().workspace.metadata)
    },

    addReviewComment: (comment: Omit<ReviewComment, 'id' | 'createdAt'>): void => {
      const comments = parseReviewComments(get().workspace.metadata)
      const newComment: ReviewComment = {
        ...comment,
        id: crypto.randomUUID(),
        createdAt: Date.now(),
      }
      comments.push(newComment)
      get().updateMetadata('reviewComments', serializeReviewComments(comments))
    },

    deleteReviewComment: (commentId: string): void => {
      const comments = parseReviewComments(get().workspace.metadata)
      const filtered = comments.filter(c => c.id !== commentId)
      get().updateMetadata('reviewComments', serializeReviewComments(filtered))
    },

    toggleReviewCommentAddressed: (commentId: string): void => {
      const comments = parseReviewComments(get().workspace.metadata)
      const updated = comments.map(c =>
        c.id === commentId ? { ...c, addressed: !c.addressed } : c
      )
      get().updateMetadata('reviewComments', serializeReviewComments(updated))
    },

    updateOutdatedReviewComments: (currentCommitHash: string): void => {
      const comments = parseReviewComments(get().workspace.metadata)
      if (comments.length === 0) return
      const updated = comments.map(comment => {
        const shouldBeOutdated = comment.commitHash !== currentCommitHash
        if (comment.isOutdated !== shouldBeOutdated) {
          return { ...comment, isOutdated: shouldBeOutdated }
        }
        return comment
      })
      get().updateMetadata('reviewComments', serializeReviewComments(updated))
    },

    clearReviewComments: (): void => {
      get().updateMetadata('reviewComments', serializeReviewComments([]))
    },

    promptHarness: (text: string): boolean => {
      const ws = get().workspace
      const tabs = getTabs(ws)
      let ptyId: string | null = null
      let tabId: string | null = null
      for (const tab of tabs) {
        if (tab.applicationId.startsWith('aiharness-') && isAiHarnessState(tab.state) && tab.state.ptyId !== null) {
          ptyId = tab.state.ptyId
          tabId = tab.id
          break
        }
      }

      if (!ptyId || !tabId) return false

      const tty = deps.getTty(ptyId)
      if (!tty) return false

      tty.getState().write(text + '\r')
      get().setActiveTab(tabId)
      return true
    },

    getGitApi: (): WorkspaceGitApi => {
      const path = get().workspace.path
      return {
        getInfo: () => deps.git.getInfo(path),
        createWorktree: (name, baseBranch?) => deps.git.createWorktree(path, name, baseBranch),
        removeWorktree: (worktreePath, deleteBranch?) => deps.git.removeWorktree(path, worktreePath, deleteBranch),
        listWorktrees: () => deps.git.listWorktrees(path),
        getChildWorktrees: (parentBranch) => deps.git.getChildWorktrees(path, parentBranch),
        listLocalBranches: () => deps.git.listLocalBranches(path),
        listRemoteBranches: () => deps.git.listRemoteBranches(path),
        getBranchesInWorktrees: () => deps.git.getBranchesInWorktrees(path),
        createWorktreeFromBranch: (branch, worktreeName) => deps.git.createWorktreeFromBranch(path, branch, worktreeName),
        createWorktreeFromRemote: (remoteBranch, worktreeName) => deps.git.createWorktreeFromRemote(path, remoteBranch, worktreeName),
        getDiff: (parentBranch) => deps.git.getDiff(path, parentBranch),
        getFileDiff: (parentBranch, filePath) => deps.git.getFileDiff(path, parentBranch, filePath),
        checkMergeConflicts: (sourceBranch, targetBranch) => deps.git.checkMergeConflicts(path, sourceBranch, targetBranch),
        merge: (worktreeBranch, targetBranch, squash?) => deps.git.merge(path, worktreeBranch, targetBranch, squash),
        hasUncommittedChanges: () => deps.git.hasUncommittedChanges(path),
        commitAll: (message) => deps.git.commitAll(path, message),
        deleteBranch: (branchName) => deps.git.deleteBranch(path, branchName),
        getUncommittedChanges: () => deps.git.getUncommittedChanges(path),
        getUncommittedFileDiff: (filePath, staged) => deps.git.getUncommittedFileDiff(path, filePath, staged),
        stageFile: (filePath) => deps.git.stageFile(path, filePath),
        unstageFile: (filePath) => deps.git.unstageFile(path, filePath),
        stageAll: () => deps.git.stageAll(path),
        unstageAll: () => deps.git.unstageAll(path),
        commitStaged: (message) => deps.git.commitStaged(path, message),
        getFileContentsForDiff: (parentBranch, filePath) => deps.git.getFileContentsForDiff(path, parentBranch, filePath),
        getUncommittedFileContentsForDiff: (filePath, staged) => deps.git.getUncommittedFileContentsForDiff(path, filePath, staged),
        getHeadCommitHash: () => deps.git.getHeadCommitHash(path),
      }
    },

    getFilesystemApi: (): WorkspaceFilesystemApi => {
      const path = get().workspace.path
      return {
        readDirectory: (dirPath) => deps.filesystem.readDirectory(path, dirPath),
        readFile: (filePath) => deps.filesystem.readFile(path, filePath),
        writeFile: (filePath, content) => deps.filesystem.writeFile(path, filePath, content),
        searchFiles: (query) => deps.filesystem.searchFiles(path, query),
      }
    },

    // Cross-cutting operations — delegate to session
    refreshGitInfo: () => deps.refreshGitInfo(id),
    quickForkWorkspace: () => deps.quickForkWorkspace(id),
    mergeAndRemove: (squash: boolean) => deps.mergeAndRemoveWorkspace(id, squash),
    closeAndClean: () => deps.closeAndCleanWorkspace(id),
    remove: () => deps.removeWorkspace(id),
    removeKeepBranch: () => deps.removeWorkspaceKeepBranch(id),
    removeKeepWorktree: () => deps.removeWorkspaceKeepWorktree(id),
    removeKeepBoth: () => deps.removeWorkspaceKeepBoth(id),
    lookupWorkspace: (otherId: string) => deps.lookupWorkspace(otherId),
  }))

  return store
}
