/**
 * Logger module for TreeTerm Daemon
 * Uses pino for structured JSON logging with child logger support
 */

import pino, { Logger } from 'pino'
import * as path from 'path'
import * as os from 'os'
import * as fs from 'fs'

const DAEMON_LOG_FILE = path.join(os.homedir(), '.treeterm', 'daemon.log')

export interface LoggerConfig {
  logFile?: string
  level?: string
  pretty?: boolean
}

let rootLogger: Logger | null = null

export function initLogger(config: LoggerConfig = {}): Logger {
  const logFile = config.logFile || DAEMON_LOG_FILE
  const level = config.level || process.env.TREETERM_LOG_LEVEL || 'info'
  const pretty = config.pretty || process.env.TREETERM_LOG_PRETTY === '1'

  // Ensure log directory exists
  const logDir = path.dirname(logFile)
  if (!fs.existsSync(logDir)) {
    fs.mkdirSync(logDir, { recursive: true })
  }

  const transport = pretty
    ? {
        target: 'pino-pretty',
        options: {
          colorize: true,
          translateTime: 'SYS:standard',
          ignore: 'pid,hostname'
        }
      }
    : {
        target: 'pino/file',
        options: { destination: logFile, append: true }
      }

  rootLogger = pino({
    level,
    transport,
    base: { pid: process.pid },
    timestamp: pino.stdTimeFunctions.isoTime
  })

  return rootLogger
}

export function getLogger(): Logger {
  if (!rootLogger) {
    throw new Error('Logger not initialized. Call initLogger() first.')
  }
  return rootLogger
}

/**
 * Create a child logger for a specific module
 * @param module - Module name (e.g., 'daemon', 'ptyManager', 'socketServer')
 */
export function createModuleLogger(module: string): Logger {
  return getLogger().child({ module })
}
