import { describe, it, expect, vi } from 'vitest'
import type { WebContents, WebPreferences } from 'electron'
import { attachWebviewGuard, hardenWebviewPreferences, isAllowedWebviewUrl } from './webviewGuard'

describe('isAllowedWebviewUrl', () => {
  it.each([
    ['http://localhost:3000/', true],
    ['https://127.0.0.1:8443/app', true],
    ['http://example.com/', false],
    ['file:///etc/passwd', false],
    ['not a url', false],
  ])('%s -> %s', (url, allowed) => {
    expect(isAllowedWebviewUrl(url)).toBe(allowed)
  })
})

describe('hardenWebviewPreferences', () => {
  it('drops preload and forces isolation', () => {
    const prefs: WebPreferences = { preload: '/evil.js', nodeIntegration: true, contextIsolation: false }
    hardenWebviewPreferences(prefs)
    expect(prefs).toEqual({ nodeIntegration: false, nodeIntegrationInSubFrames: false, contextIsolation: true })
  })
})

describe('attachWebviewGuard', () => {
  type Handler = (...args: unknown[]) => void
  function setup() {
    const handlers = new Map<string, Handler>()
    const host = { on: vi.fn((name: string, cb: Handler) => { handlers.set(name, cb) }) } as unknown as WebContents
    const openExternal = vi.fn()
    attachWebviewGuard(host, openExternal)
    return { handlers, openExternal }
  }

  it('blocks webviews pointing outside localhost', () => {
    const { handlers } = setup()
    const allowed = { preventDefault: vi.fn() }
    handlers.get('will-attach-webview')!(allowed, {}, { src: 'http://localhost:1/' })
    expect(allowed.preventDefault).not.toHaveBeenCalled()
    const blocked = { preventDefault: vi.fn() }
    handlers.get('will-attach-webview')!(blocked, {}, { src: 'https://example.com' })
    expect(blocked.preventDefault).toHaveBeenCalled()
    const missing = { preventDefault: vi.fn() }
    handlers.get('will-attach-webview')!(missing, {}, {})
    expect(missing.preventDefault).toHaveBeenCalled()
  })

  it('sends guest popups to the external browser', () => {
    const { handlers, openExternal } = setup()
    let windowOpen: (d: { url: string }) => unknown = () => undefined
    const guest = { setWindowOpenHandler: vi.fn((cb: typeof windowOpen) => { windowOpen = cb }) }
    handlers.get('did-attach-webview')!({}, guest)
    expect(windowOpen({ url: 'https://docs.example' })).toEqual({ action: 'deny' })
    expect(openExternal).toHaveBeenCalledWith('https://docs.example')
  })
})
