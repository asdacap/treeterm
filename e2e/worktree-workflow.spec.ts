/**
 * E2E Tests for Basic Worktree Workflow
 * 
 * Tests the complete workflow of:
 * 1. Creating a temp git repo
 * 2. Opening workspace
 * 3. Creating sub-worktree
 * 4. Modifying files
 * 5. Merging changes back
 * 6. Verifying merged content
 */

import { test, expect } from '@playwright/test'
import { execSync } from 'child_process'
import * as fs from 'fs'
import * as path from 'path'
import * as os from 'os'
import {
  launchApp,
  closeApp,
  killDaemon,
  cleanupTestData,
  resetTestSocketPath,
  waitForDaemon
} from './helpers'

// Helper to create a temporary git repository
function createTempGitRepo(): string {
  const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'treeterm-worktree-test-'))
  
  // Initialize git repo
  execSync('git init', { cwd: tempDir })
  execSync('git config user.email "test@example.com"', { cwd: tempDir })
  execSync('git config user.name "Test User"', { cwd: tempDir })
  
  // Create initial file
  const initialContent = 'Hello World\n'
  fs.writeFileSync(path.join(tempDir, 'README.md'), initialContent)
  
  // Initial commit
  execSync('git add README.md', { cwd: tempDir })
  execSync('git commit -m "Initial commit"', { cwd: tempDir })
  
  // Rename master branch to main (git init defaults to master)
  execSync('git branch -m master main', { cwd: tempDir })
  
  console.log('[Test] Created temp git repo at:', tempDir)
  return tempDir
}

// Helper to cleanup temp directory
function cleanupTempRepo(repoPath: string): void {
  if (fs.existsSync(repoPath)) {
    fs.rmSync(repoPath, { recursive: true, force: true })
    console.log('[Test] Cleaned up temp repo:', repoPath)
  }
}

