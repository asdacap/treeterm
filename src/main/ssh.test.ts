import { describe, it, expect, vi, beforeEach } from 'vitest'
import { EventEmitter } from 'events'
import type { ChildProcess } from 'child_process'

// Mock child_process
vi.mock('child_process', () => ({
  spawn: vi.fn(),
}))

// Mock fs
vi.mock('fs', () => ({
  existsSync: vi.fn(),
  mkdirSync: vi.fn(),
  unlinkSync: vi.fn(),
  readFileSync: vi.fn().mockReturnValue(Buffer.from('mock-binary')),
}))

// Mock socketPath
vi.mock('./socketPath', () => ({
  getRemoteForwardSocketPath: vi.fn().mockReturnValue('/tmp/treeterm-remote/daemon.sock'),
}))

import { spawn } from 'child_process'
import * as fs from 'fs'
import { SSHTunnel } from './ssh'
import type { SSHConnectionConfig } from '../shared/types'

type MockProcess = EventEmitter & {
  stdout: EventEmitter
  stderr: EventEmitter
  stdin: { write: ReturnType<typeof vi.fn> }
  kill: ReturnType<typeof vi.fn>
}

/** Type exposing private members of SSHTunnel for test access */
type SSHTunnelPrivate = {
  sshProcess: ChildProcess | null
  _connected: boolean
  appendBootstrapOutput: (line: string) => void
  appendTunnelOutput: (line: string) => void
  disconnectListeners: Set<(error?: string) => void>
  buildBaseSSHArgs: () => string[]
  bootstrapRemoteDaemon: () => Promise<string>
  startTunnel: (remoteSocketPath: string) => void
  waitForSocket: () => Promise<void>
  getLocalDaemonChecksum: (arch: string) => string
  getDaemonBinaryPath: (arch: string) => string
}

/** Get typed access to private members of SSHTunnel */
function priv(tunnel: SSHTunnel): SSHTunnelPrivate {
  return tunnel as unknown as SSHTunnelPrivate
}

function makeConfig(overrides?: Partial<SSHConnectionConfig>): SSHConnectionConfig {
  return {
    id: 'conn-1',
    host: 'example.com',
    port: 22,
    user: 'testuser',
    portForwards: [],
    ...overrides,
  }
}

