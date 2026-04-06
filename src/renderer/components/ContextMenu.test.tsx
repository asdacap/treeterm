// @vitest-environment jsdom
import { describe, it, expect } from 'vitest'
import { render, screen } from '@testing-library/react'

import ContextMenu from './ContextMenu'

describe('ContextMenu', () => {
  it('renders null when activeMenuId does not match menuId', () => {
    const { container } = render(
      <ContextMenu menuId="my-menu" activeMenuId="other-menu" position={{ x: 0, y: 0 }}>
        <div>menu item</div>
      </ContextMenu>
    )
    expect(container.innerHTML).toBe('')
  })

  it('renders null when activeMenuId is null', () => {
    const { container } = render(
      <ContextMenu menuId="my-menu" activeMenuId={null} position={{ x: 0, y: 0 }}>
        <div>menu item</div>
      </ContextMenu>
    )
    expect(container.innerHTML).toBe('')
  })

  it('renders children when activeMenuId matches menuId', () => {
    render(
      <ContextMenu menuId="my-menu" activeMenuId="my-menu" position={{ x: 0, y: 0 }}>
        <div>menu item</div>
      </ContextMenu>
    )
    expect(screen.getByText('menu item')).toBeDefined()
  })

  it('positions the menu at the given coordinates', () => {
    const { container } = render(
      <ContextMenu menuId="my-menu" activeMenuId="my-menu" position={{ x: 100, y: 200 }}>
        <div>item</div>
      </ContextMenu>
    )
    const menu = container.querySelector('.context-menu') as HTMLElement
    expect(menu.style.top).toBe('200px')
    expect(menu.style.left).toBe('100px')
  })
})
