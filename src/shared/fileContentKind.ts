const IMAGE_MIME_TYPES: Record<string, string> = {
  png: 'image/png',
  jpg: 'image/jpeg',
  jpeg: 'image/jpeg',
  gif: 'image/gif',
  webp: 'image/webp',
  bmp: 'image/bmp',
  ico: 'image/x-icon',
  avif: 'image/avif',
  svg: 'image/svg+xml',
}

function extensionOf(filePath: string): string {
  return filePath.split('.').pop()?.toLowerCase() ?? ''
}

/** Images are recognised by extension; their bytes travel base64-encoded. */
export function isImagePath(filePath: string): boolean {
  return extensionOf(filePath) in IMAGE_MIME_TYPES
}

/** Only meaningful for a path that passed `isImagePath`. */
export function imageMimeType(filePath: string): string {
  return IMAGE_MIME_TYPES[extensionOf(filePath)] ?? 'application/octet-stream'
}

// Git's own heuristic: a NUL within the first 8000 bytes marks the content binary.
const BINARY_SNIFF_LENGTH = 8000

export function isBinaryContent(content: string): boolean {
  return content.slice(0, BINARY_SNIFF_LENGTH).includes('\0')
}
