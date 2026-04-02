import { createStore } from 'zustand/vanilla'
import type { StoreApi } from 'zustand'
import type { Workspace, AppRef, AppRegistryApi, GitApi, FilesystemApi, RunActionsApi, WorkspaceGitApi, WorkspaceFilesystemApi, LlmApi, Settings, ActivityState, WorktreeSettings, SandboxConfig, GitHubApi, PtyEvent } from '../types'
import { getTabs, isAiHarnessState } from '../types'
import type { Terminal as XTerm } from '@xterm/xterm'
import type { Tty, TtyWriter } from './createTtyStore'
import { createAnalyzerStore } from './createAnalyzerStore'
import type { Analyzer } from './createAnalyzerStore'
import { createGitControllerStore } from './createGitControllerStore'
import type { GitController } from './createGitControllerStore'
import { createReviewCommentStore } from './createReviewCommentStore'
import type { ReviewCommentStore } from './createReviewCommentStore'

/**
 * Cached xterm.js terminal for BaseTerminal-derived tabs only (Terminal, AiHarness).
 * Survives component mount/unmount to avoid re-streaming PTY data on tab switch.
 * NOT serialized — purely in-memory, renderer-side cache.
 */
export interface CachedTerminal {
  terminal: XTerm
  tty: Tty
  /** Unsubscribe the background TTY event subscription (called only on dispose) */
  unsubscribeEvents: () => void
  /** Set by BaseTerminal on mount, cleared to null on unmount.
   *  When set, all events forward to this handler for full UI handling.
   *  When null, the background fallback writes data to the terminal buffer. */
  mountedHandler: ((event: PtyEvent) => void) | null
  /** Config flag for background handler — strips CSI 3J from PTY data */
  stripScrollbackClear: boolean
  /** Timestamp when the TTY stream was opened (for immediate-failure detection) */
  connectedAt: number
  /** Persistent data version counter (survives across mounts) */
  dataVersion: number
  /** Exit handling when component is unmounted */
  onExitUnmounted: (exitCode: number) => void
}

export interface WorkspaceStoreDeps {
  appRegistry: AppRegistryApi
  openTtyStream: (ptyId: string) => Promise<{ tty: Tty }>
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

  // Terminal cache for BaseTerminal-derived tabs (Terminal, AiHarness only).
  // Caches xterm.js Terminal + TTY subscription across mount/unmount cycles.
  getCachedTerminal: (tabId: string) => CachedTerminal | null
  setCachedTerminal: (tabId: string, entry: CachedTerminal) => void
  /** Dispose a single cached terminal (unsubscribe events, dispose xterm, remove from cache) */
  disposeCachedTerminal: (tabId: string) => void
  /** Dispose all cached terminals. Called by session store on workspace removal. */
  disposeAllCachedTerminals: () => void

  // Analyzer factory (used by applications in onWorkspaceLoad)
  initAnalyzer: (tabId: string) => Analyzer

  // PTY creation (delegated from session)
  createTty: (cwd: string, sandbox?: SandboxConfig, startupCommand?: string) => Promise<string>
  // Write-only PTY access (cached per workspace, separate stream from terminal events)
  getTtyWriter: (ptyId: string) => Promise<TtyWriter>
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

  // Git API (workspace-scoped, created once at init)
  gitApi: WorkspaceGitApi

  // Filesystem API (workspace-scoped, created once at init)
  filesystemApi: WorkspaceFilesystemApi

  // Run Actions API (connection-bound)
  runActionsApi: RunActionsApi

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

  // Cached xterm.js Terminal instances for BaseTerminal-derived tabs only
  // (Terminal, AiHarness). Survives mount/unmount; disposed on removeTab.
  const cachedTerminals: Record<string, CachedTerminal> = {}

  // Write-only PTY handles, cached per workspace (separate stream from terminal events)
  const ttyWriters: Record<string, TtyWriter> = {}

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

    getCachedTerminal: (tabId: string): CachedTerminal | null => cachedTerminals[tabId] ?? null,
    setCachedTerminal: (tabId: string, entry: CachedTerminal): void => { cachedTerminals[tabId] = entry },
    disposeCachedTerminal: (tabId: string): void => {
      const cached = cachedTerminals[tabId]
      if (cached) {
        cached.mountedHandler = null
        cached.unsubscribeEvents()
        cached.terminal.dispose()
        delete cachedTerminals[tabId]
      }
    },
    disposeAllCachedTerminals: (): void => {
      for (const tabId of Object.keys(cachedTerminals)) {
        get().disposeCachedTerminal(tabId)
      }
    },

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

