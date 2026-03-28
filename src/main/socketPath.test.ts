import { describe, it, expect, vi, beforeEach } from 'vitest'
import * as os from 'os'
import * as path from 'path'
import { getDefaultSocketPath, getRemoteForwardSocketPath } from './socketPath'

describe('socketPath', () => {
  describe('getDefaultSocketPath', () => {
    it('returns path containing daemon.sock', () => {
      const result = getDefaultSocketPath()
      expect(result).toContain('daemon.sock')
    })

    it('returns path under tmpdir with treeterm prefix', () => {
      const result = getDefaultSocketPath()
      const uid = process.getuid ? process.getuid() : os.userInfo().uid
      expect(result).toBe(path.join(os.tmpdir(), `treeterm-${uid}`, 'daemon.sock'))
    })
  })

  describe('getRemoteForwardSocketPath', () => {
    it('returns path with r- prefix and hash', () => {
      const result = getRemoteForwardSocketPath('test-connection')
      expect(result).toMatch(/\/r-[a-f0-9]{12}\.sock$/)
    })

    it('returns path under /tmp with treeterm prefix', () => {
      const uid = process.getuid ? process.getuid() : os.userInfo().uid
      const result = getRemoteForwardSocketPath('test-connection')
      expect(result).toContain(`/tmp/treeterm-${uid}/`)
    })

    it('produces different paths for different connection IDs', () => {
      const path1 = getRemoteForwardSocketPath('connection-1')
      const path2 = getRemoteForwardSocketPath('connection-2')
      expect(path1).not.toBe(path2)
    })

    it('produces consistent paths for the same connection ID', () => {
      const path1 = getRemoteForwardSocketPath('my-connection')
      const path2 = getRemoteForwardSocketPath('my-connection')
      expect(path1).toBe(path2)
    })
  })
})
