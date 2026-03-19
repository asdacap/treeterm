import { describe, it, expect, vi, beforeEach } from 'vitest'
import type { Workspace } from './protocol'

vi.mock('./logger', () => ({
  createModuleLogger: () => ({
    info: vi.fn(),
    warn: vi.fn(),
    error: vi.fn(),
    debug: vi.fn(),
  }),
}))

import { SessionStore } from './sessionStore'

type WorkspaceInput = Omit<Workspace, 'createdAt' | 'lastActivity'>

function makeWorkspace(overrides: Partial<WorkspaceInput> & { path: string }): WorkspaceInput {
  const defaults: WorkspaceInput = {
    id: 'ws-test',
    path: overrides.path,
    name: 'test workspace',
    parentId: null,
    children: [],
    status: 'active',
    isGitRepo: false,
    gitBranch: null,
    gitRootPath: null,
    isWorktree: false,
    appStates: {},
    activeTabId: null,
    metadata: {},
  }
  return { ...defaults, ...overrides }
}

describe('SessionStore', () => {
  let store: SessionStore

  beforeEach(() => {
    store = new SessionStore()
  })

  describe('createSession', () => {
    it('creates a session and returns it', () => {
      const session = store.createSession('client-1', [])
      expect(session.id).toMatch(/^session-/)
      expect(session.workspaces).toHaveLength(0)
    })

    it('creates session with workspaces', () => {
      const session = store.createSession('client-1', [
        makeWorkspace({ id: 'ws-1', path: '/workspace/a' })
      ])
      expect(session.workspaces).toHaveLength(1)
      expect(session.workspaces[0].path).toBe('/workspace/a')
    })

    it('generates workspace id if not provided', () => {
      const ws = makeWorkspace({ path: '/workspace/a' })
      delete (ws as any).id
      const session = store.createSession('client-1', [ws])
      expect(session.workspaces[0].id).toMatch(/^ws-/)
    })

    it('sets createdAt and lastActivity timestamps', () => {
      const before = Date.now()
      const session = store.createSession('client-1', [])
      const after = Date.now()
      expect(session.createdAt).toBeGreaterThanOrEqual(before)
      expect(session.createdAt).toBeLessThanOrEqual(after)
      expect(session.lastActivity).toBeGreaterThanOrEqual(before)
    })
  })

  describe('getSession', () => {
    it('returns null for non-existent session', () => {
      expect(store.getSession('does-not-exist')).toBeNull()
    })

    it('returns the session after creation', () => {
      const created = store.createSession('client-1', [])
      const fetched = store.getSession(created.id)
      expect(fetched).not.toBeNull()
      expect(fetched!.id).toBe(created.id)
    })
  })

  describe('listSessions', () => {
    it('returns empty array when no sessions', () => {
      expect(store.listSessions()).toHaveLength(0)
    })

    it('returns all sessions', () => {
      store.createSession('client-1', [])
      store.createSession('client-2', [])
      expect(store.listSessions()).toHaveLength(2)
    })
  })

  describe('deleteSession', () => {
    it('returns false when deleting non-existent session', () => {
      expect(store.deleteSession('nope')).toBe(false)
    })

    it('returns true and removes session', () => {
      const session = store.createSession('client-1', [])
      expect(store.deleteSession(session.id)).toBe(true)
      expect(store.getSession(session.id)).toBeNull()
    })
  })

  describe('updateSession', () => {
    it('returns null for non-existent session', () => {
      const result = store.updateSession('client-1', 'bad-id', [])
      expect(result).toBeNull()
    })

    it('updates workspaces and lastActivity', () => {
      const session = store.createSession('client-1', [
        makeWorkspace({ id: 'ws-1', path: '/old' })
      ])
      const updated = store.updateSession('client-1', session.id, [
        makeWorkspace({ id: 'ws-1', path: '/new' })
      ])
      expect(updated).not.toBeNull()
      expect(updated!.workspaces[0].path).toBe('/new')
    })

    it('preserves workspace createdAt when matching by id', () => {
      const session = store.createSession('client-1', [
        makeWorkspace({ id: 'ws-1', path: '/ws' })
      ])
      const originalCreatedAt = session.workspaces[0].createdAt

      const updated = store.updateSession('client-1', session.id, [
        makeWorkspace({ id: 'ws-1', path: '/ws' })
      ])
      expect(updated!.workspaces[0].createdAt).toBe(originalCreatedAt)
    })

  })

  describe('initializeDefaultSession', () => {
    it('creates default session and sets defaultSessionId', () => {
      const session = store.initializeDefaultSession('client-1')
      expect(store.getDefaultSessionId()).toBe(session.id)
    })
  })

  describe('getOrCreateDefaultSession', () => {
    it('creates a new session when none exists', () => {
      const session = store.getOrCreateDefaultSession('client-1')
      expect(session.id).toMatch(/^session-/)
    })

    it('returns existing default session on second call', () => {
      const first = store.getOrCreateDefaultSession('client-1')
      const second = store.getOrCreateDefaultSession('client-2')
      expect(second.id).toBe(first.id)
    })

    it('creates new session if default was deleted', () => {
      const first = store.getOrCreateDefaultSession('client-1')
      store.deleteSession(first.id)
      const second = store.getOrCreateDefaultSession('client-2')
      expect(second.id).not.toBe(first.id)
    })
  })

  describe('detachClient', () => {
    it('does nothing for unknown client', () => {
      expect(() => store.detachClient('unknown-client')).not.toThrow()
    })

    it('removes client from attachment map after detach', () => {
      store.createSession('client-1', [])
      store.detachClient('client-1')
      // Should not throw on second detach (no attachments)
      expect(() => store.detachClient('client-1')).not.toThrow()
    })
  })
})
