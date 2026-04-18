import { describe, it, expect, vi, beforeEach } from 'vitest'
import type { ExecApi, FilesystemApi } from '../types'
import type { ExecEvent } from '../../shared/ipc-types'
import {
  buildEntryFromWorkspace,
  createWorktreeRegistryApi,
  loadRegistry,
  removeRegistryEntry,
  upsertRegistryEntry,
} from './worktreeRegistry'

interface MockExecApi extends ExecApi {
  _complete: (execId: string, stdout: string, exitCode?: number) => void
}

function createMockExec(homedir = '/home/user'): MockExecApi {
  const callbacks = new Map<string, (event: ExecEvent) => void>()
  let counter = 0
  return {
    start: vi.fn().mockImplementation(() => {
      counter++
      const execId = `exec-${String(counter)}`
      setTimeout(() => {
        const cb = callbacks.get(execId)
        if (cb) {
          cb({ type: 'stdout', data: homedir })
          cb({ type: 'exit', exitCode: 0 })
        }
      })
      return Promise.resolve({ success: true, execId })
    }),
    kill: vi.fn(),
    onEvent: vi.fn().mockImplementation((execId: string, cb: (event: ExecEvent) => void) => {
      callbacks.set(execId, cb)
      return () => { callbacks.delete(execId) }
    }),
    _complete: (execId: string, stdout: string, exitCode = 0) => {
      const cb = callbacks.get(execId)
      if (cb) {
        cb({ type: 'stdout', data: stdout })
        cb({ type: 'exit', exitCode })
      }
    },
  }
}

function createMockFs(): FilesystemApi {
  return {
    readDirectory: vi.fn(),
    readFile: vi.fn(),
    writeFile: vi.fn(),
    searchFiles: vi.fn(),
  }
}

