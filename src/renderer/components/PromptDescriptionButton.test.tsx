// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import React from 'react'
import { createStore } from 'zustand/vanilla'
import { PromptDescriptionButton } from './PromptDescriptionButton'

function makeStores() {
  const promptHarness = vi.fn()
  const workspaceStore = createStore<any>()(() => ({
    promptHarness,
  }))
  return { workspaceStore, promptHarness }
}

describe('PromptDescriptionButton', () => {
  const onDismiss = vi.fn()

  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('renders "Prompt Description" and "Skip" buttons', () => {
    const { workspaceStore } = makeStores()
    render(
      <PromptDescriptionButton description="do things" workspace={workspaceStore} onDismiss={onDismiss} />
    )
    expect(screen.getByText('Prompt Description')).toBeDefined()
    expect(screen.getByText('Skip')).toBeDefined()
  })

  it('calls promptHarness with description and onDismiss when prompt button is clicked', () => {
    const { workspaceStore, promptHarness } = makeStores()
    render(
      <PromptDescriptionButton description="fix the bug" workspace={workspaceStore} onDismiss={onDismiss} />
    )
    fireEvent.click(screen.getByText('Prompt Description'))
    expect(promptHarness).toHaveBeenCalledWith('fix the bug')
    expect(onDismiss).toHaveBeenCalledTimes(1)
  })

  it('calls onDismiss when skip button is clicked', () => {
    const { workspaceStore } = makeStores()
    render(
      <PromptDescriptionButton description="desc" workspace={workspaceStore} onDismiss={onDismiss} />
    )
    fireEvent.click(screen.getByText('Skip'))
    expect(onDismiss).toHaveBeenCalledTimes(1)
  })

  it('does not call promptHarness when skip button is clicked', () => {
    const { workspaceStore, promptHarness } = makeStores()
    render(
      <PromptDescriptionButton description="desc" workspace={workspaceStore} onDismiss={onDismiss} />
    )
    fireEvent.click(screen.getByText('Skip'))
    expect(promptHarness).not.toHaveBeenCalled()
  })
})
