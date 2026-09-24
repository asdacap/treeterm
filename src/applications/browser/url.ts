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

const HAS_SCHEME = /^[a-z][a-z0-9+.-]*:\/\//i

/**
 * Resolves what was typed into the address bar against the page the tab shows.
 * Only the path, query and hash may change: the port is fixed (on remote sessions it
 * is the local end of the tab's forward), so another origin is refused.
 */
export function resolveAddress(input: string, pageUrl: string): URL {
  const trimmed = input.trim()
  if (!trimmed) throw new Error('Enter a URL or path')
  const page = new URL(pageUrl)
  const relative = trimmed.startsWith('/') || trimmed.startsWith('?') || trimmed.startsWith('#')
  const absolute = HAS_SCHEME.test(trimmed) ? trimmed : `http://${trimmed}`
  const typed = relative ? new URL(trimmed, page) : parseLocalUrl(absolute)
  if (typed.protocol !== page.protocol || targetPort(typed) !== targetPort(page)) {
    throw new Error(`Only pages on ${page.origin} can be opened in this tab`)
  }
  const next = new URL(typed.href)
  next.hostname = page.hostname
  return next
}

const MARKDOWN_TYPES = new Set(['text/markdown', 'text/x-markdown'])
const MARKDOWN_PATH = /\.(md|markdown)$/i

/**
 * Whether a loaded page is markdown source to render rather than show as text.
 * Servers often send .md files as text/plain; HTML is never treated as markdown.
 */
export function isMarkdownPage(contentType: string, url: string): boolean {
  if (MARKDOWN_TYPES.has(contentType)) return true
  // eslint-disable-next-line custom/no-string-literal-comparison -- MIME type is a fixed external constant
  return contentType === 'text/plain' && MARKDOWN_PATH.test(new URL(url).pathname)
}
