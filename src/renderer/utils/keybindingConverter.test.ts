import { describe, it, expect } from 'vitest'
import { convertDirectKeybinding } from './keybindingConverter'

describe('convertDirectKeybinding', () => {
  it('converts CommandOrControl to $mod', () => {
    expect(convertDirectKeybinding('CommandOrControl+B')).toBe('$mod+b')
  })

  it('lowercases the last key part when it is a single character', () => {
    expect(convertDirectKeybinding('Ctrl+A')).toBe('Ctrl+a')
  })

  it('does not lowercase non-single-character final parts', () => {
    expect(convertDirectKeybinding('Ctrl+Enter')).toBe('Ctrl+Enter')
  })

  it('handles binding with no modifier', () => {
    expect(convertDirectKeybinding('B')).toBe('b')
  })

  it('handles multi-modifier bindings', () => {
    expect(convertDirectKeybinding('CommandOrControl+Shift+K')).toBe('$mod+Shift+k')
  })

  it('returns the binding unchanged when it is already lowercase', () => {
    expect(convertDirectKeybinding('Ctrl+b')).toBe('Ctrl+b')
  })

  it('handles Escape key (multi-char, no transform)', () => {
    expect(convertDirectKeybinding('Ctrl+Escape')).toBe('Ctrl+Escape')
  })

  it('handles uppercase single-letter key', () => {
    expect(convertDirectKeybinding('Alt+Z')).toBe('Alt+z')
  })
})
