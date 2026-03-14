import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import * as fs from 'fs'
import * as path from 'path'
import * as os from 'os'

const mockChildLogger = {
  info: vi.fn(),
  debug: vi.fn(),
  warn: vi.fn(),
  error: vi.fn(),
  fatal: vi.fn(),
  child: vi.fn(() => mockChildLogger)
}

const mockPino = vi.fn(() => mockChildLogger)
// Add static properties to the mock function
Object.assign(mockPino, {
  stdTimeFunctions: {
    isoTime: 'ISO_TIME_FUNCTION'
  }
})

vi.mock('pino', () => ({
  default: mockPino,
  stdTimeFunctions: {
    isoTime: 'ISO_TIME_FUNCTION'
  }
}))

vi.mock('fs')

describe('logger', () => {
  const mockLogDir = path.join(os.homedir(), '.treeterm')

  beforeEach(() => {
    vi.clearAllMocks()
  })

  afterEach(() => {
    vi.unstubAllEnvs()
    vi.resetModules()
  })

  describe('initLogger', () => {
    it('creates log directory if it does not exist', async () => {
      vi.mocked(fs.existsSync).mockReturnValue(false)
      vi.mocked(fs.mkdirSync).mockImplementation(() => undefined)

      const { initLogger } = await import('./logger')
      const logger = initLogger()

      expect(fs.mkdirSync).toHaveBeenCalledWith(mockLogDir, { recursive: true })
      expect(logger).toBeDefined()
    })

    it('uses default log file path when not specified', async () => {
      vi.mocked(fs.existsSync).mockReturnValue(true)

      const { initLogger } = await import('./logger')
      const logger = initLogger()

      expect(logger).toBeDefined()
      expect(mockPino).toHaveBeenCalled()
    })

    it('uses environment variable for log level', async () => {
      vi.mocked(fs.existsSync).mockReturnValue(true)
      vi.stubEnv('TREETERM_LOG_LEVEL', 'debug')

      const { initLogger } = await import('./logger')
      initLogger()

      const pinoConfig = mockPino.mock.calls[0][0]
      expect(pinoConfig.level).toBe('debug')
    })

    it('enables pretty printing when pretty option is true', async () => {
      vi.mocked(fs.existsSync).mockReturnValue(true)

      const { initLogger } = await import('./logger')
      initLogger({ pretty: true })

      const pinoConfig = mockPino.mock.calls[0][0]
      expect(pinoConfig.transport.target).toBe('pino-pretty')
    })

    it('returns same logger on multiple calls', async () => {
      vi.mocked(fs.existsSync).mockReturnValue(true)

      const { initLogger } = await import('./logger')
      const logger1 = initLogger()
      const logger2 = initLogger()

      expect(logger1).toBe(logger2)
    })
  })

  describe('getLogger', () => {
    it('throws error if logger not initialized', async () => {
      const { getLogger } = await import('./logger')
      expect(() => getLogger()).toThrow('Logger not initialized. Call initLogger() first.')
    })

    it('returns logger after initialization', async () => {
      vi.mocked(fs.existsSync).mockReturnValue(true)
      
      const { initLogger, getLogger } = await import('./logger')
      const initializedLogger = initLogger()
      const retrievedLogger = getLogger()

      expect(retrievedLogger).toBe(initializedLogger)
    })
  })

  describe('createModuleLogger', () => {
    it('creates child logger with module name', async () => {
      vi.mocked(fs.existsSync).mockReturnValue(true)
      
      const { initLogger, createModuleLogger } = await import('./logger')
      initLogger()
      createModuleLogger('testModule')

      expect(mockChildLogger.child).toHaveBeenCalledWith({ module: 'testModule' })
    })
  })
})
