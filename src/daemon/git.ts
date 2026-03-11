import { simpleGit, SimpleGit } from 'simple-git'
import * as path from 'path'
import * as fs from 'fs'

// Git timeout configuration for large repositories
const GIT_TIMEOUT = 300000 // 5 minutes for blocking operations
const gitOptions = {
  timeout: {
    block: GIT_TIMEOUT
  }
}

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
    const git: SimpleGit = simpleGit(dirPath, gitOptions)
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
    console.log('[git] createWorktree started:', { repoPath, worktreeName, baseBranch })
    const git: SimpleGit = simpleGit(repoPath, gitOptions)

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

    // Hierarchical branch naming: if base branch is treeterm/*, append child name
    let branchName: string
    if (baseBranch && baseBranch.startsWith('treeterm/')) {
      branchName = `${baseBranch}/${worktreeName}`
    } else {
      branchName = `treeterm/${worktreeName}`
    }

    // Check if path already exists
    if (fs.existsSync(worktreePath)) {
      return { success: false, error: `Path already exists: ${worktreePath}` }
    }

    // Create new branch and worktree
    // If baseBranch provided, branch from there; otherwise from current HEAD
    console.log('[git] Creating worktree at:', worktreePath, 'with branch:', branchName)
    if (baseBranch) {
      await git.raw(['worktree', 'add', '-b', branchName, worktreePath, baseBranch])
    } else {
      await git.raw(['worktree', 'add', '-b', branchName, worktreePath])
    }
    console.log('[git] Worktree created successfully')

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
    const git: SimpleGit = simpleGit(repoPath, gitOptions)

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
    const git: SimpleGit = simpleGit(repoPath, gitOptions)
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

export interface ChildWorktreeInfo extends WorktreeInfo {
  displayName: string
}

export async function getChildWorktrees(
  repoPath: string,
  parentBranch: string | null
): Promise<ChildWorktreeInfo[]> {
  console.log('[getChildWorktrees] called with:', { repoPath, parentBranch })

  const allWorktrees = await listWorktrees(repoPath)
  console.log('[getChildWorktrees] allWorktrees:', allWorktrees)

  // Determine the expected prefix for child branches
  const branchPrefix = (parentBranch && parentBranch.startsWith('treeterm/'))
    ? `${parentBranch}/`
    : 'treeterm/'
  console.log('[getChildWorktrees] branchPrefix:', branchPrefix)

  const filtered = allWorktrees
    .filter(wt => {
      // Branch must start with the prefix
      if (!wt.branch.startsWith(branchPrefix)) return false
      // Must be a direct child (no further slashes after prefix)
      const remainder = wt.branch.slice(branchPrefix.length)
      return remainder.length > 0 && !remainder.includes('/')
    })

  console.log('[getChildWorktrees] filtered worktrees:', filtered)

  const result = filtered.map(wt => ({
    ...wt,
    displayName: wt.branch.slice(branchPrefix.length)
  }))

  console.log('[getChildWorktrees] returning:', result)
  return result
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
    const git: SimpleGit = simpleGit(worktreePath, gitOptions)
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
    const git: SimpleGit = simpleGit(worktreePath, gitOptions)
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

export async function getDiffAgainstHead(
  worktreePath: string,
  parentBranch: string
): Promise<{ success: boolean; diff?: DiffResult; error?: string }> {
  try {
    const git: SimpleGit = simpleGit(worktreePath, gitOptions)
    const currentBranch = (await git.revparse(['--abbrev-ref', 'HEAD'])).trim()

    // Get the merge base to compare against the common ancestor
    // This shows only changes introduced on the current branch
    const mergeBase = (await git.raw(['merge-base', parentBranch, currentBranch])).trim()

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
      error: err instanceof Error ? err.message : 'Unknown error getting diff against HEAD'
    }
  }
}

