import { describe, it, expect, vi, beforeEach } from 'vitest'

vi.stubGlobal('window', { innerWidth: 1000, innerHeight: 800 })

import { clampContextMenuPosition } from './contextMenuPosition'

describe('clampContextMenuPosition', () => {
  it('returns unchanged position when within bounds', () => {
    expect(clampContextMenuPosition(100, 200)).toEqual({ x: 100, y: 200 })
  })

  it('clamps x when overflowing right edge', () => {
    // default menuWidth=160, so max x = 1000-160 = 840
    expect(clampContextMenuPosition(900, 200)).toEqual({ x: 840, y: 200 })
  })

  it('clamps y when overflowing bottom edge', () => {
    // default menuHeight=200, so max y = 800-200 = 600
    expect(clampContextMenuPosition(100, 700)).toEqual({ x: 100, y: 600 })
  })

  it('clamps both x and y when both overflow', () => {
    expect(clampContextMenuPosition(950, 750)).toEqual({ x: 840, y: 600 })
  })

  it('respects custom menuWidth and menuHeight', () => {
    // menuWidth=300 → max x = 700, menuHeight=400 → max y = 400
    expect(clampContextMenuPosition(800, 500, 300, 400)).toEqual({ x: 700, y: 400 })
  })

  it('uses default menuWidth=160 and menuHeight=200', () => {
    // Exactly at the boundary
    expect(clampContextMenuPosition(840, 600)).toEqual({ x: 840, y: 600 })
    // One pixel over
    expect(clampContextMenuPosition(841, 601)).toEqual({ x: 840, y: 600 })
  })
})
