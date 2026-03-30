import { createStore } from 'zustand/vanilla'
import type { StoreApi } from 'zustand'
import type { Workspace, AppRef, AppRegistryApi, GitApi, FilesystemApi, RunActionsApi, WorkspaceGitApi, WorkspaceFilesystemApi, LlmApi, Settings, ActivityState, WorktreeSettings, SandboxConfig, GitHubApi } from '../types'
import { getTabs, isAiHarnessState } from '../types'
import type { Tty, TtyWriter } from './createTtyStore'
import { createAnalyzerStore } from './createAnalyzerStore'
import type { Analyzer } from './createAnalyzerStore'
import { createGitControllerStore } from './createGitControllerStore'
import type { GitController } from './createGitControllerStore'
import { createReviewCommentStore } from './createReviewCommentStore'
import type { ReviewCommentStore } from './createReviewCommentStore'

export interface WorkspaceStoreDeps {
  appRegistry: AppRegistryApi
  openTtyStream: (ptyId: string) => Promise<{ tty: Tty }>
  getTtyWriter: (ptyId: string) => Promise<TtyWriter>
  createTty: (cwd: string, sandbox?: SandboxConfig, startupCommand?: string) => Promise<string>
  connectionId: string
  git: GitApi
  filesystem: FilesystemApi
  runActions: RunActionsApi
  getSettings: () => Settings
  llm: Pick<LlmApi, 'analyzeTerminal' | 'generateTitle'>
  setActivityTabState: (tabId: string, state: ActivityState) => void
  // Session-level callbacks
  syncToDaemon: () => void
  removeWorkspace: (id: string) => Promise<void>
  removeWorkspaceKeepBranch: (id: string) => Promise<void>
  removeWorkspaceKeepBoth: (id: string) => Promise<void>
  mergeAndRemoveWorkspace: (id: string, squash: boolean) => Promise<{ success: boolean; error?: string }>
  mergeAndKeepWorkspace: (id: string, squash: boolean) => Promise<{ success: boolean; error?: string }>
  closeAndCleanWorkspace: (id: string) => Promise<{ success: boolean; error?: string }>
  quickForkWorkspace: (id: string) => Promise<{ success: boolean; error?: string }>
  refreshGitInfo: (id: string) => Promise<void>
  lookupWorkspace: (id: string) => Workspace | undefined
  github: GitHubApi
}

export interface WorkspaceStoreState {
  workspace: Workspace

  // Tab methods
  addTab: <T>(applicationId: string, initialState?: Partial<T>) => string
  removeTab: (tabId: string) => Promise<void>
  setActiveTab: (tabId: string) => void
  updateTabTitle: (tabId: string, title: string) => void
  updateTabState: <T>(tabId: string, updater: (state: T) => T) => void

  // Review comment controller
  reviewComments: ReviewCommentStore

  // Tab lifecycle
  initTab: (tabId: string) => void
  getTabRef: (tabId: string) => AppRef | null

  // Analyzer factory (used by applications in onWorkspaceLoad)
  initAnalyzer: (tabId: string) => Analyzer

  // PTY creation (delegated from session)
  createTty: (cwd: string, sandbox?: SandboxConfig, startupCommand?: string) => Promise<string>
  connectionId: string

  // Git controller (polling, diff status, PR status)
  gitController: GitController

  // Focus signal (ephemeral, not persisted)
  focusTabId: string | null
  requestFocus: () => void
  clearFocusRequest: () => void

  // Other per-workspace
  promptHarness: (text: string) => Promise<boolean>
  updateMetadata: (key: string, value: string) => void
  updateSettings: (settings: Partial<WorktreeSettings>) => void
  updateStatus: (status: Workspace['status']) => void

  // Git API (workspace-scoped)
  getGitApi: () => WorkspaceGitApi

  // Filesystem API (workspace-scoped)
  getFilesystemApi: () => WorkspaceFilesystemApi

  // Run Actions API (connection-bound)
  getRunActionsApi: () => RunActionsApi

  // Cross-cutting (delegate to session)
  refreshGitInfo: () => Promise<void>
  quickForkWorkspace: () => Promise<{ success: boolean; error?: string }>
  mergeAndRemove: (squash: boolean) => Promise<{ success: boolean; error?: string }>
  mergeAndKeep: (squash: boolean) => Promise<{ success: boolean; error?: string }>
  closeAndClean: () => Promise<{ success: boolean; error?: string }>
  remove: () => Promise<void>
  removeKeepBranch: () => Promise<void>
  removeKeepBoth: () => Promise<void>
  lookupWorkspace: (id: string) => Workspace | undefined
}

export type WorkspaceStore = StoreApi<WorkspaceStoreState>

function generateTabId(): string {
  return `tab-${Date.now()}-${Math.random().toString(36).slice(2, 9)}`
}

