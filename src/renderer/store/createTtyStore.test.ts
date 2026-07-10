import { describe, it, expect, vi } from 'vitest'
import { createTtyStore, type TtyTerminalDeps } from './createTtyStore'
import { toDisposable } from '../../shared/lifecycle'

function makeMockTerminalDeps(): TtyTerminalDeps {
  return {
    write: vi.fn<(handle: string, data: string) => Promise<void>>().mockResolvedValue(undefined),
    resize: vi.fn(),
    kill: vi.fn(),
  }
}

const noopSubscription = () => toDisposable(() => {})

describe('createTtyStore', () => {
  it('creates a store with correct ptyId', () => {
    const terminal = makeMockTerminalDeps()
    const tty = createTtyStore('pty-1', 'handle-1', terminal, noopSubscription())
    expect(tty.getState().ptyId).toBe('pty-1')
  })

  it('write delegates to terminal.write with handle', async () => {
    const terminal = makeMockTerminalDeps()
    const tty = createTtyStore('pty-1', 'handle-1', terminal, noopSubscription())
    await tty.getState().write('hello')
    expect(terminal.write).toHaveBeenCalledWith('handle-1', 'hello')
  })

  it('resize delegates to terminal.resize with handle', () => {
    const terminal = makeMockTerminalDeps()
    const tty = createTtyStore('pty-1', 'handle-1', terminal, noopSubscription())
    tty.getState().resize(80, 24)
    expect(terminal.resize).toHaveBeenCalledWith('handle-1', 80, 24)
  })

  it('kill delegates to terminal.kill with ptyId', () => {
    const terminal = makeMockTerminalDeps()
    const tty = createTtyStore('pty-1', 'handle-1', terminal, noopSubscription())
    tty.getState().kill()
    expect(terminal.kill).toHaveBeenCalledWith('pty-1')
  })

  it('dispose releases the event subscription', () => {
    const unsubscribe = vi.fn()
    const tty = createTtyStore('pty-1', 'handle-1', makeMockTerminalDeps(), toDisposable(unsubscribe))

    tty.dispose()

    expect(unsubscribe).toHaveBeenCalledTimes(1)
  })

  it('dispose is idempotent', () => {
    const unsubscribe = vi.fn()
    const tty = createTtyStore('pty-1', 'handle-1', makeMockTerminalDeps(), toDisposable(unsubscribe))

    tty.dispose()
    tty.dispose()

    expect(unsubscribe).toHaveBeenCalledTimes(1)
  })

  // dispose() releases this window's attachment; kill() destroys the daemon-side PTY.
  // Collapsing the two verbs is how PTYs get orphaned — see AGENTS.md "Unmount is not close".
  it('dispose does not kill the PTY', () => {
    const terminal = makeMockTerminalDeps()
    const tty = createTtyStore('pty-1', 'handle-1', terminal, noopSubscription())

    tty.dispose()

    expect(terminal.kill).not.toHaveBeenCalled()
  })
})
