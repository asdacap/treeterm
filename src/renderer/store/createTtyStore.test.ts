import { describe, it, expect, vi } from 'vitest'
import { createTtyStore } from './createTtyStore'
import type { TerminalApi } from '../types'

function makeMockTerminalApi(): TerminalApi {
  return {
    create: vi.fn(),
    attach: vi.fn(),
    list: vi.fn(),
    write: vi.fn(),
    resize: vi.fn(),
    kill: vi.fn(),
    isAlive: vi.fn().mockResolvedValue(true),
    onData: vi.fn().mockReturnValue(() => {}),
    onExit: vi.fn().mockReturnValue(() => {}),
    onNewTerminal: vi.fn().mockReturnValue(() => {}),
    onShowSessions: vi.fn().mockReturnValue(() => {}),
    onActiveProcessesOpen: vi.fn().mockReturnValue(() => {}),
  } as TerminalApi
}

describe('createTtyStore', () => {
  it('creates a store with correct ptyId', () => {
    const terminal = makeMockTerminalApi()
    const tty = createTtyStore('pty-1', 'handle-1', terminal)
    expect(tty.getState().ptyId).toBe('pty-1')
  })

  it('write delegates to terminal.write with handle', () => {
    const terminal = makeMockTerminalApi()
    const tty = createTtyStore('pty-1', 'handle-1', terminal)
    tty.getState().write('hello')
    expect(terminal.write).toHaveBeenCalledWith('handle-1', 'hello')
  })

  it('resize delegates to terminal.resize with handle', () => {
    const terminal = makeMockTerminalApi()
    const tty = createTtyStore('pty-1', 'handle-1', terminal)
    tty.getState().resize(80, 24)
    expect(terminal.resize).toHaveBeenCalledWith('handle-1', 80, 24)
  })

  it('kill delegates to terminal.kill with ptyId', () => {
    const terminal = makeMockTerminalApi()
    const tty = createTtyStore('pty-1', 'handle-1', terminal)
    tty.getState().kill()
    expect(terminal.kill).toHaveBeenCalledWith('pty-1')
  })

  it('isAlive delegates to terminal.isAlive with ptyId', async () => {
    const terminal = makeMockTerminalApi()
    const tty = createTtyStore('pty-1', 'handle-1', terminal)
    const result = await tty.getState().isAlive()
    expect(result).toBe(true)
    expect(terminal.isAlive).toHaveBeenCalledWith('pty-1')
  })

  it('onData delegates to terminal.onData with handle', () => {
    const terminal = makeMockTerminalApi()
    const tty = createTtyStore('pty-1', 'handle-1', terminal)
    const cb = vi.fn()
    tty.getState().onData(cb)
    expect(terminal.onData).toHaveBeenCalledWith('handle-1', cb)
  })

  it('onExit delegates to terminal.onExit with handle', () => {
    const terminal = makeMockTerminalApi()
    const tty = createTtyStore('pty-1', 'handle-1', terminal)
    const cb = vi.fn()
    tty.getState().onExit(cb)
    expect(terminal.onExit).toHaveBeenCalledWith('handle-1', cb)
  })
})
