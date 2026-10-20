// @vitest-environment jsdom
import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent, act } from '@testing-library/react'
import { createStore } from 'zustand/vanilla'
import { ParentReviewCommentsButton } from './ParentReviewCommentsButton'

function makeComment(id: string, addressed: boolean) {
  return { id, filePath: 'a.ts', lineNumber: 1, text: 't', commitHash: null, createdAt: 1, isOutdated: false, addressed, side: 'modified' }
}

function makeStores(opts: {
  isDiffCleanFromParent: boolean
  parentId: string | undefined
  parentComments: ReturnType<typeof makeComment>[] | null
}) {
  const parentStore = opts.parentComments === null ? undefined : (() => {
    const metadata = { reviewComments: JSON.stringify(opts.parentComments) }
    const store = createStore<any>()(() => ({ metadata }))
    const reviewComments = createStore<any>()(() => ({
      getReviewComments: (): unknown => JSON.parse((store.getState() as { metadata: { reviewComments: string } }).metadata.reviewComments),
    }))
    store.setState({ reviewComments })
    return store
  })()
  let resolvePrompt: (sent: boolean) => void = () => {}
  const promptParentReviewComments = vi.fn(() => new Promise<boolean>((resolve) => { resolvePrompt = resolve }))
  const workspaceStore = createStore<any>()(() => ({
    gitController: createStore<any>()(() => ({ isDiffCleanFromParent: opts.isDiffCleanFromParent })),
    workspace: { parentId: opts.parentId },
    lookupWorkspaceStore: () => parentStore,
    promptParentReviewComments,
  }))
  return { workspaceStore, parentStore, promptParentReviewComments, resolvePrompt: (sent: boolean) => { resolvePrompt(sent) } }
}

describe('ParentReviewCommentsButton', () => {
  it('renders null when the diff from parent is not clean', () => {
    const { workspaceStore } = makeStores({ isDiffCleanFromParent: false, parentId: 'p', parentComments: [makeComment('c1', false)] })
    const { container } = render(<ParentReviewCommentsButton workspace={workspaceStore} />)
    expect(container.innerHTML).toBe('')
  })

  it('renders null when the workspace has no parent', () => {
    const { workspaceStore } = makeStores({ isDiffCleanFromParent: true, parentId: undefined, parentComments: [makeComment('c1', false)] })
    const { container } = render(<ParentReviewCommentsButton workspace={workspaceStore} />)
    expect(container.innerHTML).toBe('')
  })

  it('renders null when the parent store is not loaded', () => {
    const { workspaceStore } = makeStores({ isDiffCleanFromParent: true, parentId: 'p', parentComments: null })
    const { container } = render(<ParentReviewCommentsButton workspace={workspaceStore} />)
    expect(container.innerHTML).toBe('')
  })

  it('renders null when the parent has no unaddressed comments', () => {
    const { workspaceStore } = makeStores({ isDiffCleanFromParent: true, parentId: 'p', parentComments: [makeComment('c1', true)] })
    const { container } = render(<ParentReviewCommentsButton workspace={workspaceStore} />)
    expect(container.innerHTML).toBe('')
  })

  it('shows the unaddressed count and re-renders when parent comments change', () => {
    const { workspaceStore, parentStore } = makeStores({
      isDiffCleanFromParent: true, parentId: 'p', parentComments: [makeComment('c1', false), makeComment('c2', true)],
    })
    render(<ParentReviewCommentsButton workspace={workspaceStore} />)
    expect(screen.getByText('Address Parent Comments (1)')).toBeTruthy()
    act(() => {
      parentStore!.setState({ metadata: { reviewComments: JSON.stringify([makeComment('c1', false), makeComment('c3', false)]) } })
    })
    expect(screen.getByText('Address Parent Comments (2)')).toBeTruthy()
  })

  it('disables the button while prompting and calls the action', async () => {
    const { workspaceStore, promptParentReviewComments, resolvePrompt } = makeStores({
      isDiffCleanFromParent: true, parentId: 'p', parentComments: [makeComment('c1', false)],
    })
    render(<ParentReviewCommentsButton workspace={workspaceStore} />)
    const button = screen.getByRole('button')
    fireEvent.click(button)
    expect(promptParentReviewComments).toHaveBeenCalledOnce()
    expect((button as HTMLButtonElement).disabled).toBe(true)
    await act(async () => { resolvePrompt(true); await Promise.resolve() })
    expect((button as HTMLButtonElement).disabled).toBe(false)
  })
})
