import type { Workspace, Session, SessionLock } from '../types'
import { WorkspaceStatus } from '../types'

export function makeWorkspace(overrides: Partial<Workspace> = {}): Workspace {
  return {
    id: 'ws-1',
    name: 'test',
    path: '/test',
    status: WorkspaceStatus.Active,
    isGitRepo: false,
    isWorktree: false,
    appStates: {},
    metadata: {},
    createdAt: 0,
    lastActivity: 0,
    ...overrides,
  }
}

export function makeSession(overrides: Partial<Session> = {}): Session {
  return {
    id: 'session-1',
    workspaces: [],
    createdAt: 0,
    lastActivity: 0,
    version: 1,
    ...overrides,
  }
}

export function makeSessionLock(overrides: Partial<SessionLock> = {}): SessionLock {
  return {
    acquiredAt: 0,
    expiresAt: 0,
    ...overrides,
  }
}
