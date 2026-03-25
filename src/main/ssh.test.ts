import { describe, it, expect, vi, beforeEach } from 'vitest'
import { EventEmitter } from 'events'

// Mock child_process
vi.mock('child_process', () => ({
  spawn: vi.fn(),
}))

// Mock fs
vi.mock('fs', () => ({
  existsSync: vi.fn(),
  mkdirSync: vi.fn(),
  unlinkSync: vi.fn(),
}))

// Mock socketPath
vi.mock('./socketPath', () => ({
  getRemoteForwardSocketPath: vi.fn().mockReturnValue('/tmp/treeterm-remote/daemon.sock'),
}))

import { spawn } from 'child_process'
import * as fs from 'fs'
import { SSHTunnel } from './ssh'
import type { SSHConnectionConfig } from '../shared/types'

function makeConfig(overrides?: Partial<SSHConnectionConfig>): SSHConnectionConfig {
  return {
    id: 'conn-1',
    host: 'example.com',
    port: 22,
    user: 'testuser',
    ...overrides,
  }
}

function makeMockProcess() {
  const proc = new EventEmitter() as any
  proc.stdout = new EventEmitter()
  proc.stderr = new EventEmitter()
  proc.stdin = { write: vi.fn() }
  proc.kill = vi.fn()
  return proc
}

