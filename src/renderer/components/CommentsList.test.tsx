// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { createStore } from 'zustand/vanilla'
import CommentsList from './CommentsList'
import type { ReviewComment } from '../types'

// useAppStore is used only for the clipboard.
vi.mock('../store/app', () => {
  const useAppStore = <T,>(selector: (s: { clipboard: { writeText: (t: string) => void } }) => T): T =>
    selector({ clipboard: { writeText: vi.fn() } })
  return { useAppStore }
})

// Filesystem hook supplies code-context file content. Succeed so the fetch
// effect settles after one pass.
vi.mock('../hooks/useWorkspaceApis', () => ({
  useFilesystemApi: () => ({
    readFile: vi.fn().mockResolvedValue({ success: true, file: { content: 'a\nb\nc\nd\ne\nf\ng\n' } }),
  }),
}))

function makeComment(overrides: Partial<ReviewComment> = {}): ReviewComment {
  return {
    id: 'c1', filePath: 'src/foo.ts', lineNumber: 3, text: 'comment',
    commitHash: null, createdAt: 1, isOutdated: false, addressed: false,
    side: 'modified', ...overrides,
  }
}

function makeStores(comments: ReviewComment[], pushResult: unknown = { posted: 1, failed: [] }) {
  const promptHarness = vi.fn<(p: string) => Promise<boolean>>().mockResolvedValue(true)
  const markReviewCommentsAddressed = vi.fn<(ids: string[]) => void>()
  const deleteReviewComment = vi.fn<(id: string) => void>()
  const addTab = vi.fn()
  const pushReviewCommentsToGitHub = vi.fn<(c: ReviewComment[]) => Promise<unknown>>().mockResolvedValue(pushResult)

  const reviewComments = createStore<any>()(() => ({
    getReviewComments: () => comments,
    markReviewCommentsAddressed,
    deleteReviewComment,
  }))
  const gitController = createStore<any>()(() => ({ pushReviewCommentsToGitHub }))
  const workspace = createStore<any>()(() => ({
    workspace: { path: '/repo' },
    reviewComments,
    gitController,
    addTab,
    promptHarness,
  }))
  return { workspace, promptHarness, markReviewCommentsAddressed, pushReviewCommentsToGitHub }
}

describe('CommentsList', () => {
  beforeEach(() => { vi.clearAllMocks() })

  it('shows empty state when there are no comments', () => {
    const { workspace } = makeStores([])
    render(<CommentsList workspace={workspace} />)
    expect(screen.getByText('No review comments yet')).toBeDefined()
  })

  it('defaults to the unprompted filter and hides prompted comments', () => {
    const { workspace } = makeStores([
      makeComment({ id: 'c1', text: 'alpha', addressed: false }),
      makeComment({ id: 'c2', text: 'beta', addressed: true }),
    ])
    render(<CommentsList workspace={workspace} />)
    expect(screen.getByText('Unprompted (1)')).toBeDefined()
    expect(screen.getByText('Prompted (1)')).toBeDefined()
    expect(screen.getByText('alpha')).toBeDefined()
    expect(screen.queryByText('beta')).toBeNull()
  })

  it('switching to the prompted filter shows prompted comments', () => {
    const { workspace } = makeStores([
      makeComment({ id: 'c1', text: 'alpha', addressed: false }),
      makeComment({ id: 'c2', text: 'beta', addressed: true }),
    ])
    render(<CommentsList workspace={workspace} />)
    fireEvent.click(screen.getByText('Prompted (1)'))
    expect(screen.getByText('beta')).toBeDefined()
    expect(screen.queryByText('alpha')).toBeNull()
  })

  it('re-prompt sends the single comment to the harness and marks it', async () => {
    const { workspace, promptHarness, markReviewCommentsAddressed } = makeStores([
      makeComment({ id: 'c1', text: 'alpha', addressed: false }),
    ])
    render(<CommentsList workspace={workspace} />)
    fireEvent.click(screen.getByText('Prompt'))
    await waitFor(() => { expect(promptHarness).toHaveBeenCalled() })
    expect(promptHarness.mock.calls[0]![0]).toContain('alpha')
    expect(markReviewCommentsAddressed).toHaveBeenCalledWith(['c1'])
  })

  it('push to GitHub pushes unprompted comments and reports the result', async () => {
    const { workspace, pushReviewCommentsToGitHub, markReviewCommentsAddressed } = makeStores([
      makeComment({ id: 'c1', text: 'alpha', addressed: false }),
      makeComment({ id: 'c2', text: 'beta', addressed: true }),
    ])
    render(<CommentsList workspace={workspace} />)
    fireEvent.click(screen.getByText('Push to GitHub (1)'))
    await waitFor(() => { expect(screen.getByText('Pushed 1 comment(s) to GitHub')).toBeDefined() })
    expect(pushReviewCommentsToGitHub).toHaveBeenCalledWith([expect.objectContaining({ id: 'c1' })])
    expect(markReviewCommentsAddressed).toHaveBeenCalledWith(['c1'])
  })

  it('push to GitHub reports an error result', async () => {
    const { workspace, markReviewCommentsAddressed } = makeStores(
      [makeComment({ id: 'c1', text: 'alpha', addressed: false })],
      { error: 'No open PR found for this branch' },
    )
    render(<CommentsList workspace={workspace} />)
    fireEvent.click(screen.getByText('Push to GitHub (1)'))
    await waitFor(() => { expect(screen.getByText('Push failed: No open PR found for this branch')).toBeDefined() })
    expect(markReviewCommentsAddressed).not.toHaveBeenCalled()
  })
})
