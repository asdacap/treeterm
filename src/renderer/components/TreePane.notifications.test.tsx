// @vitest-environment jsdom
import { afterEach, beforeEach, describe, it, expect, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { createStore } from 'zustand/vanilla'
vi.mock('../store/app', async () => {
  const { create } = await import('zustand')
  return { useAppStore: create(() => ({
    sessionStores: new Map(),
    sessionNamesStore: createStore(() => ({ getSortedIds: (ids: string[]) => ids, reorderSession: vi.fn() })),
    registerTerminalVariants: vi.fn(), registerAiHarnessVariants: vi.fn(), registerCustomRunnerVariants: vi.fn(),
  })) }
})
import TreePane from './TreePane'
import { defaultSettings, SoundSaveStatus, useSettingsStore } from '../store/settings'

beforeEach(() => {
  useSettingsStore.setState({ settings: defaultSettings, soundSaveState: { status: SoundSaveStatus.Idle }, settingsApi: {
    save: vi.fn().mockResolvedValue({ success: true }), load: vi.fn(), onOpen: vi.fn(),
  } })
})
afterEach(cleanup)

describe('sidebar notification toggle', () => {
  it.each([false, true])('renders an accessible functional toggle with collapsed=%s', async isCollapsed => {
    render(<TreePane selectFolder={vi.fn()} isCollapsed={isCollapsed} onToggleCollapse={vi.fn()} />)
    const button = screen.getByRole('button', { name: 'Mute workspace notification sounds' })
    button.focus()
    expect(document.activeElement).toBe(button)
    expect(button.getAttribute('aria-pressed')).toBe('true')
    fireEvent.click(button)
    await waitFor(() => { expect(screen.getByRole('button', { name: 'Enable workspace notification sounds' }).getAttribute('aria-pressed')).toBe('false') })
  })
  it('preserves toggle preference across sidebar remounts', () => {
    useSettingsStore.setState({ settings: { ...defaultSettings, notifications: { soundEnabled: false } } })
    const view = render(<TreePane selectFolder={vi.fn()} isCollapsed={false} onToggleCollapse={vi.fn()} />)
    view.rerender(<TreePane selectFolder={vi.fn()} isCollapsed onToggleCollapse={vi.fn()} />)
    expect(screen.getByRole('button', { name: 'Enable workspace notification sounds' }).getAttribute('aria-pressed')).toBe('false')
  })
})
