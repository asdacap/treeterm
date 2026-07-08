// Leveled console logger for the renderer.
//
// The renderer emits a lot of per-mount / per-render lifecycle chatter that is
// useful when debugging but drowns the console in normal use. Route those
// through `log.debug` so they are suppressed by default (level `warn`), while
// genuine problems still surface via `log.warn` / `log.error`.
//
// Override at runtime without a rebuild:
//   localStorage.setItem('treeterm:logLevel', 'debug'); location.reload()
// or programmatically via `setLogLevel(LogLevel.Debug)`.

export enum LogLevel {
  Debug = 0,
  Info = 1,
  Warn = 2,
  Error = 3,
  Silent = 4,
}

const LEVEL_BY_NAME: Record<string, LogLevel> = {
  debug: LogLevel.Debug,
  info: LogLevel.Info,
  warn: LogLevel.Warn,
  error: LogLevel.Error,
  silent: LogLevel.Silent,
}

const STORAGE_KEY = 'treeterm:logLevel'
const DEFAULT_LEVEL = LogLevel.Warn

function readStoredLevel(): LogLevel {
  if (typeof localStorage === 'undefined') return DEFAULT_LEVEL
  const raw = localStorage.getItem(STORAGE_KEY)
  if (raw === null) return DEFAULT_LEVEL
  return LEVEL_BY_NAME[raw.toLowerCase()] ?? DEFAULT_LEVEL
}

let currentLevel: LogLevel = readStoredLevel()

export function getLogLevel(): LogLevel {
  return currentLevel
}

export function setLogLevel(level: LogLevel): void {
  currentLevel = level
}

export interface Logger {
  debug(...args: unknown[]): void
  info(...args: unknown[]): void
  warn(...args: unknown[]): void
  error(...args: unknown[]): void
}

export const log: Logger = {
  debug: (...args) => { if (currentLevel <= LogLevel.Debug) console.debug(...args) },
  info: (...args) => { if (currentLevel <= LogLevel.Info) console.info(...args) },
  warn: (...args) => { if (currentLevel <= LogLevel.Warn) console.warn(...args) },
  error: (...args) => { if (currentLevel <= LogLevel.Error) console.error(...args) },
}
