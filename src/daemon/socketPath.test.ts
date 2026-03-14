import { describe, it, expect } from 'vitest'
import { getDefaultSocketPath } from './socketPath'
import * as os from 'os'
import * as path from 'path'

describe('getDefaultSocketPath', () => {
  it('returns a path ending in daemon.sock', () => {
    const socketPath = getDefaultSocketPath()
    expect(socketPath).toMatch(/daemon\.sock$/)
  })

  it('returns a path within the OS temp directory', () => {
    const socketPath = getDefaultSocketPath()
    expect(socketPath.startsWith(os.tmpdir())).toBe(true)
  })

  it('includes treeterm in the path', () => {
    const socketPath = getDefaultSocketPath()
    expect(socketPath).toContain('treeterm-')
  })

  it('returns a valid path structure', () => {
    const socketPath = getDefaultSocketPath()
    const parsed = path.parse(socketPath)
    expect(parsed.base).toBe('daemon.sock')
  })
})
