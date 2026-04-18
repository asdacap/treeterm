import { vi } from 'vitest'
import type { WorkspaceGitApi, WorkspaceFilesystemApi, ExecApi, RunActionsApi } from '../renderer/types'
import type { WorktreeRegistryApi } from '../renderer/lib/worktreeRegistry'

export function createMockGitApi(): WorkspaceGitApi {
  return {
    getInfo: vi.fn(), createWorktree: vi.fn(), removeWorktree: vi.fn(),
    listWorktrees: vi.fn(), listLocalBranches: vi.fn(), listRemoteBranches: vi.fn(),
    getBranchesInWorktrees: vi.fn(), createWorktreeFromBranch: vi.fn(),
    createWorktreeFromRemote: vi.fn(), getDiff: vi.fn(), getFileDiff: vi.fn(),
    checkMergeConflicts: vi.fn(), merge: vi.fn(), hasUncommittedChanges: vi.fn(),
    commitAll: vi.fn(), deleteBranch: vi.fn(), getUncommittedChanges: vi.fn(),
    getUncommittedFileDiff: vi.fn(), stageFile: vi.fn(), unstageFile: vi.fn(),
    stageAll: vi.fn(), unstageAll: vi.fn(), commitStaged: vi.fn(),
    getFileContentsForDiff: vi.fn(), getUncommittedFileContentsForDiff: vi.fn(),
    getHeadCommitHash: vi.fn(), getLog: vi.fn(), getCommitDiff: vi.fn(),
    getCommitFileDiff: vi.fn(), fetch: vi.fn(), pull: vi.fn(), getBehindCount: vi.fn(),
  }
}

export function createMockFilesystemApi(): WorkspaceFilesystemApi {
  return {
    readDirectory: vi.fn(), readFile: vi.fn(), writeFile: vi.fn(), searchFiles: vi.fn(),
  }
}

export function createMockExecApi(): ExecApi {
  return { start: vi.fn(), kill: vi.fn(), onEvent: vi.fn() }
}

export function createMockRunActionsApi(): RunActionsApi {
  return { detect: vi.fn(), run: vi.fn() }
}

export function createMockWorktreeRegistryApi(): WorktreeRegistryApi {
  return {
    list: vi.fn<WorktreeRegistryApi['list']>().mockResolvedValue([]),
    upsert: vi.fn<WorktreeRegistryApi['upsert']>().mockResolvedValue(undefined),
    remove: vi.fn<WorktreeRegistryApi['remove']>().mockResolvedValue(undefined),
  }
}
