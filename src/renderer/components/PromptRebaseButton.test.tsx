// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { createStore } from 'zustand/vanilla'
import { PromptRebaseButton } from './PromptRebaseButton'

function makeStores(opts: {
  hasConflictsWithParent: boolean
  gitBranch: string | null
  parentId: string | null
  parentWorkspace?: { gitBranch: string | null } | null
}) {
  const promptHarness = vi.fn()
  const lookupWorkspace = vi.fn((_id: string) =>
    opts.parentWorkspace !== undefined ? opts.parentWorkspace : { gitBranch: 'main' }
  )
  const gitControllerStore = createStore<any>()(() => ({
    hasConflictsWithParent: opts.hasConflictsWithParent,
  }))
  const workspaceStore = createStore<any>()(() => ({
    gitController: gitControllerStore,
    promptHarness,
    workspace: {
      gitBranch: opts.gitBranch,
      parentId: opts.parentId,
    },
    lookupWorkspace,
  }))
  return { workspaceStore, promptHarness, lookupWorkspace }
}

describe('PromptRebaseButton', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('renders null when hasConflictsWithParent is false', () => {
    const { workspaceStore } = makeStores({
      hasConflictsWithParent: false,
      gitBranch: 'feat/x',
      parentId: 'p1',
    })
    const { container } = render(<PromptRebaseButton workspace={workspaceStore} />)
    expect(container.innerHTML).toBe('')
  })

  it('renders null when workspace has no gitBranch', () => {
    const { workspaceStore } = makeStores({
      hasConflictsWithParent: true,
      gitBranch: null,
      parentId: 'p1',
    })
    const { container } = render(<PromptRebaseButton workspace={workspaceStore} />)
    expect(container.innerHTML).toBe('')
  })

  it('renders null when workspace has no parentId', () => {
    const { workspaceStore } = makeStores({
      hasConflictsWithParent: true,
      gitBranch: 'feat/x',
      parentId: null,
    })
    const { container } = render(<PromptRebaseButton workspace={workspaceStore} />)
    expect(container.innerHTML).toBe('')
  })

  it('renders null when parent workspace has no gitBranch', () => {
    const { workspaceStore } = makeStores({
      hasConflictsWithParent: true,
      gitBranch: 'feat/x',
      parentId: 'p1',
      parentWorkspace: { gitBranch: null },
    })
    const { container } = render(<PromptRebaseButton workspace={workspaceStore} />)
    expect(container.innerHTML).toBe('')
  })

  it('renders null when lookupWorkspace returns null', () => {
    const { workspaceStore } = makeStores({
      hasConflictsWithParent: true,
      gitBranch: 'feat/x',
      parentId: 'p1',
      parentWorkspace: null,
    })
    const { container } = render(<PromptRebaseButton workspace={workspaceStore} />)
    expect(container.innerHTML).toBe('')
  })

  it('renders button when all conditions met', () => {
    const { workspaceStore } = makeStores({
      hasConflictsWithParent: true,
      gitBranch: 'feat/x',
      parentId: 'p1',
      parentWorkspace: { gitBranch: 'main' },
    })
    render(<PromptRebaseButton workspace={workspaceStore} />)
    expect(screen.getByText('Prompt Rebase')).toBeDefined()
  })

  it('calls promptHarness with rebase command including branch names', () => {
    const { workspaceStore, promptHarness } = makeStores({
      hasConflictsWithParent: true,
      gitBranch: 'feat/x',
      parentId: 'p1',
      parentWorkspace: { gitBranch: 'main' },
    })
    render(<PromptRebaseButton workspace={workspaceStore} />)
    fireEvent.click(screen.getByText('Prompt Rebase'))
    expect(promptHarness).toHaveBeenCalledWith('rebase local branch feat/x onto main')
  })
})
