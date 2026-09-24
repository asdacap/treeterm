import { describe, it, expect } from 'vitest'
import { isMarkdownPage, resolveAddress } from './url'

const PAGE = 'http://localhost:3000/app/page?x=1'

describe('resolveAddress', () => {
  it.each([
    ['/about', 'http://localhost:3000/about'],
    ['?q=2', 'http://localhost:3000/app/page?q=2'],
    ['#top', 'http://localhost:3000/app/page?x=1#top'],
    ['localhost:3000/x', 'http://localhost:3000/x'],
    ['http://localhost:3000/y?z=1', 'http://localhost:3000/y?z=1'],
    ['  /padded  ', 'http://localhost:3000/padded'],
    ['http://127.0.0.1:3000/ip', 'http://localhost:3000/ip'],
  ])('resolves %s', (input, expected) => {
    expect(resolveAddress(input, PAGE).href).toBe(expected)
  })

  it.each([
    ['localhost:4000/x', 'Only pages on http://localhost:3000'],
    ['https://localhost:3000/', 'Only pages on http://localhost:3000'],
    ['localhost/', 'Only pages on http://localhost:3000'],
    ['example.com', 'URL must be http(s)://localhost'],
    ['file:///etc/passwd', 'URL must be http(s)://localhost'],
    ['   ', 'Enter a URL or path'],
  ])('refuses %s', (input, message) => {
    expect(() => resolveAddress(input, PAGE)).toThrow(message)
  })
})

describe('isMarkdownPage', () => {
  it.each([
    ['text/markdown', 'http://localhost:3000/notes', true],
    ['text/x-markdown', 'http://localhost:3000/notes', true],
    ['text/plain', 'http://localhost:3000/README.md', true],
    ['text/plain', 'http://localhost:3000/docs/Guide.MARKDOWN?x=1', true],
    ['text/plain', 'http://localhost:3000/notes.txt', false],
    ['text/html', 'http://localhost:3000/README.md', false],
  ])('%s at %s is %s', (contentType, url, expected) => {
    expect(isMarkdownPage(contentType, url)).toBe(expected)
  })
})
