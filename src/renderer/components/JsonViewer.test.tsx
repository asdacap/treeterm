// @vitest-environment jsdom
import { describe, it, expect } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import React from 'react'
import JsonViewer from './JsonViewer'

describe('JsonViewer', () => {
  it('renders null as "null"', () => {
    render(<JsonViewer data={null} />)
    expect(screen.getByText('null')).toBeDefined()
  })

  it('renders string values with quotes', () => {
    render(<JsonViewer data="hello" />)
    // The component renders &quot; entities around strings
    const el = document.querySelector('.json-viewer-string')!
    expect(el.textContent).toBe('"hello"')
  })

  it('renders number values', () => {
    render(<JsonViewer data={42} />)
    expect(screen.getByText('42')).toBeDefined()
  })

  it('renders boolean values', () => {
    render(<JsonViewer data={true} />)
    expect(screen.getByText('true')).toBeDefined()
  })

  it('renders empty object as "{}"', () => {
    const { container } = render(<JsonViewer data={{}} />)
    const brackets = container.querySelectorAll('.json-viewer-bracket')
    const text = Array.from(brackets).map((b) => b.textContent).join('')
    expect(text).toBe('{}')
  })

  it('renders empty array as "[]"', () => {
    const { container } = render(<JsonViewer data={[]} />)
    const brackets = container.querySelectorAll('.json-viewer-bracket')
    const text = Array.from(brackets).map((b) => b.textContent).join('')
    expect(text).toBe('[]')
  })

  it('renders top-level object keys expanded by default', () => {
    render(<JsonViewer data={{ name: 'test' }} />)
    const key = document.querySelector('.json-viewer-key')!
    expect(key.textContent).toBe('"name"')
  })

  it('collapses nested objects by default (depth >= 1)', () => {
    render(<JsonViewer data={{ nested: { a: 1 } }} />)
    // Nested object should show summary "1 item" when collapsed
    expect(screen.getByText('1 item')).toBeDefined()
  })

  it('expands collapsed node on toggle button click', () => {
    render(<JsonViewer data={{ nested: { a: 1, b: 2 } }} />)
    // Initially collapsed, showing "2 items"
    expect(screen.getByText('2 items')).toBeDefined()

    // Click the expand toggle (▸ button)
    const toggles = document.querySelectorAll('.json-viewer-toggle')
    // The second toggle is the nested one
    fireEvent.click(toggles[toggles.length - 1])

    // Now nested keys should be visible
    expect(document.querySelector('.json-viewer-key[class]')).toBeDefined()
  })

  it('shows item count summary when collapsed', () => {
    render(<JsonViewer data={{ items: [1, 2, 3] }} />)
    expect(screen.getByText('3 items')).toBeDefined()
  })

  it('shows singular "1 item" when collection has one entry', () => {
    render(<JsonViewer data={{ single: { one: 1 } }} />)
    expect(screen.getByText('1 item')).toBeDefined()
  })

  it('renders array indices for array items when expanded', () => {
    render(<JsonViewer data={['a', 'b']} />)
    // Top-level array is expanded, indices shown
    const indices = document.querySelectorAll('.json-viewer-index')
    expect(indices[0].textContent).toBe('0')
    expect(indices[1].textContent).toBe('1')
  })

  it('wraps output in .json-viewer div', () => {
    const { container } = render(<JsonViewer data={null} />)
    expect(container.querySelector('.json-viewer')).toBeDefined()
  })
})
