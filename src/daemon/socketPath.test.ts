import { describe, it, expect } from 'vitest'
import { getDefaultSocketPath, getRemoteForwardSocketPath } from './socketPath'
import * as os from 'os'
import * as path from 'path'

describe('getRemoteForwardSocketPath', () => {
  it('returns a path ending in .sock under /tmp', () => {
    const socketPath = getRemoteForwardSocketPath('my-connection')
    expect(socketPath).toMatch(/\.sock$/)
    expect(socketPath.startsWith('/tmp/')).toBe(true)
  })

  it('includes treeterm and r- prefix in the path', () => {
    const socketPath = getRemoteForwardSocketPath('my-connection')
    expect(socketPath).toContain('treeterm-')
    expect(path.parse(socketPath).base).toMatch(/^r-[a-f0-9]{12}\.sock$/)
  })

  it('returns different paths for different connection IDs', () => {
    const path1 = getRemoteForwardSocketPath('conn-a')
    const path2 = getRemoteForwardSocketPath('conn-b')
    expect(path1).not.toBe(path2)
  })

  it('returns the same path for the same connection ID', () => {
    const path1 = getRemoteForwardSocketPath('same-id')
    const path2 = getRemoteForwardSocketPath('same-id')
    expect(path1).toBe(path2)
  })
})

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
