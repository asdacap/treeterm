import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'

import { log, getLogLevel, setLogLevel, LogLevel } from './logger'

describe('logger', () => {
  let debugSpy: ReturnType<typeof vi.spyOn>
  let infoSpy: ReturnType<typeof vi.spyOn>
  let warnSpy: ReturnType<typeof vi.spyOn>
  let errorSpy: ReturnType<typeof vi.spyOn>
  const originalLevel = getLogLevel()

  beforeEach(() => {
    debugSpy = vi.spyOn(console, 'debug').mockImplementation(() => {})
    infoSpy = vi.spyOn(console, 'info').mockImplementation(() => {})
    warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {})
    errorSpy = vi.spyOn(console, 'error').mockImplementation(() => {})
  })

  afterEach(() => {
    setLogLevel(originalLevel)
    vi.restoreAllMocks()
  })

  it('defaults to Warn — suppresses debug/info, emits warn/error', () => {
    expect(getLogLevel()).toBe(LogLevel.Warn)

    log.debug('d')
    log.info('i')
    log.warn('w')
    log.error('e')

    expect(debugSpy).not.toHaveBeenCalled()
    expect(infoSpy).not.toHaveBeenCalled()
    expect(warnSpy).toHaveBeenCalledWith('w')
    expect(errorSpy).toHaveBeenCalledWith('e')
  })

  it('emits everything at Debug', () => {
    setLogLevel(LogLevel.Debug)

    log.debug('d')
    log.info('i')
    log.warn('w')
    log.error('e')

    expect(debugSpy).toHaveBeenCalledWith('d')
    expect(infoSpy).toHaveBeenCalledWith('i')
    expect(warnSpy).toHaveBeenCalledWith('w')
    expect(errorSpy).toHaveBeenCalledWith('e')
  })

  it('suppresses everything at Silent', () => {
    setLogLevel(LogLevel.Silent)

    log.debug('d')
    log.info('i')
    log.warn('w')
    log.error('e')

    expect(debugSpy).not.toHaveBeenCalled()
    expect(infoSpy).not.toHaveBeenCalled()
    expect(warnSpy).not.toHaveBeenCalled()
    expect(errorSpy).not.toHaveBeenCalled()
  })

  it('at Error, only error is emitted', () => {
    setLogLevel(LogLevel.Error)

    log.warn('w')
    log.error('e')

    expect(warnSpy).not.toHaveBeenCalled()
    expect(errorSpy).toHaveBeenCalledWith('e')
  })

  it('forwards all arguments to the console sink', () => {
    setLogLevel(LogLevel.Debug)
    const payload = { cols: 80, rows: 24 }

    log.debug('[tab-1] resize', payload)

    expect(debugSpy).toHaveBeenCalledWith('[tab-1] resize', payload)
  })
})
