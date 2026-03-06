import { simpleGit, SimpleGit } from 'simple-git'
import * as path from 'path'
import * as fs from 'fs'

export interface GitInfo {
  isRepo: boolean
  branch: string | null
  rootPath: string | null
}

export interface WorktreeInfo {
  path: string
  branch: string
}

export async function getGitInfo(dirPath: string): Promise<GitInfo> {
  try {
    const git: SimpleGit = simpleGit(dirPath)
    const isRepo = await git.checkIsRepo()

    if (!isRepo) {
      return { isRepo: false, branch: null, rootPath: null }
    }

    const branch = await git.revparse(['--abbrev-ref', 'HEAD'])
    const rootPath = await git.revparse(['--show-toplevel'])

    return {
      isRepo: true,
      branch: branch.trim(),
      rootPath: rootPath.trim()
    }
  } catch {
    return { isRepo: false, branch: null, rootPath: null }
  }
}

export async function createWorktree(
  repoPath: string,
  worktreeName: string,
  baseBranch?: string
): Promise<{ success: boolean; path?: string; branch?: string; error?: string }> {
  try {
    const git: SimpleGit = simpleGit(repoPath)

    // Get the repo root
    const rootPath = (await git.revparse(['--show-toplevel'])).trim()
    const parentDir = path.dirname(rootPath)
    const repoName = path.basename(rootPath)

    // Create worktree path adjacent to the main repo
    const worktreePath = path.join(parentDir, `${repoName}-${worktreeName}`)
    const branchName = `treeterm/${worktreeName}`

    // Check if path already exists
    if (fs.existsSync(worktreePath)) {
      return { success: false, error: `Path already exists: ${worktreePath}` }
    }

    // Create new branch and worktree
    // If baseBranch provided, branch from there; otherwise from current HEAD
    if (baseBranch) {
      await git.raw(['worktree', 'add', '-b', branchName, worktreePath, baseBranch])
    } else {
      await git.raw(['worktree', 'add', '-b', branchName, worktreePath])
    }

    return {
      success: true,
      path: worktreePath,
      branch: branchName
    }
  } catch (err) {
    return {
      success: false,
      error: err instanceof Error ? err.message : 'Unknown error creating worktree'
    }
  }
}

export async function removeWorktree(
  repoPath: string,
  worktreePath: string,
  deleteBranch: boolean = false
): Promise<{ success: boolean; error?: string }> {
  try {
    const git: SimpleGit = simpleGit(repoPath)

    // Get branch name before removing worktree
    let branchName: string | null = null
    if (deleteBranch) {
      try {
        const worktreeGit = simpleGit(worktreePath)
        branchName = (await worktreeGit.revparse(['--abbrev-ref', 'HEAD'])).trim()
      } catch {
        // Ignore if we can't get branch name
      }
    }

    // Remove the worktree (--force to handle uncommitted changes)
    await git.raw(['worktree', 'remove', worktreePath, '--force'])

    // Delete the branch if requested
    if (deleteBranch && branchName && branchName.startsWith('treeterm/')) {
      try {
        await git.raw(['branch', '-D', branchName])
      } catch {
        // Ignore branch deletion errors
      }
    }

    return { success: true }
  } catch (err) {
    return {
      success: false,
      error: err instanceof Error ? err.message : 'Unknown error removing worktree'
    }
  }
}

export async function listWorktrees(repoPath: string): Promise<WorktreeInfo[]> {
  try {
    const git: SimpleGit = simpleGit(repoPath)
    const result = await git.raw(['worktree', 'list', '--porcelain'])

    const worktrees: WorktreeInfo[] = []
    const lines = result.split('\n')

    let currentWorktree: Partial<WorktreeInfo> = {}

    for (const line of lines) {
      if (line.startsWith('worktree ')) {
        currentWorktree.path = line.slice(9)
      } else if (line.startsWith('branch ')) {
        currentWorktree.branch = line.slice(7).replace('refs/heads/', '')
      } else if (line === '') {
        if (currentWorktree.path && currentWorktree.branch) {
          worktrees.push(currentWorktree as WorktreeInfo)
        }
        currentWorktree = {}
      }
    }

    return worktrees
  } catch {
    return []
  }
}