test.describe('Worktree Workflow', () => {
  let tempRepoPath: string | null = null

  test.beforeEach(async () => {
    resetTestSocketPath()
    killDaemon()
    cleanupTestData()
  })

  test.afterEach(async () => {
    killDaemon()
    cleanupTestData()
    if (tempRepoPath) {
      cleanupTempRepo(tempRepoPath)
      tempRepoPath = null
    }
  })

  test('complete worktree workflow: create, modify, commit, merge', async () => {
    // Step 1: Create temporary git repo
    tempRepoPath = createTempGitRepo()
    expect(fs.existsSync(tempRepoPath)).toBe(true)
    expect(fs.existsSync(path.join(tempRepoPath, '.git'))).toBe(true)

    // Verify initial state
    const initialContent = fs.readFileSync(path.join(tempRepoPath, 'README.md'), 'utf-8')
    expect(initialContent).toBe('Hello World\n')
    console.log('[Test] Initial content:', initialContent.trim())

    // Step 2: Launch app with the workspace
    const { app, window } = await launchApp(tempRepoPath)
    await waitForDaemon(10000)

    // Wait for app to be ready
    await window.waitForLoadState('domcontentloaded')
    await window.waitForTimeout(2000)

    console.log('[Test] App launched with workspace:', tempRepoPath)

    // Step 3: Create a sub-worktree using IPC
    const worktreeResult = await window.evaluate(async (repoPath) => {
      // @ts-ignore - window.electron is injected by preload
      const result = await window.electron.git.createWorktree(repoPath, 'feature-branch', 'main')
      console.log('[Test] Worktree creation result:', result)
      return result
    }, tempRepoPath)

    expect(worktreeResult.success).toBe(true)
    expect(worktreeResult.path).toBeDefined()
    expect(worktreeResult.branch).toBe('feature-branch')
    
    const worktreePath = worktreeResult.path
    console.log('[Test] Created worktree at:', worktreePath)
    expect(fs.existsSync(worktreePath)).toBe(true)

    // Step 4: Modify a file in the worktree using filesystem API
    const modifiedContent = 'Hello World\n\nThis is a new feature!\n'
    const writeResult = await window.evaluate(async ({ worktreePath, content }) => {
      // @ts-ignore - window.electron is injected by preload
      const result = await window.electron.filesystem.writeFile(
        worktreePath,
        `${worktreePath}/README.md`,
        content
      )
      console.log('[Test] File write result:', result)
      return result
    }, { worktreePath, content: modifiedContent })

    expect(writeResult.success).toBe(true)
    console.log('[Test] Modified file in worktree')

    // Verify file was modified
    const worktreeFileContent = fs.readFileSync(path.join(worktreePath, 'README.md'), 'utf-8')
    expect(worktreeFileContent).toBe(modifiedContent)
    console.log('[Test] Verified worktree file content:', worktreeFileContent.trim())

    // Step 5: Check git status and commit changes
    const hasChanges = await window.evaluate(async (worktreePath) => {
      // @ts-ignore - window.electron is injected by preload
      const result = await window.electron.git.hasUncommittedChanges(worktreePath)
      console.log('[Test] Has uncommitted changes:', result)
      return result
    }, worktreePath)

    expect(hasChanges).toBe(true)
    console.log('[Test] Confirmed uncommitted changes exist')

    // Commit the changes
    const commitResult = await window.evaluate(async (worktreePath) => {
      // @ts-ignore - window.electron is injected by preload
      const result = await window.electron.git.commitAll(worktreePath, 'Add new feature')
      console.log('[Test] Commit result:', result)
      return result
    }, worktreePath)

    expect(commitResult.success).toBe(true)
    console.log('[Test] Committed changes')

    // Verify no more uncommitted changes
    const hasChangesAfterCommit = await window.evaluate(async (worktreePath) => {
      // @ts-ignore - window.electron is injected by preload
      const result = await window.electron.git.hasUncommittedChanges(worktreePath)
      return result
    }, worktreePath)

    expect(hasChangesAfterCommit).toBe(false)
    console.log('[Test] Confirmed no uncommitted changes after commit')

    // Step 6: Merge the worktree branch back to main
    const mergeResult = await window.evaluate(async ({ repoPath, branch }) => {
      // @ts-ignore - window.electron is injected by preload
      const result = await window.electron.git.merge(repoPath, branch, 'main', false)
      console.log('[Test] Merge result:', result)
      return result
    }, { repoPath: tempRepoPath, branch: 'feature-branch' })

    expect(mergeResult.success).toBe(true)
    console.log('[Test] Merged feature-branch into main')

    // Step 7: Verify the file is modified in main repo
    const mainFileContent = fs.readFileSync(path.join(tempRepoPath, 'README.md'), 'utf-8')
    expect(mainFileContent).toBe(modifiedContent)
    console.log('[Test] Verified main repo file content after merge:', mainFileContent.trim())

    // Verify via git log that feature commit is in main
    const gitLog = execSync('git log --oneline -5', { cwd: tempRepoPath, encoding: 'utf-8' })
    console.log('[Test] Git log after merge:', gitLog)
    expect(gitLog).toContain('Add new feature')

    await closeApp(app)
  })

  test('create worktree with base branch and verify isolation', async () => {
    // Step 1: Create temporary git repo with multiple commits on main
    tempRepoPath = createTempGitRepo()
    
    // Add another commit to main
    fs.writeFileSync(path.join(tempRepoPath, 'main-file.txt'), 'Main branch content\n')
    execSync('git add main-file.txt', { cwd: tempRepoPath })
    execSync('git commit -m "Add main file"', { cwd: tempRepoPath })
    
    console.log('[Test] Created temp git repo with multiple commits')

    // Step 2: Launch app
    const { app, window } = await launchApp(tempRepoPath)
    await waitForDaemon(10000)
    await window.waitForLoadState('domcontentloaded')
    await window.waitForTimeout(2000)

    // Step 3: Create worktree from main
    const worktreeResult = await window.evaluate(async (repoPath) => {
      // @ts-ignore - window.electron is injected by preload
      const result = await window.electron.git.createWorktree(repoPath, 'isolated-branch', 'main')
      return result
    }, tempRepoPath)

    expect(worktreeResult.success).toBe(true)
    const worktreePath = worktreeResult.path
    console.log('[Test] Created worktree at:', worktreePath)

    // Step 4: Verify worktree has the files from main
    expect(fs.existsSync(path.join(worktreePath, 'README.md'))).toBe(true)
    expect(fs.existsSync(path.join(worktreePath, 'main-file.txt'))).toBe(true)
    console.log('[Test] Verified worktree has files from main branch')

    // Step 5: Modify file only in worktree
    const worktreeContent = 'Modified in worktree\n'
    await window.evaluate(async ({ worktreePath, content }) => {
      // @ts-ignore - window.electron is injected by preload
      await window.electron.filesystem.writeFile(
        worktreePath,
        `${worktreePath}/main-file.txt`,
        content
      )
    }, { worktreePath, content: worktreeContent })

    // Verify isolation - main should not have changes
    const mainContent = fs.readFileSync(path.join(tempRepoPath, 'main-file.txt'), 'utf-8')
    expect(mainContent).toBe('Main branch content\n')
    console.log('[Test] Verified main branch file is unchanged')

    // Worktree should have the new content
    const worktreeFileContent = fs.readFileSync(path.join(worktreePath, 'main-file.txt'), 'utf-8')
    expect(worktreeFileContent).toBe(worktreeContent)
    console.log('[Test] Verified worktree file has new content')

    // Step 6: Commit in worktree
    await window.evaluate(async (worktreePath) => {
      // @ts-ignore - window.electron is injected by preload
      await window.electron.git.commitAll(worktreePath, 'Modify main file in worktree')
    }, worktreePath)

    // Step 7: Merge back to main
    const mergeResult = await window.evaluate(async ({ repoPath, branch }) => {
      // @ts-ignore - window.electron is injected by preload
      return await window.electron.git.merge(repoPath, branch, 'main', false)
    }, { repoPath: tempRepoPath, branch: 'isolated-branch' })

    expect(mergeResult.success).toBe(true)

    // Step 8: Verify main now has the worktree changes
    const mainContentAfterMerge = fs.readFileSync(path.join(tempRepoPath, 'main-file.txt'), 'utf-8')
    expect(mainContentAfterMerge).toBe(worktreeContent)
    console.log('[Test] Verified main has worktree changes after merge')

    await closeApp(app)
  })
})
