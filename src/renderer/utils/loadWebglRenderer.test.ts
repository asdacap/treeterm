import { describe, it, expect, vi, beforeEach } from 'vitest'
import type { Terminal } from '@xterm/xterm'

const { addons, FakeWebglAddon } = vi.hoisted(() => {
  const addons: FakeAddon[] = []
  class FakeAddon {
    disposed = false
    contextLossListener: (() => void) | null = null
    constructor() { addons.push(this) }
    onContextLoss(listener: () => void): void { this.contextLossListener = listener }
    dispose(): void { this.disposed = true }
  }
  return { addons, FakeWebglAddon: FakeAddon }
})

vi.mock('@xterm/addon-webgl', () => ({ WebglAddon: FakeWebglAddon }))

// vi.mock is hoisted above this import, so loadWebglRenderer sees the fake addon.
import { loadWebglRenderer, WebglFallbackReason, type WebglFallback } from './loadWebglRenderer'

/** Minimal Terminal stand-in — loadWebglRenderer only ever calls loadAddon. */
function fakeTerminal(loadAddon: (addon: unknown) => void): Terminal {
  return { loadAddon } as unknown as Terminal
}

describe('loadWebglRenderer', () => {
  beforeEach(() => { addons.length = 0 })

  it('loads the addon and reports no fallback when WebGL is available', () => {
    const loaded: unknown[] = []
    const onFallback = vi.fn<(fallback: WebglFallback) => void>()

    loadWebglRenderer(fakeTerminal((addon) => loaded.push(addon)), onFallback)

    expect(loaded).toHaveLength(1)
    expect(onFallback).not.toHaveBeenCalled()
    expect(addons[0]?.disposed).toBe(false)
  })

  it('reports Unavailable and drops the addon when activation throws', () => {
    const onFallback = vi.fn<(fallback: WebglFallback) => void>()
    const terminal = fakeTerminal(() => { throw new Error('WebGL2 not supported') })

    // Never throws: a terminal without a GPU renderer still works.
    expect(() => { loadWebglRenderer(terminal, onFallback) }).not.toThrow()

    expect(onFallback).toHaveBeenCalledTimes(1)
    const fallback = onFallback.mock.calls[0]?.[0]
    expect(fallback?.reason).toBe(WebglFallbackReason.Unavailable)
    expect(fallback?.reason === WebglFallbackReason.Unavailable && fallback.error.message)
      .toBe('WebGL2 not supported')
    // The half-built addon must not keep GPU resources or a live context-loss listener.
    expect(addons[0]?.disposed).toBe(true)
  })

  it('wraps a non-Error activation throw so callers always get an Error', () => {
    const onFallback = vi.fn<(fallback: WebglFallback) => void>()
    // The point of the test: a third-party addon can throw a non-Error, and callers must
    // still receive an Error.
    // eslint-disable-next-line @typescript-eslint/only-throw-error
    const terminal = fakeTerminal(() => { throw 'raw string failure' })

    loadWebglRenderer(terminal, onFallback)

    const fallback = onFallback.mock.calls[0]?.[0]
    expect(fallback?.reason === WebglFallbackReason.Unavailable && fallback.error).toBeInstanceOf(Error)
    expect(fallback?.reason === WebglFallbackReason.Unavailable && fallback.error.message)
      .toBe('raw string failure')
  })

  it('disposes the addon and reports ContextLost when the GPU drops the context', () => {
    const onFallback = vi.fn<(fallback: WebglFallback) => void>()

    loadWebglRenderer(fakeTerminal(() => {}), onFallback)
    expect(onFallback).not.toHaveBeenCalled()

    // Chromium evicts the oldest WebGL context once too many are live.
    addons[0]?.contextLossListener?.()

    expect(addons[0]?.disposed).toBe(true)
    expect(onFallback).toHaveBeenCalledTimes(1)
    expect(onFallback.mock.calls[0]?.[0].reason).toBe(WebglFallbackReason.ContextLost)
  })
})