describe('SSHTunnel', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.mocked(fs.existsSync).mockReturnValue(false)
  })

  describe('constructor and properties', () => {
    it('starts disconnected', () => {
      const tunnel = new SSHTunnel(makeConfig())
      expect(tunnel.connected).toBe(false)
    })
  })

  describe('disconnect', () => {
    it('kills ssh process and cleans up socket', () => {
      const tunnel = new SSHTunnel(makeConfig())
      const mockProc = makeMockProcess()
      ;(tunnel as any).sshProcess = mockProc
      ;(tunnel as any)._connected = true

      vi.mocked(fs.existsSync).mockReturnValue(true)

      tunnel.disconnect()

      expect(tunnel.connected).toBe(false)
      expect(mockProc.kill).toHaveBeenCalled()
      expect(fs.unlinkSync).toHaveBeenCalled()
    })

    it('handles disconnect when not connected', () => {
      const tunnel = new SSHTunnel(makeConfig())
      tunnel.disconnect()
      expect(tunnel.connected).toBe(false)
    })

    it('ignores cleanup errors', () => {
      const tunnel = new SSHTunnel(makeConfig())
      ;(tunnel as any)._connected = true
      vi.mocked(fs.existsSync).mockReturnValue(true)
      vi.mocked(fs.unlinkSync).mockImplementation(() => { throw new Error('ENOENT') })

      tunnel.disconnect()
    })
  })

  describe('output management', () => {
    it('getOutput returns copy of buffer', () => {
      const tunnel = new SSHTunnel(makeConfig())
      ;(tunnel as any).appendOutput('line 1')
      ;(tunnel as any).appendOutput('line 2')

      const output = tunnel.getOutput()
      expect(output).toEqual(['line 1', 'line 2'])
      output.push('line 3')
      expect(tunnel.getOutput()).toHaveLength(2)
    })

    it('onOutput notifies listeners', () => {
      const tunnel = new SSHTunnel(makeConfig())
      const cb = vi.fn()
      tunnel.onOutput(cb)

      ;(tunnel as any).appendOutput('test line')
      expect(cb).toHaveBeenCalledWith('test line')
    })

    it('onOutput unsubscribe works', () => {
      const tunnel = new SSHTunnel(makeConfig())
      const cb = vi.fn()
      const unsub = tunnel.onOutput(cb)

      unsub()
      ;(tunnel as any).appendOutput('test')
      expect(cb).not.toHaveBeenCalled()
    })

    it('output buffer is bounded', () => {
      const tunnel = new SSHTunnel(makeConfig())
      for (let i = 0; i < 1100; i++) {
        ;(tunnel as any).appendOutput(`line ${i}`)
      }
      const output = tunnel.getOutput()
      expect(output.length).toBeLessThanOrEqual(1000)
    })
  })

  describe('onDisconnect', () => {
    it('registers and unregisters listeners', () => {
      const tunnel = new SSHTunnel(makeConfig())
      const cb = vi.fn()
      const unsub = tunnel.onDisconnect(cb)

      ;(tunnel as any).disconnectListeners.forEach((c: any) => c('test error'))
      expect(cb).toHaveBeenCalledWith('test error')

      unsub()
      cb.mockClear()
      ;(tunnel as any).disconnectListeners.forEach((c: any) => c('test'))
      expect(cb).not.toHaveBeenCalled()
    })
  })

  describe('buildBaseSSHArgs', () => {
    it('builds args with standard options', () => {
      const tunnel = new SSHTunnel(makeConfig())
      const args = (tunnel as any).buildBaseSSHArgs()

      expect(args).toContain('StrictHostKeyChecking=accept-new')
      expect(args).toContain('BatchMode=yes')
      expect(args).toContain('22')
      expect(args).toContain('testuser@example.com')
    })

    it('includes identity file when configured', () => {
      const tunnel = new SSHTunnel(makeConfig({ identityFile: '/home/user/.ssh/id_rsa' }))
      const args = (tunnel as any).buildBaseSSHArgs()

      expect(args).toContain('-i')
      expect(args).toContain('/home/user/.ssh/id_rsa')
    })

    it('uses custom port', () => {
      const tunnel = new SSHTunnel(makeConfig({ port: 2222 }))
      const args = (tunnel as any).buildBaseSSHArgs()
      expect(args).toContain('2222')
    })
  })

  describe('bootstrapRemoteDaemon', () => {
    it('resolves with socket path on success', async () => {
      const proc = makeMockProcess()
      vi.mocked(spawn).mockReturnValue(proc as any)

      const tunnel = new SSHTunnel(makeConfig())
      const promise = (tunnel as any).bootstrapRemoteDaemon()

      proc.stdout.emit('data', Buffer.from('Starting daemon...\nTREETERM_SOCKET:/tmp/treeterm-1000/daemon.sock\n'))
      proc.emit('close', 0)

      const result = await promise
      expect(result).toBe('/tmp/treeterm-1000/daemon.sock')
    })

    it('rejects on non-zero exit code', async () => {
      const proc = makeMockProcess()
      vi.mocked(spawn).mockReturnValue(proc as any)

      const tunnel = new SSHTunnel(makeConfig())
      const promise = (tunnel as any).bootstrapRemoteDaemon()

      proc.stderr.emit('data', Buffer.from('Permission denied\n'))
      proc.emit('close', 1)

      await expect(promise).rejects.toThrow('SSH bootstrap failed')
    })

    it('rejects on spawn error', async () => {
      const proc = makeMockProcess()
      vi.mocked(spawn).mockReturnValue(proc as any)

      const tunnel = new SSHTunnel(makeConfig())
      const promise = (tunnel as any).bootstrapRemoteDaemon()

      proc.emit('error', new Error('spawn ENOENT'))

      await expect(promise).rejects.toThrow('Failed to spawn ssh')
    })

    it('falls back to default socket path without TREETERM_SOCKET marker', async () => {
      const proc = makeMockProcess()
      vi.mocked(spawn).mockReturnValue(proc as any)

      const tunnel = new SSHTunnel(makeConfig())
      const promise = (tunnel as any).bootstrapRemoteDaemon()

      proc.stdout.emit('data', Buffer.from('uid=1001\n'))
      proc.emit('close', 0)

      const result = await promise
      expect(result).toBe('/tmp/treeterm-1001/daemon.sock')
    })

    it('includes REFRESH_DAEMON=1 when refreshDaemon option set', async () => {
      const proc = makeMockProcess()
      vi.mocked(spawn).mockReturnValue(proc as any)

      const tunnel = new SSHTunnel(makeConfig(), { refreshDaemon: true })
      const promise = (tunnel as any).bootstrapRemoteDaemon()

      // Check the bootstrap script contains REFRESH_DAEMON=1
      const spawnCall = vi.mocked(spawn).mock.calls[0]
      const scriptArg = spawnCall[1][spawnCall[1].length - 1] as string
      expect(scriptArg).toContain('REFRESH_DAEMON=1')

      proc.stdout.emit('data', Buffer.from('TREETERM_SOCKET:/tmp/daemon.sock\n'))
      proc.emit('close', 0)

      await promise
    })
  })

  describe('startTunnel', () => {
    it('spawns ssh with socket forwarding args', () => {
      const proc = makeMockProcess()
      vi.mocked(spawn).mockReturnValue(proc as any)

      const tunnel = new SSHTunnel(makeConfig())
      ;(tunnel as any).startTunnel('/tmp/remote.sock')

      const call = vi.mocked(spawn).mock.calls[0]
      expect(call[0]).toBe('ssh')
      expect(call[1]).toContain('-L')
      // Should include the local:remote socket mapping
      expect(call[1].some((arg: string) => arg.includes('/tmp/remote.sock'))).toBe(true)
    })

    it('notifies disconnect listeners on process close when connected', () => {
      const proc = makeMockProcess()
      vi.mocked(spawn).mockReturnValue(proc as any)

      const tunnel = new SSHTunnel(makeConfig())
      ;(tunnel as any)._connected = true
      ;(tunnel as any).startTunnel('/tmp/remote.sock')

      const cb = vi.fn()
      tunnel.onDisconnect(cb)

      proc.emit('close', 1)

      expect(cb).toHaveBeenCalled()
      expect(tunnel.connected).toBe(false)
    })

    it('notifies disconnect listeners on process error', () => {
      const proc = makeMockProcess()
      vi.mocked(spawn).mockReturnValue(proc as any)

      const tunnel = new SSHTunnel(makeConfig())
      ;(tunnel as any).startTunnel('/tmp/remote.sock')

      const cb = vi.fn()
      tunnel.onDisconnect(cb)

      proc.emit('error', new Error('connection refused'))

      expect(cb).toHaveBeenCalled()
      expect(tunnel.connected).toBe(false)
    })
  })

  describe('waitForSocket', () => {
    it('resolves immediately when socket exists', async () => {
      vi.mocked(fs.existsSync).mockReturnValue(true)

      const tunnel = new SSHTunnel(makeConfig())
      ;(tunnel as any).sshProcess = {} // not null

      await (tunnel as any).waitForSocket()
      // Should not throw
    })

    it('rejects when ssh process dies before socket appears', async () => {
      vi.mocked(fs.existsSync).mockReturnValue(false)

      const tunnel = new SSHTunnel(makeConfig())
      ;(tunnel as any).sshProcess = null

      await expect((tunnel as any).waitForSocket()).rejects.toThrow('SSH process exited')
    })
  })
})
