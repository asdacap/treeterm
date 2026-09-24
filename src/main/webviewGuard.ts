/**
 * Guards the <webview> tags the WebApp application embeds. They only ever show a
 * dev server on localhost, so anything else is refused, and the guest never gets
 * node integration or our preload.
 */

import type { WebContents, WebPreferences } from 'electron'

const ALLOWED_HOSTS = new Set(['localhost', '127.0.0.1'])

export function isAllowedWebviewUrl(url: string): boolean {
  if (!URL.canParse(url)) return false
  const parsed = new URL(url)
  // eslint-disable-next-line custom/no-string-literal-comparison -- URL protocol is a fixed external constant
  return (parsed.protocol === 'http:' || parsed.protocol === 'https:') && ALLOWED_HOSTS.has(parsed.hostname)
}

export function hardenWebviewPreferences(webPreferences: WebPreferences): void {
  delete webPreferences.preload
  webPreferences.nodeIntegration = false
  webPreferences.nodeIntegrationInSubFrames = false
  webPreferences.contextIsolation = true
}

/** Installs the guard on a host window's WebContents. */
export function attachWebviewGuard(host: WebContents, openExternal: (url: string) => void): void {
  host.on('will-attach-webview', (event, webPreferences, params) => {
    hardenWebviewPreferences(webPreferences)
    if (params.src === undefined || !isAllowedWebviewUrl(params.src)) event.preventDefault()
  })
  host.on('did-attach-webview', (_event, guest) => {
    // Popups (target=_blank, window.open) leave the app for the system browser.
    guest.setWindowOpenHandler(({ url }) => {
      openExternal(url)
      return { action: 'deny' }
    })
  })
}