    getTtyWriter: async (ptyId: string): Promise<TtyWriter> => {
      const cached = ttyWriters[ptyId]
      if (cached) return cached
      const { tty } = await deps.openTtyStream(ptyId)
      const state = tty.getState()
      const writer: TtyWriter = { write: state.write, kill: state.kill }
      ttyWriters[ptyId] = writer
      return writer
    },

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

      // Dispose cached terminal (unsubscribe background events, dispose xterm)
      get().disposeCachedTerminal(tabId)

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

      const writer = await get().getTtyWriter(ptyId)
      writer.write(text + '\r')
      get().setActiveTab(tabId)
      return true
    },

    gitApi: {
      getInfo: () => deps.git.getInfo(workspace.path),
      createWorktree: (name, baseBranch?) => deps.git.createWorktree(workspace.path, name, baseBranch),
      removeWorktree: (worktreePath, deleteBranch?) => deps.git.removeWorktree(workspace.path, worktreePath, deleteBranch),
      listWorktrees: () => deps.git.listWorktrees(workspace.path),
      listLocalBranches: () => deps.git.listLocalBranches(workspace.path),
      listRemoteBranches: () => deps.git.listRemoteBranches(workspace.path),
      getBranchesInWorktrees: () => deps.git.getBranchesInWorktrees(workspace.path),
      createWorktreeFromBranch: (branch, worktreeName) => deps.git.createWorktreeFromBranch(workspace.path, branch, worktreeName),
      createWorktreeFromRemote: (remoteBranch, worktreeName) => deps.git.createWorktreeFromRemote(workspace.path, remoteBranch, worktreeName),
      getDiff: (parentBranch) => deps.git.getDiff(workspace.path, parentBranch),
      getFileDiff: (parentBranch, filePath) => deps.git.getFileDiff(workspace.path, parentBranch, filePath),
      checkMergeConflicts: (sourceBranch, targetBranch) => deps.git.checkMergeConflicts(workspace.path, sourceBranch, targetBranch),
      merge: (worktreeBranch, squash?) => deps.git.merge(workspace.path, worktreeBranch, squash),
      hasUncommittedChanges: () => deps.git.hasUncommittedChanges(workspace.path),
      commitAll: (message) => deps.git.commitAll(workspace.path, message),
      deleteBranch: (branchName) => deps.git.deleteBranch(workspace.path, branchName),
      getUncommittedChanges: () => deps.git.getUncommittedChanges(workspace.path),
      getUncommittedFileDiff: (filePath, staged) => deps.git.getUncommittedFileDiff(workspace.path, filePath, staged),
      stageFile: (filePath) => deps.git.stageFile(workspace.path, filePath),
      unstageFile: (filePath) => deps.git.unstageFile(workspace.path, filePath),
      stageAll: () => deps.git.stageAll(workspace.path),
      unstageAll: () => deps.git.unstageAll(workspace.path),
      commitStaged: (message) => deps.git.commitStaged(workspace.path, message),
      getFileContentsForDiff: (parentBranch, filePath) => deps.git.getFileContentsForDiff(workspace.path, parentBranch, filePath),
      getUncommittedFileContentsForDiff: (filePath, staged) => deps.git.getUncommittedFileContentsForDiff(workspace.path, filePath, staged),
      getHeadCommitHash: () => deps.git.getHeadCommitHash(workspace.path),
      getLog: (parentBranch, skip, limit) => deps.git.getLog(workspace.path, parentBranch, skip, limit),
      getCommitDiff: (commitHash) => deps.git.getCommitDiff(workspace.path, commitHash),
      getCommitFileDiff: (commitHash, filePath) => deps.git.getCommitFileDiff(workspace.path, commitHash, filePath),
      fetch: () => deps.git.fetch(workspace.path),
      pull: () => deps.git.pull(workspace.path),
      getBehindCount: () => deps.git.getBehindCount(workspace.path),
    },

    filesystemApi: {
      readDirectory: (dirPath) => deps.filesystem.readDirectory(workspace.path, dirPath),
      readFile: (filePath) => deps.filesystem.readFile(workspace.path, filePath),
      writeFile: (filePath, content) => deps.filesystem.writeFile(workspace.path, filePath, content),
      searchFiles: (query) => deps.filesystem.searchFiles(workspace.path, query),
    },

    runActionsApi: deps.runActions,

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