function makeMockProcess(): MockProcess {
  const proc = new EventEmitter() as MockProcess
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
      priv(tunnel).sshProcess = mockProc as unknown as ChildProcess
      priv(tunnel)._connected = true

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
      priv(tunnel)._connected = true
      vi.mocked(fs.existsSync).mockReturnValue(true)
      vi.mocked(fs.unlinkSync).mockImplementation(() => { throw new Error('ENOENT') })

      tunnel.disconnect()
    })
  })

  describe('bootstrap output management', () => {
    it('getBootstrapOutput returns copy of buffer', () => {
      const tunnel = new SSHTunnel(makeConfig())
      priv(tunnel).appendBootstrapOutput('line 1')
      priv(tunnel).appendBootstrapOutput('line 2')

      const output = tunnel.getBootstrapOutput()
      expect(output).toEqual(['line 1', 'line 2'])
      output.push('line 3')
      expect(tunnel.getBootstrapOutput()).toHaveLength(2)
    })

    it('onBootstrapOutput notifies listeners', () => {
      const tunnel = new SSHTunnel(makeConfig())
      const cb = vi.fn()
      tunnel.onBootstrapOutput(cb)

      priv(tunnel).appendBootstrapOutput('test line')
      expect(cb).toHaveBeenCalledWith('test line')
    })

    it('onBootstrapOutput unsubscribe works', () => {
      const tunnel = new SSHTunnel(makeConfig())
      const cb = vi.fn()
      const unsub = tunnel.onBootstrapOutput(cb)

      unsub()
      priv(tunnel).appendBootstrapOutput('test')
      expect(cb).not.toHaveBeenCalled()
    })

    it('bootstrap output buffer is bounded', () => {
      const tunnel = new SSHTunnel(makeConfig())
      for (let i = 0; i < 1100; i++) {
        priv(tunnel).appendBootstrapOutput(`line ${String(i)}`)
      }
      const output = tunnel.getBootstrapOutput()
      expect(output.length).toBeLessThanOrEqual(1000)
    })
  })

  describe('tunnel output management', () => {
    it('getTunnelOutput returns copy of buffer', () => {
      const tunnel = new SSHTunnel(makeConfig())
      priv(tunnel).appendTunnelOutput('line 1')
      priv(tunnel).appendTunnelOutput('line 2')

      const output = tunnel.getTunnelOutput()
      expect(output).toEqual(['line 1', 'line 2'])
      output.push('line 3')
      expect(tunnel.getTunnelOutput()).toHaveLength(2)
    })

    it('onTunnelOutput notifies listeners', () => {
      const tunnel = new SSHTunnel(makeConfig())
      const cb = vi.fn()
      tunnel.onTunnelOutput(cb)

      priv(tunnel).appendTunnelOutput('test line')
      expect(cb).toHaveBeenCalledWith('test line')
    })
  })

  describe('onDisconnect', () => {
    it('registers and unregisters listeners', () => {
      const tunnel = new SSHTunnel(makeConfig())
      const cb = vi.fn()
      const unsub = tunnel.onDisconnect(cb)

      priv(tunnel).disconnectListeners.forEach((c) => { c('test error') })
      expect(cb).toHaveBeenCalledWith('test error')

      unsub()
      cb.mockClear()
      priv(tunnel).disconnectListeners.forEach((c) => { c('test') })
      expect(cb).not.toHaveBeenCalled()
    })
  })

  describe('buildBaseSSHArgs', () => {
    it('builds args with standard options', () => {
      const tunnel = new SSHTunnel(makeConfig())
      const args = priv(tunnel).buildBaseSSHArgs()

      expect(args).toContain('StrictHostKeyChecking=accept-new')
      expect(args).toContain('BatchMode=yes')
      expect(args).toContain('22')
      expect(args).toContain('testuser@example.com')
    })

    it('includes identity file when configured', () => {
      const tunnel = new SSHTunnel(makeConfig({ identityFile: '/home/user/.ssh/id_rsa' }))
      const args = priv(tunnel).buildBaseSSHArgs()

      expect(args).toContain('-i')
      expect(args).toContain('/home/user/.ssh/id_rsa')
    })

    it('uses custom port', () => {
      const tunnel = new SSHTunnel(makeConfig({ port: 2222 }))
      const args = priv(tunnel).buildBaseSSHArgs()
      expect(args).toContain('2222')
    })
  })

  describe('bootstrapRemoteDaemon', () => {
    it('resolves with socket path on success', async () => {
      const proc = makeMockProcess()
      vi.mocked(spawn).mockReturnValue(proc as unknown as ChildProcess)

      const tunnel = new SSHTunnel(makeConfig())
      const promise = priv(tunnel).bootstrapRemoteDaemon()

      proc.stdout.emit('data', Buffer.from('TREETERM_ARCH:x86_64\nTREETERM_SOCKET:/tmp/treeterm-1000/daemon.sock\n'))
      proc.emit('close', 0)

      const result = await promise
      expect(result).toBe('/tmp/treeterm-1000/daemon.sock')
    })

    it('rejects on non-zero exit code', async () => {
      const proc = makeMockProcess()
      vi.mocked(spawn).mockReturnValue(proc as unknown as ChildProcess)

      const tunnel = new SSHTunnel(makeConfig())
      const promise = priv(tunnel).bootstrapRemoteDaemon()

      proc.stderr.emit('data', Buffer.from('Permission denied\n'))
      proc.emit('close', 1)

      await expect(promise).rejects.toThrow('SSH bootstrap failed')
    })

    it('rejects on spawn error', async () => {
      const proc = makeMockProcess()
      vi.mocked(spawn).mockReturnValue(proc as unknown as ChildProcess)

      const tunnel = new SSHTunnel(makeConfig())
      const promise = priv(tunnel).bootstrapRemoteDaemon()

      proc.emit('error', new Error('spawn ENOENT'))

      await expect(promise).rejects.toThrow('Failed to spawn ssh')
    })

    it('rejects when TREETERM_ARCH marker is missing', async () => {
      const proc = makeMockProcess()
      vi.mocked(spawn).mockReturnValue(proc as unknown as ChildProcess)

      const tunnel = new SSHTunnel(makeConfig())
      const promise = priv(tunnel).bootstrapRemoteDaemon()

      proc.stdout.emit('data', Buffer.from('uid=1001\n'))
      proc.emit('close', 0)

      await expect(promise).rejects.toThrow('Could not detect remote architecture')
    })

    it('includes REFRESH_DAEMON=1 when refreshDaemon option set', async () => {
      const proc = makeMockProcess()
      vi.mocked(spawn).mockReturnValue(proc as unknown as ChildProcess)

      const tunnel = new SSHTunnel(makeConfig(), { refreshDaemon: true })
      const promise = priv(tunnel).bootstrapRemoteDaemon()

      // Check the bootstrap script contains REFRESH_DAEMON=1
      const spawnCall = vi.mocked(spawn).mock.calls[0]!
      const scriptArg = spawnCall[1][spawnCall[1].length - 1]
      expect(scriptArg).toContain('REFRESH_DAEMON=1')

      proc.stdout.emit('data', Buffer.from('TREETERM_ARCH:x86_64\nTREETERM_SOCKET:/tmp/daemon.sock\n'))
      proc.emit('close', 0)

      await promise
    })

    it('bootstrap script includes sha256sum checksum reporting', () => {
      const proc = makeMockProcess()
      vi.mocked(spawn).mockReturnValue(proc as unknown as ChildProcess)

      const tunnel = new SSHTunnel(makeConfig())
      void priv(tunnel).bootstrapRemoteDaemon()

      const spawnCall = vi.mocked(spawn).mock.calls[0]!
      const scriptArg = spawnCall[1][spawnCall[1].length - 1]
      expect(scriptArg).toContain('sha256sum')
      expect(scriptArg).toContain('TREETERM_REMOTE_HASH')
      expect(scriptArg).toContain('TREETERM_HOME:$HOME')

      proc.stdout.emit('data', Buffer.from('TREETERM_ARCH:x86_64\nTREETERM_SOCKET:/tmp/daemon.sock\n'))
      proc.emit('close', 0)
    })

    it('resolves immediately when hash matches', async () => {
      const proc = makeMockProcess()
      vi.mocked(spawn).mockReturnValue(proc as unknown as ChildProcess)

      const tunnel = new SSHTunnel(makeConfig())
      vi.spyOn(priv(tunnel), 'getLocalDaemonChecksum').mockReturnValue('aabbccdd11223344')

      const promise = priv(tunnel).bootstrapRemoteDaemon()

      proc.stdout.emit(
        'data',
        Buffer.from(
          'TREETERM_ARCH:x86_64\nTREETERM_REMOTE_HASH:aabbccdd11223344\nTREETERM_SOCKET:/tmp/treeterm-1000/daemon.sock\n',
        ),
      )
      proc.emit('close', 0)

      const result = await promise
      expect(result).toBe('/tmp/treeterm-1000/daemon.sock')
      // Only 1 spawn call (the bootstrap itself) — no kill or upload
      expect(spawn).toHaveBeenCalledTimes(1)
    })

    it('kills and re-uploads when hash mismatches with refreshDaemon', async () => {
      const bootstrapProc = makeMockProcess()
      const killProc = makeMockProcess()
      const scpProc = makeMockProcess()
      const startProc = makeMockProcess()
      vi.mocked(spawn)
        .mockReturnValueOnce(bootstrapProc as unknown as ChildProcess)
        .mockReturnValueOnce(killProc as unknown as ChildProcess)
        .mockReturnValueOnce(scpProc as unknown as ChildProcess)
        .mockReturnValueOnce(startProc as unknown as ChildProcess)

      vi.mocked(fs.existsSync).mockReturnValue(false)

      const tunnel = new SSHTunnel(makeConfig(), { refreshDaemon: true })
      vi.spyOn(priv(tunnel), 'getLocalDaemonChecksum').mockReturnValue('localhash000')
      vi.spyOn(priv(tunnel), 'getDaemonBinaryPath').mockReturnValue('/mock/path/treeterm-daemon-x86_64-linux')

      const promise = priv(tunnel).bootstrapRemoteDaemon()

      // Bootstrap reports mismatched hash
      bootstrapProc.stdout.emit(
        'data',
        Buffer.from(
          'TREETERM_ARCH:x86_64\nTREETERM_HOME:/home/testuser\nTREETERM_REMOTE_HASH:remotehash999\nTREETERM_SOCKET:/tmp/treeterm-1000/daemon.sock\n',
        ),
      )
      bootstrapProc.emit('close', 0)

      // Allow microtask to process
      await new Promise(r => setTimeout(r, 0))

      // Kill old daemon completes
      killProc.emit('close', 0)
      await new Promise(r => setTimeout(r, 0))

      // SCP upload completes
      scpProc.emit('close', 0)
      await new Promise(r => setTimeout(r, 0))

      // Start new daemon reports socket
      startProc.stdout.emit('data', Buffer.from('TREETERM_SOCKET:/tmp/treeterm-1000/daemon.sock\n'))
      startProc.emit('close', 0)

      const result = await promise
      expect(result).toBe('/tmp/treeterm-1000/daemon.sock')
      expect(spawn).toHaveBeenCalledTimes(4)

      // scp destination must be an absolute path (no `~/`), to survive SFTP-mode scp
      const scpCall = vi.mocked(spawn).mock.calls[2]!
      const scpDest = scpCall[1][scpCall[1].length - 1]
      expect(scpDest).toBe('testuser@example.com:/home/testuser/.treeterm/treeterm-daemon')
      expect(scpDest).not.toContain('~')
    })

    it('rejects when upload needed but TREETERM_HOME marker is missing', async () => {
      const proc = makeMockProcess()
      vi.mocked(spawn).mockReturnValue(proc as unknown as ChildProcess)

      const tunnel = new SSHTunnel(makeConfig())

      const promise = priv(tunnel).bootstrapRemoteDaemon()

      // Bootstrap reports NEEDS_UPLOAD but omits TREETERM_HOME
      proc.stdout.emit(
        'data',
        Buffer.from(
          'TREETERM_ARCH:x86_64\nTREETERM_REMOTE_HASH:NONE\nTREETERM_SOCKET:NEEDS_UPLOAD\n',
        ),
      )
      proc.emit('close', 0)

      await expect(promise).rejects.toThrow('Could not detect remote home directory')
    })

    it('rejects with error on hash mismatch without refreshDaemon', async () => {
      const proc = makeMockProcess()
      vi.mocked(spawn).mockReturnValue(proc as unknown as ChildProcess)

      const tunnel = new SSHTunnel(makeConfig())
      vi.spyOn(priv(tunnel), 'getLocalDaemonChecksum').mockReturnValue('localhash000')

      const promise = priv(tunnel).bootstrapRemoteDaemon()

      proc.stdout.emit(
        'data',
        Buffer.from(
          'TREETERM_ARCH:x86_64\nTREETERM_REMOTE_HASH:remotehash999\nTREETERM_SOCKET:/tmp/treeterm-1000/daemon.sock\n',
        ),
      )
      proc.emit('close', 0)

      await expect(promise).rejects.toThrow('Daemon binary hash mismatch')
      // Only bootstrap spawn — no kill or upload
      expect(spawn).toHaveBeenCalledTimes(1)
    })

    it('resolves with existing socket when allowOutdatedDaemon and hash mismatches', async () => {
      const proc = makeMockProcess()
      vi.mocked(spawn).mockReturnValue(proc as unknown as ChildProcess)

      const tunnel = new SSHTunnel(makeConfig(), { allowOutdatedDaemon: true })
      vi.spyOn(priv(tunnel), 'getLocalDaemonChecksum').mockReturnValue('localhash000')

      const promise = priv(tunnel).bootstrapRemoteDaemon()

      proc.stdout.emit(
        'data',
        Buffer.from(
          'TREETERM_ARCH:x86_64\nTREETERM_REMOTE_HASH:remotehash999\nTREETERM_SOCKET:/tmp/treeterm-1000/daemon.sock\n',
        ),
      )
      proc.emit('close', 0)

      const result = await promise
      expect(result).toBe('/tmp/treeterm-1000/daemon.sock')
      // Only bootstrap spawn — no kill or upload
      expect(spawn).toHaveBeenCalledTimes(1)
    })

    it('trusts running daemon when sha256sum is unavailable (NONE hash)', async () => {
      const proc = makeMockProcess()
      vi.mocked(spawn).mockReturnValue(proc as unknown as ChildProcess)

      const tunnel = new SSHTunnel(makeConfig())
      const promise = priv(tunnel).bootstrapRemoteDaemon()

      proc.stdout.emit(
        'data',
        Buffer.from(
          'TREETERM_ARCH:x86_64\nTREETERM_REMOTE_HASH:NONE\nTREETERM_SOCKET:/tmp/treeterm-1000/daemon.sock\n',
        ),
      )
      proc.emit('close', 0)

      const result = await promise
      expect(result).toBe('/tmp/treeterm-1000/daemon.sock')
      // Only 1 spawn call — no upload triggered
      expect(spawn).toHaveBeenCalledTimes(1)
    })
  })

  describe('startTunnel', () => {
    it('spawns ssh with socket forwarding args', () => {
      const proc = makeMockProcess()
      vi.mocked(spawn).mockReturnValue(proc as unknown as ChildProcess)

      const tunnel = new SSHTunnel(makeConfig())
      priv(tunnel).startTunnel('/tmp/remote.sock')

      const call = vi.mocked(spawn).mock.calls[0]!
      expect(call[0]).toBe('ssh')
      expect(call[1]).toContain('-L')
      // Should include the local:remote socket mapping
      expect(call[1].some((arg: string) => arg.includes('/tmp/remote.sock'))).toBe(true)
    })

    it('notifies disconnect listeners on process close when connected', () => {
      const proc = makeMockProcess()
      vi.mocked(spawn).mockReturnValue(proc as unknown as ChildProcess)

      const tunnel = new SSHTunnel(makeConfig())
      priv(tunnel)._connected = true
      priv(tunnel).startTunnel('/tmp/remote.sock')

      const cb = vi.fn()
      tunnel.onDisconnect(cb)

      proc.emit('close', 1)

      expect(cb).toHaveBeenCalled()
      expect(tunnel.connected).toBe(false)
    })

    it('notifies disconnect listeners on process error', () => {
      const proc = makeMockProcess()
      vi.mocked(spawn).mockReturnValue(proc as unknown as ChildProcess)

      const tunnel = new SSHTunnel(makeConfig())
      priv(tunnel).startTunnel('/tmp/remote.sock')

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
      priv(tunnel).sshProcess = {} as ChildProcess // not null

      await priv(tunnel).waitForSocket()
      // Should not throw
    })

    it('rejects when ssh process dies before socket appears', async () => {
      vi.mocked(fs.existsSync).mockReturnValue(false)

      const tunnel = new SSHTunnel(makeConfig())
      priv(tunnel).sshProcess = null

      await expect(priv(tunnel).waitForSocket()).rejects.toThrow('SSH process exited')
    })
  })
})
