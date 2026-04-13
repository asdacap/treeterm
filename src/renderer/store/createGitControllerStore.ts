import { createStore } from 'zustand/vanilla'
import type { StoreApi } from 'zustand'
import type { GitApi, GitHubApi, GitHubPrInfo, Workspace } from '../types'

export interface GitControllerDeps {
  git: GitApi
  github: GitHubApi
  lookupWorkspace: (id: string) => Workspace | undefined
  refreshGitInfo: () => Promise<void>
  getWorkspace: () => Workspace
  initialWorkspace: Workspace
  isActiveWorkspace: () => boolean
}

export interface GitControllerState {
  hasUncommittedChanges: boolean
  isDiffCleanFromParent: boolean
  hasConflictsWithParent: boolean
  behindCount: number
  pullLoading: boolean
  gitRefreshing: boolean
  prInfo: GitHubPrInfo | null
  refreshDiffStatus: () => Promise<void>
  refreshRemoteStatus: () => Promise<void>
  pullFromRemote: () => Promise<{ success: boolean; error?: string }>
  refreshPrStatus: () => Promise<void>
  openGitHub: () => Promise<{ url: string; hasPr: boolean } | { error: string }>
  triggerRefresh: () => void
  // Called by createWorkspaceStore after the workspace store is fully initialized
  startPolling: () => void
  dispose: () => void
}

export type GitController = StoreApi<GitControllerState>

export function createGitControllerStore(deps: GitControllerDeps): GitController {
  let gitControllerInterval: ReturnType<typeof setInterval> | null = null

  async function refreshDiffStatus(): Promise<void> {
    store.setState({ gitRefreshing: true })
    try {
      try {
        const uncommitted = await deps.git.hasUncommittedChanges(deps.getWorkspace().path)
        store.setState({ hasUncommittedChanges: uncommitted })
      } catch { /* ignore — workspace may be removed */ }

      try {
        const ws = deps.getWorkspace()
        if (ws.isWorktree && ws.parentId) {
          const parent = deps.lookupWorkspace(ws.parentId)
          if (parent?.gitBranch) {
            const result = await deps.git.getDiff(ws.path, parent.gitBranch)
            const clean = result.success ? result.diff.files.length === 0 : false
            const uncommitted = store.getState().hasUncommittedChanges
            store.setState({ isDiffCleanFromParent: clean && !uncommitted })
          }
        }
      } catch { /* ignore */ }

      try {
        const ws = deps.getWorkspace()
        if (ws.isWorktree && ws.parentId && ws.gitBranch) {
          const parent = deps.lookupWorkspace(ws.parentId)
          if (parent?.gitBranch) {
            const result = await deps.git.checkMergeConflicts(ws.path, ws.gitBranch, parent.gitBranch)
            store.setState({ hasConflictsWithParent: result.success ? result.conflicts.hasConflicts : false })
          }
        }
      } catch { /* ignore */ }
    } finally {
      store.setState({ gitRefreshing: false })
    }
  }

  async function refreshPrStatus(): Promise<void> {
    const ws = deps.getWorkspace()
    if (!ws.isWorktree || !ws.parentId || !ws.gitBranch || !ws.gitRootPath) return
    const parent = deps.lookupWorkspace(ws.parentId)
    if (!parent?.gitBranch) return
    try {
      const result = await deps.github.getPrInfo(ws.gitRootPath, ws.gitBranch, parent.gitBranch)
      if ('prInfo' in result) {
        store.setState({ prInfo: result.prInfo })
      } else if ('noPr' in result) {
        store.setState({ prInfo: null })
      }
    } catch { /* ignore — network/auth issues */ }
  }

  function startGitController(): void {
    if (!deps.initialWorkspace.isGitRepo) return

    void refreshDiffStatus()
    gitControllerInterval = setInterval(() => { if (deps.isActiveWorkspace()) void refreshDiffStatus(); }, 10_000)
  }

  function stopGitController(): void {
    if (gitControllerInterval) {
      clearInterval(gitControllerInterval)
      gitControllerInterval = null
    }
  }

  const store = createStore<GitControllerState>()((set) => ({
    hasUncommittedChanges: false,
    isDiffCleanFromParent: false,
    hasConflictsWithParent: false,
    behindCount: 0,
    pullLoading: false,
    gitRefreshing: false,
    prInfo: null,

    refreshDiffStatus,
    triggerRefresh: (): void => { void refreshDiffStatus(); },

    refreshRemoteStatus: async () => {
      const ws = deps.getWorkspace()
      if (!ws.isGitRepo) return
      try {
        await deps.git.fetch(ws.path)
        const count = await deps.git.getBehindCount(ws.path)
        set({ behindCount: count })
      } catch { /* ignore — no remote or network issue */ }
    },

    pullFromRemote: async () => {
      const ws = deps.getWorkspace()
      if (!ws.isGitRepo) return { success: false, error: 'Not a git repo' }
      set({ pullLoading: true })
      try {
        const result = await deps.git.pull(ws.path)
        if (result.success) {
          set({ behindCount: 0 })
          await deps.refreshGitInfo()
        }
        return result
      } catch (error) {
        return { success: false, error: error instanceof Error ? error.message : 'Unknown error' }
      } finally {
        set({ pullLoading: false })
      }
    },

    refreshPrStatus,

    openGitHub: async () => {
      const ws = deps.getWorkspace()
      if (!ws.parentId || !ws.gitBranch || !ws.gitRootPath) return { error: 'Missing workspace info' }
      const parent = deps.lookupWorkspace(ws.parentId)
      if (!parent?.gitBranch) return { error: 'Parent branch not found' }
      const result = await deps.github.getPrInfo(ws.gitRootPath, ws.gitBranch, parent.gitBranch)
      if ('prInfo' in result) {
        set({ prInfo: result.prInfo })
        return { url: result.prInfo.url, hasPr: true }
      } else if ('noPr' in result) {
        set({ prInfo: null })
        return { url: result.createUrl, hasPr: false }
      }
      return result
    },

    startPolling: (): void => {
      startGitController()
      if (deps.initialWorkspace.isGitRepo) {
        void store.getState().refreshRemoteStatus()
      }
      if (deps.initialWorkspace.isWorktree && deps.initialWorkspace.parentId) {
        void refreshPrStatus()
      }
    },

    dispose: () => { stopGitController(); },
  }))

  return store
}
