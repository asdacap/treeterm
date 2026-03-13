/**
 * In-memory session store for daemon
 * A session contains multiple workspaces that were open together
 */

import type { Session, Workspace } from './protocol'
import { createModuleLogger } from './logger'

const log = createModuleLogger('sessionStore')

function generateSessionId(): string {
  return `session-${Date.now()}-${Math.random().toString(36).slice(2, 9)}`
}

function generateWorkspaceId(): string {
  return `ws-${Date.now()}-${Math.random().toString(36).slice(2, 9)}`
}

export class SessionStore {
  // Map from sessionId -> session
  private sessions: Map<string, Session> = new Map()

  // Track which client IDs are attached to which session IDs
  private clientAttachments: Map<string, Set<string>> = new Map() // clientId -> Set<sessionId>

  // The default session ID - always exists after daemon starts
  private defaultSessionId: string | null = null

  /**
   * Initialize the default session (called once on daemon startup)
   * Creates a new session with empty workspaces
   */
  initializeDefaultSession(clientId: string): Session {
    const session = this.createSession(clientId, [])
    this.defaultSessionId = session.id
    log.info({ sessionId: session.id }, 'default session initialized')
    return session
  }

  /**
   * Get the default session, creating it if necessary
   */
  getOrCreateDefaultSession(clientId: string): Session {
    // Return existing default session if it exists
    if (this.defaultSessionId) {
      const session = this.sessions.get(this.defaultSessionId)
      if (session) {
        // Attach this client to the session
        this.attachClient(clientId, session.id)
        return session
      }
      // Session was deleted, clear the reference
      this.defaultSessionId = null
    }

    // Create new default session
    return this.initializeDefaultSession(clientId)
  }

  /**
   * Get the default session ID (or null if not created yet)
   */
  getDefaultSessionId(): string | null {
    return this.defaultSessionId
  }

  /**
   * Create a new session with workspaces
   * Returns the created session
   */
  createSession(
    clientId: string,
    workspaces: Omit<Workspace, 'createdAt' | 'lastActivity' | 'attachedClients'>[]
  ): Session {
    const sessionId = generateSessionId()
    const now = Date.now()

    // Add metadata to each workspace, generating ID if not provided
    const fullWorkspaces: Workspace[] = workspaces.map(ws => ({
      ...ws,
      id: ws.id || generateWorkspaceId(),
      createdAt: now,
      lastActivity: now,
      attachedClients: 1
    }))

    const session: Session = {
      id: sessionId,
      workspaces: fullWorkspaces,
      createdAt: now,
      lastActivity: now,
      attachedClients: 1
    }

    this.sessions.set(sessionId, session)
    this.attachClient(clientId, sessionId)

    log.info({ sessionId, workspaceCount: workspaces.length }, 'session created')
    return session
  }

  /**
   * Update a session's workspaces
   * Returns the updated session or null if not found
   */
  updateSession(
    clientId: string,
    sessionId: string,
    workspaces: Omit<Workspace, 'createdAt' | 'lastActivity' | 'attachedClients'>[]
  ): Session | null {
    const session = this.sessions.get(sessionId)
    if (!session) return null

    const now = Date.now()

    // Update workspace metadata, preserving createdAt; match by id or path for backward compat
    const fullWorkspaces: Workspace[] = workspaces.map(ws => {
      const existing = session.workspaces.find(w => (ws.id && w.id === ws.id) || w.path === ws.path)
      return {
        ...ws,
        id: ws.id || existing?.id || generateWorkspaceId(),
        createdAt: existing?.createdAt || now,
        lastActivity: now,
        attachedClients: existing?.attachedClients || 1
      }
    })

    const updated: Session = {
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

    log.info({ sessionId, workspaceCount: workspaces.length }, 'session updated')
    return updated
  }

  /**
   * Get session by ID
   */
  getSession(sessionId: string): Session | null {
    return this.sessions.get(sessionId) || null
  }

  /**
   * List all sessions
   */
  listSessions(): Session[] {
    return Array.from(this.sessions.values())
  }

  /**
   * Delete a session
   */
  deleteSession(sessionId: string): boolean {
    const existed = this.sessions.delete(sessionId)
    if (existed) {
      log.info({ sessionId }, 'session deleted')
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
      log.debug({ clientId }, 'detachClient: no attachments')
      return
    }

    for (const sessionId of sessionIds) {
      const session = this.sessions.get(sessionId)
      if (session && session.attachedClients > 0) {
        session.attachedClients--
        session.lastActivity = Date.now()
        log.debug(
          { sessionId, clientsRemaining: session.attachedClients },
          'client detached from session'
        )
      }
    }

    this.clientAttachments.delete(clientId)
    log.info({ clientId, sessionCount: sessionIds.size }, 'client detached from all sessions')
  }
}
