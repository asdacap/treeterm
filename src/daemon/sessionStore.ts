/**
 * In-memory session store for daemon
 * A session contains multiple workspaces that were open together
 */

import type { DaemonSession, DaemonWorkspace } from './protocol'

function generateSessionId(): string {
  return `session-${Date.now()}-${Math.random().toString(36).slice(2, 9)}`
}

export class SessionStore {
  // Map from sessionId -> session
  private sessions: Map<string, DaemonSession> = new Map()

  // Track which client IDs are attached to which session IDs
  private clientAttachments: Map<string, Set<string>> = new Map() // clientId -> Set<sessionId>

  /**
   * Create a new session with workspaces
   * Returns the created session
   */
  createSession(
    clientId: string,
    workspaces: Omit<DaemonWorkspace, 'createdAt' | 'lastActivity' | 'attachedClients'>[]
  ): DaemonSession {
    const sessionId = generateSessionId()
    const now = Date.now()

    // Add metadata to each workspace
    const fullWorkspaces: DaemonWorkspace[] = workspaces.map(ws => ({
      ...ws,
      createdAt: now,
      lastActivity: now,
      attachedClients: 1
    }))

    const session: DaemonSession = {
      id: sessionId,
      workspaces: fullWorkspaces,
      createdAt: now,
      lastActivity: now,
      attachedClients: 1
    }

    this.sessions.set(sessionId, session)
    this.attachClient(clientId, sessionId)

    console.log(`[sessionStore] created session: ${sessionId} with ${workspaces.length} workspace(s)`)
    return session
  }

  /**
   * Update a session's workspaces
   * Returns the updated session or null if not found
   */
  updateSession(
    clientId: string,
    sessionId: string,
    workspaces: Omit<DaemonWorkspace, 'createdAt' | 'lastActivity' | 'attachedClients'>[]
  ): DaemonSession | null {
    const session = this.sessions.get(sessionId)
    if (!session) return null

    const now = Date.now()

    // Update workspace metadata, preserving createdAt
    const fullWorkspaces: DaemonWorkspace[] = workspaces.map(ws => {
      const existing = session.workspaces.find(w => w.path === ws.path)
      return {
        ...ws,
        createdAt: existing?.createdAt || now,
        lastActivity: now,
        attachedClients: existing?.attachedClients || 1
      }
    })

    const updated: DaemonSession = {
      ...session,
      workspaces: fullWorkspaces,
      lastActivity: now,
      // Increment attachedClients if this is a new client
      attachedClients: this.isClientAttached(clientId, sessionId)
        ? session.attachedClients
        : session.attachedClients + 1
    }

    this.sessions.set(sessionId, updated)
    this.attachClient(clientId, sessionId)

    console.log(`[sessionStore] updated session: ${sessionId} with ${workspaces.length} workspace(s)`)
    return updated
  }

  /**
   * Get session by ID
   */
  getSession(sessionId: string): DaemonSession | null {
    return this.sessions.get(sessionId) || null
  }

  /**
   * List all sessions
   */
  listSessions(): DaemonSession[] {
    return Array.from(this.sessions.values())
  }

  /**
   * Delete a session
   */
  deleteSession(sessionId: string): boolean {
    const existed = this.sessions.delete(sessionId)
    if (existed) {
      console.log(`[sessionStore] deleted session: ${sessionId}`)
    }
    return existed
  }

  /**
   * Check if a client is attached to a session
   */
  private isClientAttached(clientId: string, sessionId: string): boolean {
    const sessions = this.clientAttachments.get(clientId)
    return sessions ? sessions.has(sessionId) : false
  }

  /**
   * Attach a client to a session
   */
  private attachClient(clientId: string, sessionId: string): void {
    if (!this.clientAttachments.has(clientId)) {
      this.clientAttachments.set(clientId, new Set())
    }
    this.clientAttachments.get(clientId)!.add(sessionId)
  }

  /**
   * Detach a client from all sessions (called when client disconnects)
   */
  detachClient(clientId: string): void {
    const sessionIds = this.clientAttachments.get(clientId)
    if (!sessionIds) {
      console.log(`[sessionStore] detachClient: client ${clientId} has no attachments`)
      return
    }

    for (const sessionId of sessionIds) {
      const session = this.sessions.get(sessionId)
      if (session && session.attachedClients > 0) {
        session.attachedClients--
        session.lastActivity = Date.now()
        console.log(`[sessionStore] detached client from ${sessionId}, clients remaining: ${session.attachedClients}`)
      }
    }

    this.clientAttachments.delete(clientId)
    console.log(`[sessionStore] client ${clientId} detached from ${sessionIds.size} session(s)`)
  }
}
