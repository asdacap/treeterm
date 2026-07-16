import type { Workspace, Session, SessionLock, WorkspaceRef } from '../types'
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
    favouritePaths: [],
    createdAt: 0,
    lastActivity: 0,
    ...overrides,
  }
}

export function makeWorkspaceRef(overrides: Partial<WorkspaceRef> = {}): WorkspaceRef {
  return {
    id: 'ws-1',
    path: '/test',
    ...overrides,
  }
}

export function makeSession(overrides: Partial<Session> = {}): Session {
  return {
    id: 'session-1',
    workspaceRefs: [],
    workspaceDataDir: '/test/.treeterm/workspaces',
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
