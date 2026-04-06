// @vitest-environment jsdom
import { describe, it, expect, vi } from 'vitest'
import { render, screen } from '@testing-library/react'


vi.mock('react-markdown', () => ({
  default: ({ children }: { children: string }) => <div data-testid="markdown">{children}</div>,
}))
vi.mock('remark-gfm', () => ({ default: () => {} }))

import { MarkdownPreview } from './MarkdownPreview'

describe('MarkdownPreview', () => {
  it('renders content text', () => {
    render(<MarkdownPreview content="Hello world" />)
    expect(screen.getByText('Hello world')).toBeDefined()
  })

  it('wraps output in .markdown-preview div', () => {
    const { container } = render(<MarkdownPreview content="test" />)
    expect(container.querySelector('.markdown-preview')).toBeDefined()
  })

  it('passes content to ReactMarkdown', () => {
    render(<MarkdownPreview content="# Heading" />)
    expect(screen.getByTestId('markdown').textContent).toBe('# Heading')
  })
})
