#!/usr/bin/env node

/**
 * TreeTerm Daemon - Persistent PTY session manager
 * Runs as a standalone process independent of Electron app lifecycle
 */

import * as fs from 'fs'
import * as path from 'path'
import * as os from 'os'
import { DaemonPtyManager } from './ptyManager'
import { SocketServer, getDefaultSocketPath } from './socketServer'
import { SessionStore } from './sessionStore'

const DAEMON_PID_FILE = path.join(os.homedir(), '.treeterm', 'daemon.pid')
const DAEMON_LOG_FILE = path.join(os.homedir(), '.treeterm', 'daemon.log')

interface DaemonConfig {
  socketPath: string
  orphanTimeout: number // minutes
  scrollbackLimit: number
  logFile: string
}

function getConfig(): DaemonConfig {
  return {
    socketPath: process.env.TREETERM_SOCKET_PATH || getDefaultSocketPath(),
    orphanTimeout: parseInt(process.env.TREETERM_ORPHAN_TIMEOUT || '0', 10),
    scrollbackLimit: parseInt(process.env.TREETERM_SCROLLBACK_LIMIT || '50000', 10),
    logFile: process.env.TREETERM_LOG_FILE || DAEMON_LOG_FILE
  }
}

function setupLogging(logFile: string): void {
  const logDir = path.dirname(logFile)
  if (!fs.existsSync(logDir)) {
    fs.mkdirSync(logDir, { recursive: true })
  }

  const logStream = fs.createWriteStream(logFile, { flags: 'a' })

  // Redirect stdout/stderr to log file
  process.stdout.write = (
    chunk: string | Uint8Array,
    _encodingOrCallback?: BufferEncoding | ((err?: Error | null) => void),
    _cb?: (err?: Error | null) => void
  ): boolean => {
    const timestamp = new Date().toISOString()
    const text = typeof chunk === 'string' ? chunk : chunk.toString()
    logStream.write(`[${timestamp}] ${text}`)
    return true
  }

  process.stderr.write = (
    chunk: string | Uint8Array,
    _encodingOrCallback?: BufferEncoding | ((err?: Error | null) => void),
    _cb?: (err?: Error | null) => void
  ): boolean => {
    const timestamp = new Date().toISOString()
    const text = typeof chunk === 'string' ? chunk : chunk.toString()
    logStream.write(`[${timestamp}] ERROR: ${text}`)
    return true
  }

  console.log('[daemon] logging to', logFile)
}

function writePidFile(): void {
  const pidDir = path.dirname(DAEMON_PID_FILE)
  if (!fs.existsSync(pidDir)) {
    fs.mkdirSync(pidDir, { recursive: true })
  }
  fs.writeFileSync(DAEMON_PID_FILE, process.pid.toString(), 'utf-8')
  console.log('[daemon] PID', process.pid, 'written to', DAEMON_PID_FILE)
}

function removePidFile(): void {
  if (fs.existsSync(DAEMON_PID_FILE)) {
    fs.unlinkSync(DAEMON_PID_FILE)
  }
}

async function main(): Promise<void> {
  const config = getConfig()

  // Setup logging first to ensure all output goes to log file
  setupLogging(config.logFile)

  console.log('========================================')
  console.log('TreeTerm Daemon Starting')
  console.log('========================================')
  console.log('Socket path:', config.socketPath)
  console.log('Orphan timeout:', config.orphanTimeout, 'minutes')
  console.log('Scrollback limit:', config.scrollbackLimit, 'lines')
  console.log('Log file:', config.logFile)
  console.log('========================================')

  // Write PID file
  writePidFile()

  // Initialize components
  const ptyManager = new DaemonPtyManager(config.orphanTimeout, config.scrollbackLimit)
  const sessionStore = new SessionStore()
  const socketServer = new SocketServer(config.socketPath, ptyManager, sessionStore)

  // Load persisted sessions (future enhancement)
  // For now, we start fresh each time the daemon starts
  // Note: SessionStore is memory-only (not persisted to disk)

  // Start socket server
  try {
    await socketServer.start()
    console.log('[daemon] daemon is ready')
  } catch (error) {
    console.error('[daemon] failed to start socket server:', error)
    process.exit(1)
  }

  // Graceful shutdown handlers
  const shutdown = async (signal: string) => {
    console.log(`[daemon] received ${signal}, shutting down...`)

    // Save sessions before shutdown (future enhancement)
    const sessions = ptyManager.listSessions()
    console.log(`[daemon] ${sessions.length} active sessions`)

    // Cleanup
    socketServer.stop()
    ptyManager.shutdown()
    removePidFile()

    console.log('[daemon] shutdown complete')
    process.exit(0)
  }

  process.on('SIGTERM', () => shutdown('SIGTERM'))
  process.on('SIGINT', () => shutdown('SIGINT'))

  // Keep alive
  process.on('uncaughtException', (error) => {
    console.error('[daemon] uncaught exception:', error)
    shutdown('uncaughtException').catch(console.error)
  })

  process.on('unhandledRejection', (reason) => {
    console.error('[daemon] unhandled rejection:', reason)
  })
}

// Check if already running
if (fs.existsSync(DAEMON_PID_FILE)) {
  const pid = parseInt(fs.readFileSync(DAEMON_PID_FILE, 'utf-8'), 10)
  try {
    // Check if process is still alive
    process.kill(pid, 0)
    // Daemon already running, exit silently
    process.exit(0)
  } catch {
    // Process not running, remove stale PID file
    fs.unlinkSync(DAEMON_PID_FILE)
  }
}

// Start daemon
main().catch((error) => {
  console.error('[daemon] fatal error:', error)
  removePidFile()
  process.exit(1)
})