export function createWorkspaceStore(
  workspace: Workspace,
  deps: WorkspaceStoreDeps
): WorkspaceStore {
  const id = workspace.id

  // Declare store early so sub-stores can reference it lazily via callbacks
  let store!: WorkspaceStore

  function updateWorkspace(updater: (ws: Workspace) => Workspace): void {
    store.setState((state) => ({ workspace: updater(state.workspace) }))
  }

  // Closure-level tab ref registry (non-serialized per-tab runtime state)
  const tabRefs: Record<string, AppRef> = {}

  const gitController = createGitControllerStore({
    git: deps.git,
    github: deps.github,
    lookupWorkspace: deps.lookupWorkspace,
    refreshGitInfo: () => deps.refreshGitInfo(id),
    getWorkspace: () => store.getState().workspace,
    initialWorkspace: workspace,
  })

  const reviewComments = createReviewCommentStore({
    getMetadata: () => store.getState().workspace.metadata,
    updateMetadata: (key, value) => store.getState().updateMetadata(key, value),
  })

  store = createStore<WorkspaceStoreState>()((set, get) => ({
    workspace,

    gitController,
    reviewComments,

    initTab: (tabId: string): void => {
      const appState = get().workspace.appStates[tabId]
      if (!appState) return
      const app = deps.appRegistry.get(appState.applicationId)
      if (!app) return
      tabRefs[tabId] = app.onWorkspaceLoad({ ...appState, id: tabId }, store)
    },

    getTabRef: (tabId: string): AppRef | null => tabRefs[tabId] ?? null,

    initAnalyzer: (tabId: string): Analyzer => createAnalyzerStore(tabId, {
      getSettings: deps.getSettings,
      llm: deps.llm,
      updateMetadata: (key, value) => get().updateMetadata(key, value),
      getDisplayName: () => get().workspace.metadata?.displayName,
      getDescription: () => get().workspace.metadata?.description,
      setActivityTabState: deps.setActivityTabState,
      openTtyStream: deps.openTtyStream,
      cwd: get().workspace.path,
      renameBranch: async (oldName, newName) => {
        await deps.git.renameBranch(get().workspace.gitRootPath!, oldName, newName)
        await deps.refreshGitInfo(id)
      },
      getGitBranch: () => get().workspace.gitBranch,
      getBranchIsUserDefined: () => get().workspace.metadata?.branchIsUserDefined === 'true',
      getParentId: () => get().workspace.parentId,
      refreshGitInfo: () => deps.refreshGitInfo(id),
      refreshDiffStatus: () => gitController.getState().refreshDiffStatus(),
    }),

    createTty: (cwd: string, sandbox?: SandboxConfig, startupCommand?: string) =>
      deps.createTty(cwd, sandbox, startupCommand),

    connectionId: deps.connectionId,

    addTab: <T,>(applicationId: string, initialState?: Partial<T>): string => {
      const tabId = generateTabId()
      const app = deps.appRegistry.get(applicationId)
      if (!app) return tabId

      updateWorkspace((ws) => {
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
      get().initTab(tabId)
      return tabId
    },

    removeTab: async (tabId: string): Promise<void> => {
      const ws = get().workspace
      const appState = ws.appStates[tabId]
      if (!appState) return

      const app = deps.appRegistry.get(appState.applicationId)
      if (!app) return
      if (!app.canClose) return

      // Dispose tab ref (stops analyzer, kills PTY, etc.)
      const ref = tabRefs[tabId]
      if (ref) {
        ref.dispose()
        delete tabRefs[tabId]
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

    focusTabId: null,
    requestFocus: (): void => set({ focusTabId: get().workspace.activeTabId }),
    clearFocusRequest: (): void => set({ focusTabId: null }),

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

    updateSettings: (newSettings: Partial<WorktreeSettings>): void => {
      updateWorkspace((ws) => ({
        ...ws,
        settings: { ...(ws.settings ?? {}), ...newSettings } as WorktreeSettings
      }))
      deps.syncToDaemon()
    },

    updateStatus: (status: Workspace['status']): void => {
      updateWorkspace((ws) => ({ ...ws, status }))
      deps.syncToDaemon()
    },

    promptHarness: async (text: string): Promise<boolean> => {
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

      const writer = await deps.getTtyWriter(ptyId)
      writer.write(text + '\r')
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
        listLocalBranches: () => deps.git.listLocalBranches(path),
        listRemoteBranches: () => deps.git.listRemoteBranches(path),
        getBranchesInWorktrees: () => deps.git.getBranchesInWorktrees(path),
        createWorktreeFromBranch: (branch, worktreeName) => deps.git.createWorktreeFromBranch(path, branch, worktreeName),
        createWorktreeFromRemote: (remoteBranch, worktreeName) => deps.git.createWorktreeFromRemote(path, remoteBranch, worktreeName),
        getDiff: (parentBranch) => deps.git.getDiff(path, parentBranch),
        getFileDiff: (parentBranch, filePath) => deps.git.getFileDiff(path, parentBranch, filePath),
        checkMergeConflicts: (sourceBranch, targetBranch) => deps.git.checkMergeConflicts(path, sourceBranch, targetBranch),
        merge: (worktreeBranch, squash?) => deps.git.merge(path, worktreeBranch, squash),
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
        getLog: (parentBranch, skip, limit) => deps.git.getLog(path, parentBranch, skip, limit),
        getCommitDiff: (commitHash) => deps.git.getCommitDiff(path, commitHash),
        getCommitFileDiff: (commitHash, filePath) => deps.git.getCommitFileDiff(path, commitHash, filePath),
        fetch: () => deps.git.fetch(path),
        pull: () => deps.git.pull(path),
        getBehindCount: () => deps.git.getBehindCount(path),
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

    getRunActionsApi: (): RunActionsApi => deps.runActions,

    // Cross-cutting operations — delegate to session
    refreshGitInfo: () => deps.refreshGitInfo(id),
    quickForkWorkspace: () => deps.quickForkWorkspace(id),
    mergeAndRemove: (squash: boolean) => deps.mergeAndRemoveWorkspace(id, squash),
    mergeAndKeep: (squash: boolean) => deps.mergeAndKeepWorkspace(id, squash),
    closeAndClean: () => deps.closeAndCleanWorkspace(id),
    remove: () => deps.removeWorkspace(id),
    removeKeepBranch: () => deps.removeWorkspaceKeepBranch(id),
    removeKeepBoth: () => deps.removeWorkspaceKeepBoth(id),
    lookupWorkspace: (otherId: string) => deps.lookupWorkspace(otherId),
  }))

  gitController.getState().startPolling()

  return store
}