describe('worktreeRegistry', () => {
  let fs: FilesystemApi
  let exec: MockExecApi

  beforeEach(() => {
    fs = createMockFs()
    exec = createMockExec('/home/user')
  })

  describe('loadRegistry', () => {
    it('returns [] when file is missing', async () => {
      vi.mocked(fs.readFile).mockResolvedValue({ success: false, error: 'No such file or directory (os error 2)' })
      const entries = await loadRegistry(fs, exec, 'conn-1')
      expect(entries).toEqual([])
      expect(fs.readFile).toHaveBeenCalledWith('/home/user/.treeterm', 'worktrees.json')
    })

    it('parses valid registry file', async () => {
      const file = {
        version: 1,
        entries: [{ path: '/wt/a', branch: 'a', displayName: 'A', description: null, lastUsedAt: 100 }],
      }
      vi.mocked(fs.readFile).mockResolvedValue({
        success: true,
        file: { path: '/home/user/.treeterm/worktrees.json', content: JSON.stringify(file), size: 0, language: 'json' },
      })
      const entries = await loadRegistry(fs, exec, 'conn-1')
      expect(entries).toEqual(file.entries)
    })

    it('throws on malformed JSON', async () => {
      vi.mocked(fs.readFile).mockResolvedValue({
        success: true,
        file: { path: 'x', content: 'not json {', size: 0, language: 'json' },
      })
      await expect(loadRegistry(fs, exec, 'conn-1')).rejects.toThrow()
    })

    it('throws on unsupported version', async () => {
      vi.mocked(fs.readFile).mockResolvedValue({
        success: true,
        file: { path: 'x', content: JSON.stringify({ version: 2, entries: [] }), size: 0, language: 'json' },
      })
      await expect(loadRegistry(fs, exec, 'conn-1')).rejects.toThrow('Unsupported worktree registry version: 2')
    })

    it('throws on non-ENOENT read failure', async () => {
      vi.mocked(fs.readFile).mockResolvedValue({ success: false, error: 'permission denied' })
      await expect(loadRegistry(fs, exec, 'conn-1')).rejects.toThrow('Failed to read worktree registry: permission denied')
    })
  })

  describe('upsertRegistryEntry', () => {
    it('appends new entry when path not present', async () => {
      vi.mocked(fs.readFile).mockResolvedValue({ success: false, error: 'No such file' })
      vi.mocked(fs.writeFile).mockResolvedValue({ success: true })

      await upsertRegistryEntry(fs, exec, 'conn-1', {
        path: '/wt/a', branch: 'a', displayName: 'A', description: 'desc',
      })

      expect(fs.writeFile).toHaveBeenCalledOnce()
      const call = vi.mocked(fs.writeFile).mock.calls[0]!
      expect(call[0]).toBe('/home/user/.treeterm')
      expect(call[1]).toBe('worktrees.json')
      const written = JSON.parse(call[2]) as { version: number; entries: { path: string; lastUsedAt: number }[] }
      expect(written.version).toBe(1)
      expect(written.entries).toHaveLength(1)
      expect(written.entries[0]!.path).toBe('/wt/a')
      expect(typeof written.entries[0]!.lastUsedAt).toBe('number')
    })

    it('replaces existing entry by path and bumps lastUsedAt', async () => {
      const existing = {
        version: 1,
        entries: [{ path: '/wt/a', branch: 'a', displayName: null, description: null, lastUsedAt: 100 }],
      }
      vi.mocked(fs.readFile).mockResolvedValue({
        success: true,
        file: { path: 'x', content: JSON.stringify(existing), size: 0, language: 'json' },
      })
      vi.mocked(fs.writeFile).mockResolvedValue({ success: true })

      await upsertRegistryEntry(fs, exec, 'conn-1', {
        path: '/wt/a', branch: 'a', displayName: 'NewName', description: 'new',
      })

      const written = JSON.parse(vi.mocked(fs.writeFile).mock.calls[0]![2]) as { entries: { path: string; displayName: string | null; lastUsedAt: number }[] }
      expect(written.entries).toHaveLength(1)
      expect(written.entries[0]!.displayName).toBe('NewName')
      expect(written.entries[0]!.lastUsedAt).toBeGreaterThan(100)
    })

    it('throws when write fails', async () => {
      vi.mocked(fs.readFile).mockResolvedValue({ success: false, error: 'not found' })
      vi.mocked(fs.writeFile).mockResolvedValue({ success: false, error: 'disk full' })
      await expect(upsertRegistryEntry(fs, exec, 'conn-1', {
        path: '/wt/a', branch: 'a', displayName: null, description: null,
      })).rejects.toThrow('Failed to write worktree registry: disk full')
    })
  })

  describe('removeRegistryEntry', () => {
    it('filters out matching path and writes', async () => {
      const existing = {
        version: 1,
        entries: [
          { path: '/wt/a', branch: 'a', displayName: null, description: null, lastUsedAt: 100 },
          { path: '/wt/b', branch: 'b', displayName: null, description: null, lastUsedAt: 200 },
        ],
      }
      vi.mocked(fs.readFile).mockResolvedValue({
        success: true,
        file: { path: 'x', content: JSON.stringify(existing), size: 0, language: 'json' },
      })
      vi.mocked(fs.writeFile).mockResolvedValue({ success: true })

      await removeRegistryEntry(fs, exec, 'conn-1', '/wt/a')

      const written = JSON.parse(vi.mocked(fs.writeFile).mock.calls[0]![2]) as { entries: { path: string }[] }
      expect(written.entries).toEqual([existing.entries[1]])
    })

    it('no-ops when path is absent (no write)', async () => {
      vi.mocked(fs.readFile).mockResolvedValue({ success: false, error: 'No such file' })
      vi.mocked(fs.writeFile).mockResolvedValue({ success: true })

      await removeRegistryEntry(fs, exec, 'conn-1', '/wt/a')
      expect(fs.writeFile).not.toHaveBeenCalled()
    })
  })

  describe('buildEntryFromWorkspace', () => {
    it('pulls metadata.displayName and metadata.description', () => {
      const entry = buildEntryFromWorkspace({
        id: 'ws-1',
        path: '/wt/x',
        name: 'x',
        parentId: null,
        status: 'active',
        isGitRepo: true,
        gitBranch: 'feat/x',
        gitRootPath: '/repo',
        isWorktree: true,
        isDetached: false,
        appStates: {},
        activeTabId: null,
        settings: { defaultApplicationId: '' },
        metadata: { displayName: 'X custom', description: 'a desc' },
        createdAt: 0,
        lastActivity: 0,
      })
      expect(entry).toEqual({
        path: '/wt/x',
        branch: 'feat/x',
        displayName: 'X custom',
        description: 'a desc',
      })
    })

    it('returns null for missing metadata fields', () => {
      const entry = buildEntryFromWorkspace({
        id: 'ws-2',
        path: '/wt/y',
        name: 'y',
        parentId: null,
        status: 'active',
        isGitRepo: true,
        gitBranch: null,
        gitRootPath: null,
        isWorktree: true,
        isDetached: false,
        appStates: {},
        activeTabId: null,
        settings: { defaultApplicationId: '' },
        metadata: {},
        createdAt: 0,
        lastActivity: 0,
      })
      expect(entry).toEqual({
        path: '/wt/y',
        branch: '',
        displayName: null,
        description: null,
      })
    })
  })

  describe('createWorktreeRegistryApi', () => {
    it('exposes list/upsert/remove bound to connection', async () => {
      vi.mocked(fs.readFile).mockResolvedValue({ success: false, error: 'No such file' })
      vi.mocked(fs.writeFile).mockResolvedValue({ success: true })

      const api = createWorktreeRegistryApi(fs, exec, 'conn-1')
      expect(await api.list()).toEqual([])
      await api.upsert({ path: '/wt/a', branch: 'a', displayName: null, description: null })
      expect(fs.writeFile).toHaveBeenCalled()
    })
  })
})
