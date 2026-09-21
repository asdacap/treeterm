// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
vi.mock('../store/app', () => ({ useAppStore: { getState: () => ({ registerTerminalVariants: vi.fn(), registerAiHarnessVariants: vi.fn(), registerCustomRunnerVariants: vi.fn() }) } }))
import SoundToggle from './SoundToggle'
import { defaultSettings, SoundSaveStatus, useSettingsStore } from '../store/settings'

const save = vi.fn<() => Promise<{ success: boolean }>>()
beforeEach(() => {
  save.mockReset().mockResolvedValue({ success: true })
  useSettingsStore.setState({ settings: defaultSettings, soundSaveState: { status: SoundSaveStatus.Idle }, settingsApi: { save, load: vi.fn(), onOpen: vi.fn() } })
})
afterEach(cleanup)
describe('SoundToggle', () => {
  it('saves off and on with accessible state', async () => {
    render(<SoundToggle />)
    expect(screen.getByRole('button').getAttribute('aria-pressed')).toBe('true')
    fireEvent.click(screen.getByRole('button', { name: 'Mute workspace notification sounds' }))
    await waitFor(() => { expect(screen.getByRole('button').getAttribute('aria-pressed')).toBe('false'); })
    expect(save).toHaveBeenCalledWith(expect.objectContaining({ notifications: { soundEnabled: false } }))
    fireEvent.click(screen.getByRole('button', { name: 'Enable workspace notification sounds' }))
    await waitFor(() => { expect(screen.getByRole('button').getAttribute('aria-pressed')).toBe('true'); })
  })
  it('disables while saving and suppresses duplicate store requests', async () => {
    let finish: () => void = () => {}
    save.mockImplementation(() => new Promise<{ success: boolean }>(resolve => { finish = (): void => { resolve({ success: true }) } }))
    render(<SoundToggle />)
    fireEvent.click(screen.getByRole('button'))
    expect(screen.getByRole('button').hasAttribute('disabled')).toBe(true)
    expect(screen.getByRole('status').textContent).toContain('Saving')
    await useSettingsStore.getState().toggleSound()
    expect(save).toHaveBeenCalledTimes(1)
    finish()
    await waitFor(() => { expect(screen.getByRole('button').hasAttribute('disabled')).toBe(false); })
  })
  it('reports an unsuccessful save response', async () => {
    save.mockResolvedValue({ success: false })
    render(<SoundToggle />)
    fireEvent.click(screen.getByRole('button'))
    await waitFor(() => { expect(screen.getByRole('alert').textContent).toContain('unsuccessful') })
    expect(useSettingsStore.getState().settings.notifications.soundEnabled).toBe(true)
  })
  it('reports failure and retains the saved preference', async () => {
    save.mockRejectedValue(new Error('disk unavailable'))
    render(<SoundToggle />)
    fireEvent.click(screen.getByRole('button'))
    await waitFor(() => { expect(screen.getByRole('alert').textContent).toContain('disk unavailable'); })
    expect(screen.getByRole('button').getAttribute('aria-pressed')).toBe('true')
  })
})
