/**
 * Session persistence to disk
 * Saves session metadata and scrollback buffers
 */

import * as fs from 'fs'
import * as path from 'path'
import * as os from 'os'

export interface PersistedSession {
  id: string
  cwd: string
  env: Record<string, string>
  cols: number
  rows: number
  createdAt: number
  scrollbackPath: string
}

export class SessionStore {
  private storePath: string
  private scrollbackDir: string

  constructor(basePath?: string) {
    const base = basePath || path.join(os.homedir(), '.treeterm')
    this.storePath = path.join(base, 'sessions.json')
    this.scrollbackDir = path.join(base, 'scrollback')

    // Ensure directories exist
    this.ensureDirectories()
  }

  private ensureDirectories(): void {
    const baseDir = path.dirname(this.storePath)
    if (!fs.existsSync(baseDir)) {
      fs.mkdirSync(baseDir, { recursive: true })
    }
    if (!fs.existsSync(this.scrollbackDir)) {
      fs.mkdirSync(this.scrollbackDir, { recursive: true })
    }
  }

  save(sessions: PersistedSession[]): void {
    try {
      fs.writeFileSync(this.storePath, JSON.stringify(sessions, null, 2), 'utf-8')
      console.log(`[sessionStore] saved ${sessions.length} sessions to ${this.storePath}`)
    } catch (error) {
      console.error('[sessionStore] failed to save sessions:', error)
    }
  }

  load(): PersistedSession[] {
    try {
      if (!fs.existsSync(this.storePath)) {
        return []
      }

      const data = fs.readFileSync(this.storePath, 'utf-8')
      const sessions = JSON.parse(data) as PersistedSession[]
      console.log(`[sessionStore] loaded ${sessions.length} sessions from ${this.storePath}`)
      return sessions
    } catch (error) {
      console.error('[sessionStore] failed to load sessions:', error)
      return []
    }
  }

  saveScrollback(sessionId: string, buffer: string[]): void {
    try {
      const scrollbackPath = this.getScrollbackPath(sessionId)
      const content = buffer.join('')
      fs.writeFileSync(scrollbackPath, content, 'utf-8')
    } catch (error) {
      console.error(`[sessionStore] failed to save scrollback for ${sessionId}:`, error)
    }
  }

  loadScrollback(sessionId: string, limit: number = 50000): string[] {
    try {
      const scrollbackPath = this.getScrollbackPath(sessionId)
      if (!fs.existsSync(scrollbackPath)) {
        return []
      }

      const content = fs.readFileSync(scrollbackPath, 'utf-8')

      // Split by ANSI escape sequences or lines for better preservation
      const chunks = this.splitScrollback(content)

      // Truncate if exceeds limit
      if (chunks.length > limit) {
        return chunks.slice(chunks.length - limit)
      }

      return chunks
    } catch (error) {
      console.error(`[sessionStore] failed to load scrollback for ${sessionId}:`, error)
      return []
    }
  }

  deleteScrollback(sessionId: string): void {
    try {
      const scrollbackPath = this.getScrollbackPath(sessionId)
      if (fs.existsSync(scrollbackPath)) {
        fs.unlinkSync(scrollbackPath)
      }
    } catch (error) {
      console.error(`[sessionStore] failed to delete scrollback for ${sessionId}:`, error)
    }
  }

  clearAll(): void {
    try {
      // Delete sessions file
      if (fs.existsSync(this.storePath)) {
        fs.unlinkSync(this.storePath)
      }

      // Delete all scrollback files
      if (fs.existsSync(this.scrollbackDir)) {
        const files = fs.readdirSync(this.scrollbackDir)
        for (const file of files) {
          fs.unlinkSync(path.join(this.scrollbackDir, file))
        }
      }

      console.log('[sessionStore] cleared all persisted sessions')
    } catch (error) {
      console.error('[sessionStore] failed to clear sessions:', error)
    }
  }

  private getScrollbackPath(sessionId: string): string {
    return path.join(this.scrollbackDir, `${sessionId}.txt`)
  }

  /**
   * Split scrollback content into chunks
   * For now, we just store as one big chunk, but we could be smarter
   * about preserving ANSI escape sequences in the future
   */
  private splitScrollback(content: string): string[] {
    if (!content) return []
    // Return as single chunk to preserve all ANSI codes
    return [content]
  }
}
