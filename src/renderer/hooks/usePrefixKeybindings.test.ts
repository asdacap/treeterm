import { describe, it, expect, vi } from 'vitest'

// Mock modules that pull in browser-only deps (xterm) before importing the module under test
vi.mock('../store/settings', () => ({
  useSettingsStore: { getState: vi.fn() }
}))
vi.mock('../store/prefixMode', () => ({
  usePrefixModeStore: { getState: vi.fn() }
}))
vi.mock('tinykeys', () => ({
  parseKeybinding: vi.fn()
}))
vi.mock('../utils/keybindingConverter', () => ({
  convertDirectKeybinding: vi.fn()
}))

import { matchesKeybinding } from './usePrefixKeybindings'

function createKeyEvent(overrides: Partial<KeyboardEvent> = {}): KeyboardEvent {
  return {
    key: '',
    ctrlKey: false,
    shiftKey: false,
    altKey: false,
    metaKey: false,
    ...overrides
  } as KeyboardEvent
}

describe('matchesKeybinding', () => {
  it('matches plain key with no modifiers', () => {
    const event = createKeyEvent({ key: 't' })
    expect(matchesKeybinding(event, [], 't')).toBe(true)
  })

  it('does not match different key', () => {
    const event = createKeyEvent({ key: 'x' })
    expect(matchesKeybinding(event, [], 't')).toBe(false)
  })

  it('matches key with Meta modifier', () => {
    const event = createKeyEvent({ key: 'b', metaKey: true })
    expect(matchesKeybinding(event, ['Meta'], 'b')).toBe(true)
  })

  it('fails when Meta required but not pressed', () => {
    const event = createKeyEvent({ key: 'b', metaKey: false })
    expect(matchesKeybinding(event, ['Meta'], 'b')).toBe(false)
  })

  it('fails when extra modifier pressed', () => {
    const event = createKeyEvent({ key: 'b', metaKey: true, shiftKey: true })
    expect(matchesKeybinding(event, ['Meta'], 'b')).toBe(false)
  })

  it('matches key with Control modifier', () => {
    const event = createKeyEvent({ key: 'a', ctrlKey: true })
    expect(matchesKeybinding(event, ['Control'], 'a')).toBe(true)
  })

  it('matches key with multiple modifiers', () => {
    const event = createKeyEvent({ key: 'k', ctrlKey: true, shiftKey: true })
    expect(matchesKeybinding(event, ['Control', 'Shift'], 'k')).toBe(true)
  })

  it('fails with multiple modifiers when one missing', () => {
    const event = createKeyEvent({ key: 'k', ctrlKey: true, shiftKey: false })
    expect(matchesKeybinding(event, ['Control', 'Shift'], 'k')).toBe(false)
  })

  it('matches case-insensitively', () => {
    const event = createKeyEvent({ key: 'T' })
    expect(matchesKeybinding(event, [], 't')).toBe(true)
  })

  it('matches Alt modifier', () => {
    const event = createKeyEvent({ key: 'x', altKey: true })
    expect(matchesKeybinding(event, ['Alt'], 'x')).toBe(true)
  })

  it('matches all four modifiers together', () => {
    const event = createKeyEvent({
      key: 'z',
      ctrlKey: true,
      shiftKey: true,
      altKey: true,
      metaKey: true
    })
    expect(matchesKeybinding(event, ['Control', 'Shift', 'Alt', 'Meta'], 'z')).toBe(true)
  })
})
