// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { createStore } from 'zustand/vanilla'
import { PromptGitHubCommentsButton } from './PromptGitHubCommentsButton'

function makeStores(prInfo: any) {
  const promptHarness = vi.fn<(prompt: string) => void>()
  const gitControllerStore = createStore<any>()(() => ({
    prInfo,
  }))
  const workspaceStore = createStore<any>()(() => ({
    gitController: gitControllerStore,
    promptHarness,
  }))
  return { workspaceStore, promptHarness }
}

describe('PromptGitHubCommentsButton', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('renders null when prInfo is null', () => {
    const { workspaceStore } = makeStores(null)
    const { container } = render(<PromptGitHubCommentsButton workspace={workspaceStore} />)
    expect(container.innerHTML).toBe('')
  })

  it('renders null when unresolvedThreads is empty', () => {
    const { workspaceStore } = makeStores({ unresolvedThreads: [], unresolvedCount: 0 })
    const { container } = render(<PromptGitHubCommentsButton workspace={workspaceStore} />)
    expect(container.innerHTML).toBe('')
  })

  it('renders button with unresolved count when threads exist', () => {
    const { workspaceStore } = makeStores({
      unresolvedThreads: [{ path: 'a.ts', line: 1, author: 'x', body: 'fix' }],
      unresolvedCount: 3,
    })
    render(<PromptGitHubCommentsButton workspace={workspaceStore} />)
    expect(screen.getByText('Address PR Comments (3)')).toBeDefined()
  })

  it('calls promptHarness when button is clicked', () => {
    const { workspaceStore, promptHarness } = makeStores({
      unresolvedThreads: [{ path: 'a.ts', line: 1, author: 'x', body: 'fix' }],
      unresolvedCount: 1,
    })
    render(<PromptGitHubCommentsButton workspace={workspaceStore} />)
    fireEvent.click(screen.getByText('Address PR Comments (1)'))
    expect(promptHarness).toHaveBeenCalledWith('Pull Github comment and address')
  })
})
