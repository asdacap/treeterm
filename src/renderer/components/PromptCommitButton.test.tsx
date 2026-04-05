// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import React from 'react'
import { createStore } from 'zustand/vanilla'
import { PromptCommitButton } from './PromptCommitButton'

function makeStores(git: { hasUncommittedChanges: boolean; hasConflictsWithParent: boolean }) {
  const promptHarness = vi.fn()
  const gitControllerStore = createStore<any>()(() => ({
    hasUncommittedChanges: git.hasUncommittedChanges,
    hasConflictsWithParent: git.hasConflictsWithParent,
  }))
  const workspaceStore = createStore<any>()(() => ({
    gitController: gitControllerStore,
    promptHarness,
  }))
  return { workspaceStore, promptHarness }
}

describe('PromptCommitButton', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('renders null when hasUncommittedChanges is false', () => {
    const { workspaceStore } = makeStores({ hasUncommittedChanges: false, hasConflictsWithParent: false })
    const { container } = render(<PromptCommitButton workspace={workspaceStore} />)
    expect(container.innerHTML).toBe('')
  })

  it('renders null when hasConflictsWithParent is true', () => {
    const { workspaceStore } = makeStores({ hasUncommittedChanges: true, hasConflictsWithParent: true })
    const { container } = render(<PromptCommitButton workspace={workspaceStore} />)
    expect(container.innerHTML).toBe('')
  })

  it('renders button when hasUncommittedChanges=true and hasConflictsWithParent=false', () => {
    const { workspaceStore } = makeStores({ hasUncommittedChanges: true, hasConflictsWithParent: false })
    render(<PromptCommitButton workspace={workspaceStore} />)
    expect(screen.getByText('Prompt Commit')).toBeDefined()
  })

  it('calls promptHarness with "commit" when button is clicked', () => {
    const { workspaceStore, promptHarness } = makeStores({ hasUncommittedChanges: true, hasConflictsWithParent: false })
    render(<PromptCommitButton workspace={workspaceStore} />)
    fireEvent.click(screen.getByText('Prompt Commit'))
    expect(promptHarness).toHaveBeenCalledWith('commit')
  })
})
