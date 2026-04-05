// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen } from '@testing-library/react'
import React from 'react'

let mockActiveMenuId: string | null = null
let mockPosition = { x: 0, y: 0 }

vi.mock('../store/contextMenu', () => ({
  useContextMenuStore: (selector: (s: any) => any) =>
    selector({ activeMenuId: mockActiveMenuId, position: mockPosition }),
}))

import ContextMenu from './ContextMenu'

describe('ContextMenu', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    mockActiveMenuId = null
    mockPosition = { x: 0, y: 0 }
  })

  it('renders null when activeMenuId does not match menuId', () => {
    mockActiveMenuId = 'other-menu'
    const { container } = render(
      <ContextMenu menuId="my-menu">
        <div>menu item</div>
      </ContextMenu>
    )
    expect(container.innerHTML).toBe('')
  })

  it('renders null when activeMenuId is null', () => {
    mockActiveMenuId = null
    const { container } = render(
      <ContextMenu menuId="my-menu">
        <div>menu item</div>
      </ContextMenu>
    )
    expect(container.innerHTML).toBe('')
  })

  it('renders children when activeMenuId matches menuId', () => {
    mockActiveMenuId = 'my-menu'
    render(
      <ContextMenu menuId="my-menu">
        <div>menu item</div>
      </ContextMenu>
    )
    expect(screen.getByText('menu item')).toBeDefined()
  })

  it('positions the menu at the store position coordinates', () => {
    mockActiveMenuId = 'my-menu'
    mockPosition = { x: 100, y: 200 }
    const { container } = render(
      <ContextMenu menuId="my-menu">
        <div>item</div>
      </ContextMenu>
    )
    const menu = container.querySelector('.context-menu') as HTMLElement
    expect(menu.style.top).toBe('200px')
    expect(menu.style.left).toBe('100px')
  })
})
