import { createStore } from 'zustand/vanilla'
import type { StoreApi } from 'zustand'
import type { GitApi, GitHubApi, GitHubPrInfo, Workspace } from '../types'

export interface GitControllerDeps {
  git: GitApi
  github: GitHubApi
  lookupWorkspace: (id: string) => Workspace | undefined
  refreshGitInfo: () => Promise<void>
  refreshWorkspaceGitInfo: (id: string) => Promise<void>
  getWorkspace: () => Workspace
  initialWorkspace: Workspace
}

export interface GitControllerState {
  hasUncommittedChanges: boolean
  isDiffCleanFromParent: boolean
  hasConflictsWithParent: boolean
  behindCount: number
  pullLoading: boolean
  gitRefreshing: boolean
  prInfo: GitHubPrInfo | null
  refreshGit: () => Promise<void>
  pullFromRemote: () => Promise<{ success: boolean; error?: string }>
  openGitHub: () => Promise<{ url: string; hasPr: boolean } | { error: string }>
  dispose: () => void
}

export type GitController = StoreApi<GitControllerState>

export function createGitControllerStore(deps: GitControllerDeps): GitController {
  let debounceTimer: ReturnType<typeof setTimeout> | null = null
  let pendingResolvers: (() => void)[] = []

  async function runRefreshGit(): Promise<void> {
    store.setState({ gitRefreshing: true })
    try {
      try {
        const uncommitted = await deps.git.hasUncommittedChanges(deps.getWorkspace().path)
        store.setState({ hasUncommittedChanges: uncommitted })
      } catch { /* ignore — workspace may be removed */ }

      try {
        const ws = deps.getWorkspace()
        if (ws.isWorktree && ws.parentId) {
          await deps.refreshWorkspaceGitInfo(ws.parentId)
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

      try {
        const ws = deps.getWorkspace()
        if (ws.isGitRepo) {
          await deps.git.fetch(ws.path)
          const count = await deps.git.getBehindCount(ws.path)
          store.setState({ behindCount: count })
        }
      } catch { /* ignore — no remote or network issue */ }

      try {
        const ws = deps.getWorkspace()
        if (ws.isWorktree && ws.parentId && ws.gitBranch && ws.gitRootPath) {
          const parent = deps.lookupWorkspace(ws.parentId)
          if (parent?.gitBranch) {
            const result = await deps.github.getPrInfo(ws.gitRootPath, ws.gitBranch, parent.gitBranch)
            if ('prInfo' in result) {
              store.setState({ prInfo: result.prInfo })
            } else if ('noPr' in result) {
              store.setState({ prInfo: null })
            }
          }
        }
      } catch { /* ignore — network/auth issues */ }
    } finally {
      store.setState({ gitRefreshing: false })
      const resolvers = pendingResolvers
      pendingResolvers = []
      for (const resolve of resolvers) resolve()
    }
  }

  function refreshGit(): Promise<void> {
    return new Promise<void>((resolve) => {
      pendingResolvers.push(resolve)
      if (debounceTimer !== null) clearTimeout(debounceTimer)
      debounceTimer = setTimeout(() => {
        debounceTimer = null
        void runRefreshGit()
      }, 3000)
    })
  }

  const store = createStore<GitControllerState>()((set) => ({
    hasUncommittedChanges: false,
    isDiffCleanFromParent: false,
    hasConflictsWithParent: false,
    behindCount: 0,
    pullLoading: false,
    gitRefreshing: false,
    prInfo: null,

    refreshGit,

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

    dispose: (): void => {
      if (debounceTimer !== null) {
        clearTimeout(debounceTimer)
        debounceTimer = null
      }
      pendingResolvers = []
    },
  }))

  return store
}
