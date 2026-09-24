const LOCAL_HOSTS = new Set(['localhost', '127.0.0.1'])

/** The embedded browser only ever shows localhost pages (see main/webviewGuard.ts). */
export function parseLocalUrl(url: string): URL {
  const parsed = URL.canParse(url) ? new URL(url) : null
  // eslint-disable-next-line custom/no-string-literal-comparison -- URL protocol is a fixed external constant
  if (!parsed || (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') || !LOCAL_HOSTS.has(parsed.hostname)) {
    throw new Error(`URL must be http(s)://localhost or 127.0.0.1: ${url}`)
  }
  return parsed
}

export function targetPort(url: URL): number {
  if (url.port) return Number(url.port)
  // eslint-disable-next-line custom/no-string-literal-comparison -- URL protocol is a fixed external constant
  return url.protocol === 'https:' ? 443 : 80
}

/** Same path and query, served through the local end of the forward. */
export function forwardedUrl(url: URL, localPort: number): string {
  const next = new URL(url.href)
  next.hostname = 'localhost'
  next.port = String(localPort)
  return next.href
}
