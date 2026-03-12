#!/usr/bin/env node

/**
 * TreeTerm Daemon - Persistent PTY session manager
 * Runs as a standalone process independent of Electron app lifecycle
 */

import * as fs from 'fs'
import * as path from 'path'
import * as os from 'os'
import { getDefaultSocketPath } from './socketPath'
import { initLogger, createModuleLogger } from './logger'

const DEFAULT_DAEMON_PID_FILE = path.join(os.homedir(), '.treeterm', 'daemon.pid')
const DAEMON_LOG_FILE = path.join(os.homedir(), '.treeterm', 'daemon.log')

interface DaemonConfig {
  socketPath: string
  pidFile: string
  orphanTimeout: number // minutes
  scrollbackLimit: number
  logFile: string
  logLevel: string
  logPretty: boolean
}

function getConfig(): DaemonConfig {
  return {
    socketPath: process.env.TREETERM_SOCKET_PATH || getDefaultSocketPath(),
    pidFile: process.env.TREETERM_PID_FILE || DEFAULT_DAEMON_PID_FILE,
    orphanTimeout: parseInt(process.env.TREETERM_ORPHAN_TIMEOUT || '0', 10),
    scrollbackLimit: parseInt(process.env.TREETERM_SCROLLBACK_LIMIT || '50000', 10),
    logFile: process.env.TREETERM_LOG_FILE || DAEMON_LOG_FILE,
    logLevel: process.env.TREETERM_LOG_LEVEL || 'info',
    logPretty: process.env.TREETERM_LOG_PRETTY === '1'
  }
}


function writePidFile(pidFile: string, log: ReturnType<typeof createModuleLogger>): void {
  const pidDir = path.dirname(pidFile)
  if (!fs.existsSync(pidDir)) {
    fs.mkdirSync(pidDir, { recursive: true })
  }
  fs.writeFileSync(pidFile, process.pid.toString(), 'utf-8')
  log.info({ pid: process.pid, pidFile }, 'PID file written')
}

function removePidFile(pidFile: string): void {
  if (fs.existsSync(pidFile)) {
    fs.unlinkSync(pidFile)
  }
}

async function main(): Promise<void> {
  const config = getConfig()

  // Initialize logging first
  initLogger({
    logFile: config.logFile,
    level: config.logLevel,
    pretty: config.logPretty
  })

  const log = createModuleLogger('daemon')

  log.info('========================================')
  log.info('TreeTerm Daemon Starting')
  log.info('========================================')
  log.info({ socketPath: config.socketPath }, 'socket path')
  log.info({ orphanTimeout: config.orphanTimeout }, 'orphan timeout (minutes)')
  log.info({ scrollbackLimit: config.scrollbackLimit }, 'scrollback limit (lines)')
  log.info({ logFile: config.logFile }, 'log file')
  log.info({ logLevel: config.logLevel }, 'log level')
  log.info('========================================')

  // Write PID file
  writePidFile(config.pidFile, log)

  // Dynamic imports after logger is initialized
  const { DaemonPtyManager } = await import('./ptyManager')
  const { GrpcServer } = await import('./grpcServer')
  const { SessionStore } = await import('./sessionStore')

  // Initialize components
  const ptyManager = new DaemonPtyManager(config.orphanTimeout, config.scrollbackLimit)
  const sessionStore = new SessionStore()

  // Initialize default session on daemon startup
  // This ensures there's always a session available for clients
  const defaultSession = sessionStore.initializeDefaultSession('daemon-init')
  log.info({ defaultSessionId: defaultSession.id }, 'default session created at startup')

  const grpcServer = new GrpcServer(config.socketPath, ptyManager, sessionStore)

  // Load persisted sessions (future enhancement)
  // For now, we start fresh each time the daemon starts
  // Note: SessionStore is memory-only (not persisted to disk)

  // Start gRPC server
  try {
    await grpcServer.start()
    log.info('daemon is ready')
  } catch (error) {
    log.fatal({ err: error }, 'failed to start gRPC server')
    process.exit(1)
  }

  // Graceful shutdown handlers
  const shutdown = async (signal: string) => {
    log.info({ signal }, 'shutdown signal received')

    // Save sessions before shutdown (future enhancement)
    const sessions = ptyManager.listSessions()
    log.info({ sessionCount: sessions.length }, 'active sessions')

    // Cleanup
    grpcServer.stop()
    ptyManager.shutdown()
    removePidFile(config.pidFile)

    log.info('shutdown complete')
    process.exit(0)
  }

  process.on('SIGTERM', () => shutdown('SIGTERM'))
  process.on('SIGINT', () => shutdown('SIGINT'))

  // Keep alive
  process.on('uncaughtException', (error) => {
    log.fatal({ err: error }, 'uncaught exception')
    shutdown('uncaughtException').catch((err) => log.error({ err }, 'error during shutdown'))
  })

  process.on('unhandledRejection', (reason) => {
    log.error({ reason }, 'unhandled rejection')
  })
}

// Get config early to determine PID file location
const startupConfig = getConfig()

// Check if already running
if (fs.existsSync(startupConfig.pidFile)) {
  const pid = parseInt(fs.readFileSync(startupConfig.pidFile, 'utf-8'), 10)
  try {
    // Check if process is still alive
    process.kill(pid, 0)
    // Daemon already running, exit silently
    process.exit(0)
  } catch {
    // Process not running, remove stale PID file
    fs.unlinkSync(startupConfig.pidFile)
  }
}

// Start daemon
main().catch((error) => {
  // Logger might not be initialized yet, so use console.error as fallback
  try {
    const log = createModuleLogger('daemon')
    log.fatal({ err: error }, 'fatal error during startup')
  } catch {
    // Logger not initialized, use console.error
    console.error('[daemon] fatal error:', error)
  }
  removePidFile(startupConfig.pidFile)
  process.exit(1)
})
