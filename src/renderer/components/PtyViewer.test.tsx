// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, act, cleanup } from '@testing-library/react'
import PtyViewer from './PtyViewer'
import { PtyAttachKind, PtyEventType } from '../../shared/ipc-types'
import type { PtyEvent } from '../../shared/ipc-types'
import type { TerminalApi } from '../types'

// Fake xterm: opens a focusable element into the container, and holds write callbacks
// until `parse()` — the real terminal parses asynchronously.
const { terminals, FakeTerminal, windowFocusTarget } = vi.hoisted(() => {
  const terminals: InstanceType<typeof FakeTerminal>[] = []
  class FakeTerminal {
    readonly element = Object.assign(document.createElement('textarea'), { tabIndex: 0 })
    dataListener: ((data: string) => void) | null = null
    pendingWriteCallbacks: (() => void)[] = []
    constructor() { terminals.push(this) }
    open(container: HTMLElement): void { container.appendChild(this.element) }
    write(_data: string | Uint8Array, onWritten?: () => void): void {
      if (onWritten) this.pendingWriteCallbacks.push(onWritten)
    }
    parse(): void { for (const callback of this.pendingWriteCallbacks.splice(0)) callback() }
    resize(): void {}
    dispose(): void { this.element.remove() }
    onData(handler: (data: string) => void): { dispose(): void } {
      this.dataListener = handler
      return { dispose: () => { this.dataListener = null } }
    }
  }
  return { terminals, FakeTerminal, windowFocusTarget: new EventTarget() }
})
vi.mock('@xterm/xterm', () => ({ Terminal: FakeTerminal }))
vi.mock('../utils/fitTerminal', () => ({ fitTerminal: () => {} }))
vi.mock('../store/app', () => {
  const state = { windowFocusTarget }
  return { useAppStore: <T,>(selector: (s: typeof state) => T): T => selector(state) }
})

beforeEach(() => {
  terminals.length = 0
  ;(globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = class {
    observe(): void {}
    disconnect(): void {}
  }
})
afterEach(() => { cleanup() })

async function mount() {
  let onEvent: ((event: PtyEvent) => void) | null = null
  const write = vi.fn<(handle: string, data: string) => Promise<void>>().mockResolvedValue(undefined)
  const focus = vi.fn<(handle: string) => void>()
  const attach = vi.fn().mockResolvedValue({ success: true })
  const terminalApi = {
    onEvent: (_handle: string, callback: (event: PtyEvent) => void) => { onEvent = callback; return () => {} },
    attach,
    resize: vi.fn(),
    write,
    focus,
  } as unknown as TerminalApi
  render(<PtyViewer ptyId="pty1" connectionId="local" terminalApi={terminalApi} />)
  await act(async () => { await Promise.resolve() })
  const term = terminals[0]!
  const emit = (event: PtyEvent) => { act(() => { onEvent?.(event) }) }
  return { term, emit, write, focus, attach }
}

describe('PtyViewer input forwarding', () => {
  it('attaches as a terminal, so the daemon gates its writes on focus', async () => {
    const { attach } = await mount()
    expect(attach).toHaveBeenCalledWith('local', expect.any(String), 'pty1', PtyAttachKind.Terminal)
  })

  it('forwards keystrokes while its terminal holds keyboard focus', async () => {
    const { term, write } = await mount()
    term.element.focus()

    term.dataListener?.('q')

    expect(write).toHaveBeenCalledWith(expect.any(String), 'q')
  })

  it('claims the PTY input when its terminal takes focus, and stops once unmounted', async () => {
    const { term, focus } = await mount()

    term.element.focus()
    expect(focus).toHaveBeenCalledTimes(1)

    cleanup()
    windowFocusTarget.dispatchEvent(new Event('focus'))
    expect(focus).toHaveBeenCalledTimes(1)
  })

  it('drops query answers while focus is elsewhere — the terminal tab answers those', async () => {
    const { term, write } = await mount()

    term.dataListener?.('\x1b[?1;2c')

    expect(write).not.toHaveBeenCalled()
  })

  it('drops answers to replayed queries until the replay has been parsed', async () => {
    const { term, emit, write } = await mount()
    term.element.focus()

    emit({ type: PtyEventType.ReplayStart })
    emit({ type: PtyEventType.Data, data: new TextEncoder().encode('\x1b[c') })
    emit({ type: PtyEventType.ReplayEnd })
    term.dataListener?.('\x1b[?1;2c')
    expect(write).not.toHaveBeenCalled()

    term.parse()
    term.dataListener?.('q')
    expect(write).toHaveBeenCalledTimes(1)
    expect(write).toHaveBeenCalledWith(expect.any(String), 'q')
  })
})
