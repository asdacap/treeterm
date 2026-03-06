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

function isWorktreesDirInGitignore(rootPath: string): boolean {
  const gitignorePath = path.join(rootPath, '.gitignore')
  if (!fs.existsSync(gitignorePath)) {
    return false
  }

  try {
    const content = fs.readFileSync(gitignorePath, 'utf-8')
    const lines = content.split('\n').map((line) => line.trim())
    return lines.some((line) => line === '.worktrees' || line === '.worktrees/')
  } catch {
    return false
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

    // Determine worktree path based on .gitignore
    let worktreePath: string
    if (isWorktreesDirInGitignore(rootPath)) {
      // Use .worktrees directory inside the repo
      const worktreesDir = path.join(rootPath, '.worktrees')
      if (!fs.existsSync(worktreesDir)) {
        fs.mkdirSync(worktreesDir, { recursive: true })
      }
      worktreePath = path.join(worktreesDir, worktreeName)
    } else {
      // Create worktree path adjacent to the main repo
      const parentDir = path.dirname(rootPath)
      const repoName = path.basename(rootPath)
      worktreePath = path.join(parentDir, `${repoName}-${worktreeName}`)
    }

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

export interface DiffFile {
  path: string
  status: 'added' | 'modified' | 'deleted' | 'renamed'
  additions: number
  deletions: number
}

export interface DiffResult {
  files: DiffFile[]
  totalAdditions: number
  totalDeletions: number
  baseBranch: string
  headBranch: string
}

export async function getDiff(
  worktreePath: string,
  parentBranch: string
): Promise<{ success: boolean; diff?: DiffResult; error?: string }> {
  try {
    const git: SimpleGit = simpleGit(worktreePath)
    const currentBranch = (await git.revparse(['--abbrev-ref', 'HEAD'])).trim()

    // Get the merge base
    const mergeBase = (await git.raw(['merge-base', parentBranch, currentBranch])).trim()

    // Get diff stat
    const diffStat = await git.raw(['diff', '--numstat', mergeBase, currentBranch])
    const nameStatus = await git.raw(['diff', '--name-status', mergeBase, currentBranch])

    const files: DiffFile[] = []
    let totalAdditions = 0
    let totalDeletions = 0

    const statLines = diffStat.trim().split('\n').filter(Boolean)
    const statusLines = nameStatus.trim().split('\n').filter(Boolean)

    const statusMap: Record<string, 'added' | 'modified' | 'deleted' | 'renamed'> = {}
    for (const line of statusLines) {
      const [status, ...pathParts] = line.split('\t')
      const filePath = pathParts[pathParts.length - 1] // Handle renames
      if (status.startsWith('A')) statusMap[filePath] = 'added'
      else if (status.startsWith('M')) statusMap[filePath] = 'modified'
      else if (status.startsWith('D')) statusMap[filePath] = 'deleted'
      else if (status.startsWith('R')) statusMap[filePath] = 'renamed'
    }

    for (const line of statLines) {
      const [add, del, filePath] = line.split('\t')
      const additions = add === '-' ? 0 : parseInt(add)
      const deletions = del === '-' ? 0 : parseInt(del)

      files.push({
        path: filePath,
        status: statusMap[filePath] || 'modified',
        additions,
        deletions
      })

      totalAdditions += additions
      totalDeletions += deletions
    }

    return {
      success: true,
      diff: {
        files,
        totalAdditions,
        totalDeletions,
        baseBranch: parentBranch,
        headBranch: currentBranch
      }
    }
  } catch (err) {
    return {
      success: false,
      error: err instanceof Error ? err.message : 'Unknown error getting diff'
    }
  }
}

export async function getFileDiff(
  worktreePath: string,
  parentBranch: string,
  filePath: string
): Promise<{ success: boolean; diff?: string; error?: string }> {
  try {
    const git: SimpleGit = simpleGit(worktreePath)
    const currentBranch = (await git.revparse(['--abbrev-ref', 'HEAD'])).trim()
    const mergeBase = (await git.raw(['merge-base', parentBranch, currentBranch])).trim()

    const diff = await git.raw(['diff', '--color=never', mergeBase, currentBranch, '--', filePath])

    return { success: true, diff }
  } catch (err) {
    return {
      success: false,
      error: err instanceof Error ? err.message : 'Unknown error getting file diff'
    }
  }
}

export async function mergeWorktree(
  mainRepoPath: string,
  worktreeBranch: string,
  targetBranch: string,
  squash: boolean = false
): Promise<{ success: boolean; error?: string }> {
  try {
    const git: SimpleGit = simpleGit(mainRepoPath)

    // Checkout target branch
    await git.checkout(targetBranch)

    if (squash) {
      // Squash merge
      await git.raw(['merge', '--squash', worktreeBranch])
      await git.commit(`Squash merge ${worktreeBranch}`)
    } else {
      // Regular merge
      await git.merge([worktreeBranch, '--no-ff', '-m', `Merge ${worktreeBranch}`])
    }

    return { success: true }
  } catch (err) {
    return {
      success: false,
      error: err instanceof Error ? err.message : 'Unknown error merging'
    }
  }
}

export async function hasUncommittedChanges(repoPath: string): Promise<boolean> {
  try {
    const git: SimpleGit = simpleGit(repoPath)
    const status = await git.status()
    return !status.isClean()
  } catch {
    return false
  }
}

export async function commitAll(
  repoPath: string,
  message: string
): Promise<{ success: boolean; error?: string }> {
  try {
    const git: SimpleGit = simpleGit(repoPath)
    await git.add('.')
    await git.commit(message)
    return { success: true }
  } catch (err) {
    return {
      success: false,
      error: err instanceof Error ? err.message : 'Unknown error committing'
    }
  }
}
