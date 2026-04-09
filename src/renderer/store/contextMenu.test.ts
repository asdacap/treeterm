// @vitest-environment jsdom
import { describe, it, expect, beforeEach, vi } from 'vitest'

vi.mock('./app', () => ({
  useAppStore: {
    getState: () => ({
      getViewportSize: () => ({ width: 1024, height: 768 }),
    }),
  },
}))

import { useContextMenuStore, handleClickOutside, installClickListener } from './contextMenu'

describe('ContextMenuStore', () => {
  beforeEach(() => {
    useContextMenuStore.setState({ activeMenuId: null, position: { x: 0, y: 0 } })
  })

  it.each([
    { menuId: 'menu-a', x: 100, y: 200 },
    { menuId: 'menu-b', x: 50, y: 300 },
  ])('open($menuId, $x, $y) sets activeMenuId and position', ({ menuId, x, y }) => {
    useContextMenuStore.getState().open(menuId, x, y)

    const state = useContextMenuStore.getState()
    expect(state.activeMenuId).toBe(menuId)
    expect(state.position).toEqual({ x, y })
  })

  it('opening a second menu replaces the first (mutual exclusivity)', () => {
    useContextMenuStore.getState().open('menu-a', 10, 20)
    useContextMenuStore.getState().open('menu-b', 30, 40)

    const state = useContextMenuStore.getState()
    expect(state.activeMenuId).toBe('menu-b')
    expect(state.position).toEqual({ x: 30, y: 40 })
  })

  it('close sets activeMenuId to null', () => {
    useContextMenuStore.getState().open('menu-a', 10, 20)
    useContextMenuStore.getState().close()

    expect(useContextMenuStore.getState().activeMenuId).toBeNull()
  })

  it.each([
    { desc: 'closes menu when clicking outside', closest: null, expectClosed: true },
    { desc: 'keeps menu when clicking on .context-menu', closest: 'match', expectClosed: false },
  ])('handleClickOutside $desc', ({ closest, expectClosed }) => {
    useContextMenuStore.getState().open('menu-a', 10, 20)

    const target = { closest: () => closest }
    handleClickOutside({ target } as unknown as { target: EventTarget | null })

    if (expectClosed) {
      expect(useContextMenuStore.getState().activeMenuId).toBeNull()
    } else {
      expect(useContextMenuStore.getState().activeMenuId).toBe('menu-a')
    }
  })

  it('handleClickOutside does nothing when no menu is open', () => {
    const target = { closest: () => null }
    handleClickOutside({ target } as unknown as { target: EventTarget | null })

    expect(useContextMenuStore.getState().activeMenuId).toBeNull()
  })

  it('installClickListener registers capture-phase handler', () => {
    const addEventListener = vi.fn()
    installClickListener({ addEventListener })

    expect(addEventListener).toHaveBeenCalledWith('click', expect.any(Function), true)
  })

  it('clamps position to viewport bounds', () => {
    useContextMenuStore.getState().open('menu-a', 950, 700)

    const { position } = useContextMenuStore.getState()
    // default menuWidth=160, menuHeight=200
    expect(position.x).toBe(1024 - 160)
    expect(position.y).toBe(768 - 200)
  })
})
