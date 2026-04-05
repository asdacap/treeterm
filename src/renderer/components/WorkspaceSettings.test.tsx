// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import React from 'react'
import { createStore } from 'zustand/vanilla'
import WorkspaceSettings from './WorkspaceSettings'

// Mock useAppStore to return application list
vi.mock('../store/app', () => ({
  useAppStore: vi.fn((selector: (s: any) => any) => selector({
    applications: {
      terminal: { id: 'terminal', name: 'Terminal', showInNewTabMenu: true },
      editor: { id: 'editor', name: 'Editor', showInNewTabMenu: true },
      hidden: { id: 'hidden', name: 'Hidden', showInNewTabMenu: false },
    }
  })),
}))

function makeWorkspaceStore(overrides: Record<string, any> = {}) {
  const updateMetadata = vi.fn()
  const updateSettings = vi.fn()
  const ws = {
    id: 'ws-1',
    name: 'test-workspace',
    path: '/test',
    metadata: { displayName: 'My Workspace', description: 'A test workspace' },
    settings: { defaultApplicationId: '' },
    ...overrides,
  }
  const store = createStore<any>()(() => ({
    workspace: ws,
    updateMetadata,
    updateSettings,
  }))
  return { store, updateMetadata, updateSettings }
}

describe('WorkspaceSettings', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('renders name input with displayName from metadata', () => {
    const { store } = makeWorkspaceStore()
    render(<WorkspaceSettings workspace={store} />)
    const input = screen.getByDisplayValue('My Workspace')
    expect(input).toBeDefined()
  })

  it('renders description textarea', () => {
    const { store } = makeWorkspaceStore()
    render(<WorkspaceSettings workspace={store} />)
    const textarea = screen.getByDisplayValue('A test workspace')
    expect(textarea).toBeDefined()
  })

  it('calls updateMetadata with displayName on name blur', () => {
    const { store, updateMetadata } = makeWorkspaceStore()
    render(<WorkspaceSettings workspace={store} />)

    const input = screen.getByDisplayValue('My Workspace')
    fireEvent.change(input, { target: { value: 'New Name' } })
    fireEvent.blur(input)

    expect(updateMetadata).toHaveBeenCalledWith('displayName', 'New Name')
  })

  it('does not call updateMetadata when name is empty on blur', () => {
    const { store, updateMetadata } = makeWorkspaceStore()
    render(<WorkspaceSettings workspace={store} />)

    const input = screen.getByDisplayValue('My Workspace')
    fireEvent.change(input, { target: { value: '   ' } })
    fireEvent.blur(input)

    expect(updateMetadata).not.toHaveBeenCalledWith('displayName', expect.anything())
  })

  it('calls updateMetadata with description on textarea blur', () => {
    const { store, updateMetadata } = makeWorkspaceStore()
    render(<WorkspaceSettings workspace={store} />)

    const textarea = screen.getByDisplayValue('A test workspace')
    fireEvent.change(textarea, { target: { value: 'Updated desc' } })
    fireEvent.blur(textarea)

    expect(updateMetadata).toHaveBeenCalledWith('description', 'Updated desc')
  })

  it('calls updateSettings on default app change', () => {
    const { store, updateSettings } = makeWorkspaceStore()
    render(<WorkspaceSettings workspace={store} />)

    const select = screen.getByDisplayValue('Use Global Default')
    fireEvent.change(select, { target: { value: 'terminal' } })

    expect(updateSettings).toHaveBeenCalledWith({ defaultApplicationId: 'terminal' })
  })

  it('renders only showInNewTabMenu apps in dropdown', () => {
    const { store } = makeWorkspaceStore()
    render(<WorkspaceSettings workspace={store} />)

    expect(screen.getByText('Terminal')).toBeDefined()
    expect(screen.getByText('Editor')).toBeDefined()
    // 'Hidden' should NOT be in the dropdown (showInNewTabMenu: false)
    // It could still appear as text elsewhere, so check within options specifically
    const options = screen.getAllByRole('option')
    const optionTexts = options.map((o) => o.textContent)
    expect(optionTexts).toContain('Terminal')
    expect(optionTexts).toContain('Editor')
    expect(optionTexts).not.toContain('Hidden')
  })

  it('toggles raw JSON on click', () => {
    const { store } = makeWorkspaceStore()
    render(<WorkspaceSettings workspace={store} />)

    // Initially collapsed
    expect(screen.queryByText(/"ws-1"/)).toBeNull()

    // Click to expand
    fireEvent.click(screen.getByText('Raw Workspace JSON'))

    // Now JSON is visible
    expect(screen.getByText(/"ws-1"/)).toBeDefined()
  })

  it('falls back to workspace name when no displayName', () => {
    const { store } = makeWorkspaceStore({ metadata: {} })
    render(<WorkspaceSettings workspace={store} />)
    expect(screen.getByDisplayValue('test-workspace')).toBeDefined()
  })
})
