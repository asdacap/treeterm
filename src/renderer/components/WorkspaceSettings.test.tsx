// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { createStore } from 'zustand/vanilla'
import WorkspaceSettings from './WorkspaceSettings'

const applications = new Map([
  ['terminal', { id: 'terminal', name: 'Terminal', showInNewTabMenu: true }],
  ['editor', { id: 'editor', name: 'Editor', showInNewTabMenu: true }],
  ['hidden', { id: 'hidden', name: 'Hidden', showInNewTabMenu: false }],
]) as unknown as Map<string, import('../types').Application>

function makeWorkspaceStore(overrides: { settings?: { defaultApplicationId: string }; metadata?: Record<string, string>; workspace?: Record<string, unknown> } = {}) {
  const updateMetadata = vi.fn<(...args: any[]) => void>()
  const updateSettings = vi.fn<(...args: any[]) => void>()
  const metadata = overrides.metadata ?? { displayName: 'My Workspace', description: 'A test workspace' }
  const ws = {
    id: 'ws-1',
    name: 'test-workspace',
    path: '/test',
    metadata,
    ...overrides.workspace,
  }
  const store = createStore<any>()(() => ({
    workspace: ws,
    metadata,
    settings: overrides.settings ?? { defaultApplicationId: '' },
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
    render(<WorkspaceSettings workspace={store} applications={applications} />)
    const input = screen.getByDisplayValue('My Workspace')
    expect(input).toBeDefined()
  })

  it('renders description textarea', () => {
    const { store } = makeWorkspaceStore()
    render(<WorkspaceSettings workspace={store} applications={applications} />)
    const textarea = screen.getByDisplayValue('A test workspace')
    expect(textarea).toBeDefined()
  })

  it('calls updateMetadata with displayName on name blur', () => {
    const { store, updateMetadata } = makeWorkspaceStore()
    render(<WorkspaceSettings workspace={store} applications={applications} />)

    const input = screen.getByDisplayValue('My Workspace')
    fireEvent.change(input, { target: { value: 'New Name' } })
    fireEvent.blur(input)

    expect(updateMetadata).toHaveBeenCalledWith('displayName', 'New Name', 'workspaceSettingsEditName')
  })

  it('does not call updateMetadata when name is empty on blur', () => {
    const { store, updateMetadata } = makeWorkspaceStore()
    render(<WorkspaceSettings workspace={store} applications={applications} />)

    const input = screen.getByDisplayValue('My Workspace')
    fireEvent.change(input, { target: { value: '   ' } })
    fireEvent.blur(input)

    expect(updateMetadata).not.toHaveBeenCalledWith('displayName', expect.anything(), expect.anything())
  })

  it('calls updateMetadata with description on textarea blur', () => {
    const { store, updateMetadata } = makeWorkspaceStore()
    render(<WorkspaceSettings workspace={store} applications={applications} />)

    const textarea = screen.getByDisplayValue('A test workspace')
    fireEvent.change(textarea, { target: { value: 'Updated desc' } })
    fireEvent.blur(textarea)

    expect(updateMetadata).toHaveBeenCalledWith('description', 'Updated desc', 'workspaceSettingsEditDescription')
  })

  it('calls updateSettings on default app change', () => {
    const { store, updateSettings } = makeWorkspaceStore()
    render(<WorkspaceSettings workspace={store} applications={applications} />)

    const select = screen.getByDisplayValue('Use Global Default')
    fireEvent.change(select, { target: { value: 'terminal' } })

    expect(updateSettings).toHaveBeenCalledWith({ defaultApplicationId: 'terminal' })
  })

  it('renders only showInNewTabMenu apps in dropdown', () => {
    const { store } = makeWorkspaceStore()
    render(<WorkspaceSettings workspace={store} applications={applications} />)

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
    render(<WorkspaceSettings workspace={store} applications={applications} />)

    // Initially collapsed
    expect(screen.queryByText(/"ws-1"/)).toBeNull()

    // Click to expand
    fireEvent.click(screen.getByText('Raw Workspace JSON'))

    // Now JSON is visible
    expect(screen.getByText(/"ws-1"/)).toBeDefined()
  })

  it('falls back to workspace name when no displayName', () => {
    const { store } = makeWorkspaceStore({ metadata: {} })
    render(<WorkspaceSettings workspace={store} applications={applications} />)
    expect(screen.getByDisplayValue('test-workspace')).toBeDefined()
  })
})