export async function getFileDiffAgainstHead(
  worktreePath: string,
  parentBranch: string,
  filePath: string
): Promise<{ success: boolean; diff?: string; error?: string }> {
  try {
    const git: SimpleGit = simpleGit(worktreePath, gitOptions)
    const currentBranch = (await git.revparse(['--abbrev-ref', 'HEAD'])).trim()

    // Get the merge base to compare against the common ancestor
    const mergeBase = (await git.raw(['merge-base', parentBranch, currentBranch])).trim()

    const diff = await git.raw(['diff', '--color=never', mergeBase, currentBranch, '--', filePath])

    return { success: true, diff }
  } catch (err) {
    return {
      success: false,
      error: err instanceof Error ? err.message : 'Unknown error getting file diff against HEAD'
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
    const git: SimpleGit = simpleGit(mainRepoPath, gitOptions)

    // Checkout target branch
    await git.checkout(targetBranch)

    if (squash) {
      // Squash merge
      await git.raw(['merge', '--squash', worktreeBranch])
      await git.commit(`Squash merge ${worktreeBranch}`)
    } else {
      // Regular merge
      await git.merge([worktreeBranch])
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
    const git: SimpleGit = simpleGit(repoPath, gitOptions)
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
    const git: SimpleGit = simpleGit(repoPath, gitOptions)
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

export async function deleteBranch(
  repoPath: string,
  branchName: string
): Promise<{ success: boolean; error?: string }> {
  try {
    const git: SimpleGit = simpleGit(repoPath, gitOptions)
    await git.raw(['branch', '-D', branchName])
    return { success: true }
  } catch (err) {
    return {
      success: false,
      error: err instanceof Error ? err.message : 'Unknown error deleting branch'
    }
  }
}

export interface ConflictInfo {
  hasConflicts: boolean
  conflictedFiles: string[]
  messages: string[]
}

/**
 * Check for merge conflicts without actually performing the merge.
 * Uses git merge-tree --write-tree to simulate the merge.
 */
export interface UncommittedFile {
  path: string
  status: 'added' | 'modified' | 'deleted' | 'renamed' | 'untracked'
  staged: boolean
  additions: number
  deletions: number
}

export interface UncommittedChanges {
  files: UncommittedFile[]
  totalAdditions: number
  totalDeletions: number
}

export async function getUncommittedChanges(
  repoPath: string
): Promise<{ success: boolean; changes?: UncommittedChanges; error?: string }> {
  try {
    const git: SimpleGit = simpleGit(repoPath, gitOptions)
    const status = await git.status()

    const files: UncommittedFile[] = []
    let totalAdditions = 0
    let totalDeletions = 0

    // Helper to get numstat for a file
    const getFileStats = async (
      filePath: string,
      staged: boolean
    ): Promise<{ additions: number; deletions: number }> => {
      try {
        const diffArgs = staged ? ['diff', '--cached', '--numstat', '--', filePath] : ['diff', '--numstat', '--', filePath]
        const numstat = await git.raw(diffArgs)
        if (numstat.trim()) {
          const [add, del] = numstat.trim().split('\t')
          return {
            additions: add === '-' ? 0 : parseInt(add) || 0,
            deletions: del === '-' ? 0 : parseInt(del) || 0
          }
        }
      } catch {
        // Ignore errors
      }
      return { additions: 0, deletions: 0 }
    }

    // Process staged files
    for (const file of status.staged) {
      const statusChar = status.files.find((f) => f.path === file)?.index || 'M'
      let fileStatus: UncommittedFile['status'] = 'modified'
      if (statusChar === 'A') fileStatus = 'added'
      else if (statusChar === 'D') fileStatus = 'deleted'
      else if (statusChar === 'R') fileStatus = 'renamed'

      const stats = await getFileStats(file, true)
      files.push({
        path: file,
        status: fileStatus,
        staged: true,
        ...stats
      })
      totalAdditions += stats.additions
      totalDeletions += stats.deletions
    }

    // Process modified (unstaged) files
    for (const file of status.modified) {
      // Skip if already in staged
      if (files.some((f) => f.path === file)) continue

      const stats = await getFileStats(file, false)
      files.push({
        path: file,
        status: 'modified',
        staged: false,
        ...stats
      })
      totalAdditions += stats.additions
      totalDeletions += stats.deletions
    }

    // Process deleted (unstaged) files
    for (const file of status.deleted) {
      if (files.some((f) => f.path === file)) continue

      const stats = await getFileStats(file, false)
      files.push({
        path: file,
        status: 'deleted',
        staged: false,
        ...stats
      })
      totalAdditions += stats.additions
      totalDeletions += stats.deletions
    }

    // Process untracked (new) files
    for (const file of status.not_added) {
      files.push({
        path: file,
        status: 'untracked',
        staged: false,
        additions: 0,
        deletions: 0
      })
    }

    return {
      success: true,
      changes: {
        files,
        totalAdditions,
        totalDeletions
      }
    }
  } catch (err) {
    return {
      success: false,
      error: err instanceof Error ? err.message : 'Unknown error getting uncommitted changes'
    }
  }
}

export async function getUncommittedFileDiff(
  repoPath: string,
  filePath: string,
  staged: boolean
): Promise<{ success: boolean; diff?: string; error?: string }> {
  try {
    const git: SimpleGit = simpleGit(repoPath, gitOptions)

    const diffArgs = staged
      ? ['diff', '--cached', '--color=never', '--', filePath]
      : ['diff', '--color=never', '--', filePath]

    const diff = await git.raw(diffArgs)

    return { success: true, diff }
  } catch (err) {
    return {
      success: false,
      error: err instanceof Error ? err.message : 'Unknown error getting file diff'
    }
  }
}

export async function stageFile(
  repoPath: string,
  filePath: string
): Promise<{ success: boolean; error?: string }> {
  try {
    const git: SimpleGit = simpleGit(repoPath, gitOptions)
    await git.add(filePath)
    return { success: true }
  } catch (err) {
    return {
      success: false,
      error: err instanceof Error ? err.message : 'Unknown error staging file'
    }
  }
}

export async function unstageFile(
  repoPath: string,
  filePath: string
): Promise<{ success: boolean; error?: string }> {
  try {
    const git: SimpleGit = simpleGit(repoPath, gitOptions)
    await git.raw(['reset', 'HEAD', '--', filePath])
    return { success: true }
  } catch (err) {
    return {
      success: false,
      error: err instanceof Error ? err.message : 'Unknown error unstaging file'
    }
  }
}

export async function stageAll(
  repoPath: string
): Promise<{ success: boolean; error?: string }> {
  try {
    const git: SimpleGit = simpleGit(repoPath, gitOptions)
    await git.add('.')
    return { success: true }
  } catch (err) {
    return {
      success: false,
      error: err instanceof Error ? err.message : 'Unknown error staging all files'
    }
  }
}

export async function unstageAll(
  repoPath: string
): Promise<{ success: boolean; error?: string }> {
  try {
    const git: SimpleGit = simpleGit(repoPath, gitOptions)
    await git.raw(['reset', 'HEAD'])
    return { success: true }
  } catch (err) {
    return {
      success: false,
      error: err instanceof Error ? err.message : 'Unknown error unstaging all files'
    }
  }
}

export async function commitStaged(
  repoPath: string,
  message: string
): Promise<{ success: boolean; error?: string }> {
  try {
    const git: SimpleGit = simpleGit(repoPath, gitOptions)
    await git.commit(message)
    return { success: true }
  } catch (err) {
    return {
      success: false,
      error: err instanceof Error ? err.message : 'Unknown error committing'
    }
  }
}

// Helper function to parse conflict information from git merge-tree output
function parseConflicts(output: string): ConflictInfo {
  const lines = output.split('\n')
  const conflictedFiles: string[] = []
  const messages: string[] = []

  for (const line of lines) {
    const trimmed = line.trim()
    if (trimmed.startsWith('CONFLICT') || trimmed.startsWith('Auto-merging')) {
      messages.push(trimmed)

      // Extract file paths from CONFLICT messages
      // Format: "CONFLICT (content): Merge conflict in <filepath>"
      const mergeConflictMatch = trimmed.match(/Merge conflict in (.+)/)
      if (mergeConflictMatch) {
        const filePath = mergeConflictMatch[1].trim()
        if (!conflictedFiles.includes(filePath)) {
          conflictedFiles.push(filePath)
        }
      }

      // Format: "CONFLICT (add/add): Merge conflict in <filepath>"
      // Format: "CONFLICT (modify/delete): <filepath> deleted in ..."
      const conflictPathMatch = trimmed.match(/CONFLICT \([^)]+\): (.+?) (?:deleted|renamed|modified)/)
      if (conflictPathMatch) {
        const filePath = conflictPathMatch[1].trim()
        if (!conflictedFiles.includes(filePath)) {
          conflictedFiles.push(filePath)
        }
      }
    }
  }

  return {
    hasConflicts: conflictedFiles.length > 0,
    conflictedFiles,
    messages
  }
}

export async function checkMergeConflicts(
  repoPath: string,
  sourceBranch: string,
  targetBranch: string
): Promise<{ success: boolean; conflicts?: ConflictInfo; error?: string }> {
  try {
    const git: SimpleGit = simpleGit(repoPath, gitOptions)

    // Use git merge-tree --write-tree to simulate merge
    // This performs a 3-way merge without touching the working tree
    // Note: We intentionally don't use --no-messages because we need the
    // conflict messages to detect and report conflicts to the user
    const result = await git.raw([
      'merge-tree',
      '--write-tree',
      targetBranch,
      sourceBranch
    ])

    // simple-git may not throw an error even when git exits with code 1 for conflicts
    // Check the result string for conflict indicators
    if (result.includes('CONFLICT') || result.includes('Merge conflict')) {
      return {
        success: true,
        conflicts: parseConflicts(result)
      }
    }

    // Clean merge - the output is just the tree OID
    return {
      success: true,
      conflicts: {
        hasConflicts: false,
        conflictedFiles: [],
        messages: []
      }
    }
  } catch (err) {
    const errorMessage = err instanceof Error ? err.message : String(err)

    // git merge-tree returns exit code 1 for conflicts
    // Check if this is a conflict vs an actual error
    if (errorMessage.includes('CONFLICT') || errorMessage.includes('Merge conflict')) {
      return {
        success: true,
        conflicts: parseConflicts(errorMessage)
      }
    }

    // Actual error (not a conflict)
    return {
      success: false,
      error: errorMessage
    }
  }
}

// Helper function to detect language from file extension
function detectLanguageFromPath(filePath: string): string {
  const ext = path.extname(filePath).toLowerCase()
  const languageMap: Record<string, string> = {
    '.ts': 'typescript',
    '.tsx': 'typescript',
    '.js': 'javascript',
    '.jsx': 'javascript',
    '.json': 'json',
    '.md': 'markdown',
    '.css': 'css',
    '.scss': 'scss',
    '.less': 'less',
    '.html': 'html',
    '.htm': 'html',
    '.xml': 'xml',
    '.py': 'python',
    '.rs': 'rust',
    '.go': 'go',
    '.java': 'java',
    '.c': 'c',
    '.cpp': 'cpp',
    '.h': 'c',
    '.hpp': 'cpp',
    '.yaml': 'yaml',
    '.yml': 'yaml',
    '.toml': 'toml',
    '.sh': 'bash',
    '.bash': 'bash',
    '.zsh': 'bash',
    '.sql': 'sql',
    '.graphql': 'graphql',
    '.gql': 'graphql',
    '.vue': 'html',
    '.svelte': 'html',
    '.rb': 'ruby',
    '.php': 'php',
    '.swift': 'swift',
    '.kt': 'kotlin',
    '.kts': 'kotlin',
    '.scala': 'scala',
    '.r': 'r',
    '.R': 'r',
    '.lua': 'lua',
    '.dockerfile': 'dockerfile',
    '.gitignore': 'plaintext',
    '.env': 'plaintext'
  }
  return languageMap[ext] || 'plaintext'
}

export interface FileDiffContents {
  originalContent: string
  modifiedContent: string
  language: string
}

/**
 * Get file contents for diff comparison (merge-base comparison)
 * Returns both original (at merge base) and modified (current branch) versions
 */
export async function getFileContentsForDiff(
  worktreePath: string,
  parentBranch: string,
  filePath: string
): Promise<{ success: boolean; contents?: FileDiffContents; error?: string }> {
  try {
    const git: SimpleGit = simpleGit(worktreePath, gitOptions)
    const currentBranch = (await git.revparse(['--abbrev-ref', 'HEAD'])).trim()
    const mergeBase = (await git.raw(['merge-base', parentBranch, currentBranch])).trim()

    // Get original content (at merge base)
    let originalContent = ''
    try {
      originalContent = await git.raw(['show', `${mergeBase}:${filePath}`])
    } catch {
      // File didn't exist at merge base (new file)
      originalContent = ''
    }

    // Get modified content (current branch)
    let modifiedContent = ''
    try {
      modifiedContent = await git.raw(['show', `${currentBranch}:${filePath}`])
    } catch {
      // File was deleted
      modifiedContent = ''
    }

    const language = detectLanguageFromPath(filePath)

    return {
      success: true,
      contents: {
        originalContent,
        modifiedContent,
        language
      }
    }
  } catch (err) {
    return {
      success: false,
      error: err instanceof Error ? err.message : 'Unknown error getting file contents for diff'
    }
  }
}

/**
 * Get file contents for diff comparison (against parent HEAD)
 * Returns both original (parent branch HEAD) and modified (current branch) versions
 */
export async function getFileContentsForDiffAgainstHead(
  worktreePath: string,
  parentBranch: string,
  filePath: string
): Promise<{ success: boolean; contents?: FileDiffContents; error?: string }> {
  try {
    const git: SimpleGit = simpleGit(worktreePath, gitOptions)
    const currentBranch = (await git.revparse(['--abbrev-ref', 'HEAD'])).trim()

    // Get the merge base to compare against the common ancestor
    const mergeBase = (await git.raw(['merge-base', parentBranch, currentBranch])).trim()

    // Get original content (at merge base)
    let originalContent = ''
    try {
      originalContent = await git.raw(['show', `${mergeBase}:${filePath}`])
    } catch {
      // File doesn't exist on parent branch
      originalContent = ''
    }

    // Get modified content (current branch)
    let modifiedContent = ''
    try {
      modifiedContent = await git.raw(['show', `${currentBranch}:${filePath}`])
    } catch {
      // File was deleted
      modifiedContent = ''
    }

    const language = detectLanguageFromPath(filePath)

    return {
      success: true,
      contents: {
        originalContent,
        modifiedContent,
        language
      }
    }
  } catch (err) {
    return {
      success: false,
      error: err instanceof Error ? err.message : 'Unknown error getting file contents for diff against HEAD'
    }
  }
}

/**
 * Get file contents for uncommitted changes diff
 * Returns both original (HEAD or index) and modified (working tree or index) versions
 */
export async function getUncommittedFileContentsForDiff(
  repoPath: string,
  filePath: string,
  staged: boolean
): Promise<{ success: boolean; contents?: FileDiffContents; error?: string }> {
  try {
    const git: SimpleGit = simpleGit(repoPath, gitOptions)
    const fullPath = path.join(repoPath, filePath)

    let originalContent = ''
    let modifiedContent = ''

    if (staged) {
      // Staged: compare HEAD to index
      try {
        originalContent = await git.raw(['show', `HEAD:${filePath}`])
      } catch {
        originalContent = '' // New file
      }
      try {
        modifiedContent = await git.raw(['show', `:${filePath}`]) // Index version
      } catch {
        modifiedContent = '' // File deleted in index
      }
    } else {
      // Unstaged: compare index (or HEAD) to working tree
      try {
        // Try to get from index first, fall back to HEAD
        originalContent = await git.raw(['show', `:${filePath}`])
      } catch {
        try {
          originalContent = await git.raw(['show', `HEAD:${filePath}`])
        } catch {
          originalContent = '' // New file
        }
      }
      // Read working tree version
      try {
        modifiedContent = fs.readFileSync(fullPath, 'utf-8')
      } catch {
        modifiedContent = '' // File deleted
      }
    }

    const language = detectLanguageFromPath(filePath)

    return {
      success: true,
      contents: {
        originalContent,
        modifiedContent,
        language
      }
    }
  } catch (err) {
    return {
      success: false,
      error: err instanceof Error ? err.message : 'Unknown error getting uncommitted file contents for diff'
    }
  }
}

/**
 * List all local branches
 */
export async function listLocalBranches(repoPath: string): Promise<string[]> {
  try {
    const git: SimpleGit = simpleGit(repoPath, gitOptions)
    const result = await git.raw(['branch', '--list', '--format=%(refname:short)'])
    return result
      .split('\n')
      .map(b => b.trim())
      .filter(b => b.length > 0)
  } catch {
    return []
  }
}

/**
 * List all remote branches
 */
export async function listRemoteBranches(repoPath: string): Promise<string[]> {
  try {
    const git: SimpleGit = simpleGit(repoPath, gitOptions)
    const result = await git.raw(['branch', '-r', '--format=%(refname:short)'])
    return result
      .split('\n')
      .map(b => b.trim())
      .filter(b => b.length > 0 && !b.includes('HEAD'))
  } catch {
    return []
  }
}

/**
 * Get which branches are currently in worktrees
 */
export async function getBranchesInWorktrees(repoPath: string): Promise<string[]> {
  const worktrees = await listWorktrees(repoPath)
  return worktrees.map(wt => wt.branch)
}

/**
 * Create worktree from existing local branch (no -b flag)
 */
export async function createWorktreeFromBranch(
  repoPath: string,
  branch: string,
  worktreeName: string
): Promise<{ success: boolean; path?: string; branch?: string; error?: string }> {
  try {
    console.log('[git] createWorktreeFromBranch started:', { repoPath, branch, worktreeName })
    const git: SimpleGit = simpleGit(repoPath, gitOptions)

    // Get the repo root
    const rootPath = (await git.revparse(['--show-toplevel'])).trim()

    // Determine worktree path based on .gitignore
    let worktreePath: string
    if (isWorktreesDirInGitignore(rootPath)) {
      const worktreesDir = path.join(rootPath, '.worktrees')
      if (!fs.existsSync(worktreesDir)) {
        fs.mkdirSync(worktreesDir, { recursive: true })
      }
      worktreePath = path.join(worktreesDir, worktreeName)
    } else {
      const parentDir = path.dirname(rootPath)
      const repoName = path.basename(rootPath)
      worktreePath = path.join(parentDir, `${repoName}-${worktreeName}`)
    }

    // Check if path already exists
    if (fs.existsSync(worktreePath)) {
      return { success: false, error: `Path already exists: ${worktreePath}` }
    }

    // Create worktree from existing branch (no -b flag)
    console.log('[git] Creating worktree at:', worktreePath, 'from branch:', branch)
    await git.raw(['worktree', 'add', worktreePath, branch])
    console.log('[git] Worktree created successfully')

    return {
      success: true,
      path: worktreePath,
      branch: branch
    }
  } catch (err) {
    return {
      success: false,
      error: err instanceof Error ? err.message : 'Unknown error creating worktree from branch'
    }
  }
}

/**
 * Create worktree from remote branch (track remote)
 */
export async function createWorktreeFromRemote(
  repoPath: string,
  remoteBranch: string,
  worktreeName: string
): Promise<{ success: boolean; path?: string; branch?: string; error?: string }> {
  try {
    console.log('[git] createWorktreeFromRemote started:', { repoPath, remoteBranch, worktreeName })
    const git: SimpleGit = simpleGit(repoPath, gitOptions)

    // Get the repo root
    const rootPath = (await git.revparse(['--show-toplevel'])).trim()

    // Determine worktree path based on .gitignore
    let worktreePath: string
    if (isWorktreesDirInGitignore(rootPath)) {
      const worktreesDir = path.join(rootPath, '.worktrees')
      if (!fs.existsSync(worktreesDir)) {
        fs.mkdirSync(worktreesDir, { recursive: true })
      }
      worktreePath = path.join(worktreesDir, worktreeName)
    } else {
      const parentDir = path.dirname(rootPath)
      const repoName = path.basename(rootPath)
      worktreePath = path.join(parentDir, `${repoName}-${worktreeName}`)
    }

    // Check if path already exists
    if (fs.existsSync(worktreePath)) {
      return { success: false, error: `Path already exists: ${worktreePath}` }
    }

    // Extract local branch name from remote branch (e.g., origin/feature -> feature)
    const localBranchName = remoteBranch.split('/').slice(1).join('/')

    // Create worktree with tracking branch
    console.log('[git] Creating worktree at:', worktreePath, 'from remote:', remoteBranch, 'as local branch:', localBranchName)
    await git.raw(['worktree', 'add', '-b', localBranchName, worktreePath, remoteBranch])
    console.log('[git] Worktree created successfully from remote branch')

    return {
      success: true,
      path: worktreePath,
      branch: localBranchName
    }
  } catch (err) {
    return {
      success: false,
      error: err instanceof Error ? err.message : 'Unknown error creating worktree from remote branch'
    }
  }
}

export async function getHeadCommitHash(
  repoPath: string
): Promise<{ success: boolean; hash?: string; error?: string }> {
  try {
    const git: SimpleGit = simpleGit(repoPath, gitOptions)
    const hash = (await git.revparse(['HEAD'])).trim()
    return { success: true, hash }
  } catch (err) {
    return {
      success: false,
      error: err instanceof Error ? err.message : 'Unknown error getting HEAD commit hash'
    }
  }
}
