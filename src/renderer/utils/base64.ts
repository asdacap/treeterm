/** Decodes base64 to raw bytes. Throws on malformed input. */
export function base64ToBytes(base64: string): Uint8Array<ArrayBuffer> {
  const binary = atob(base64)
  const bytes = new Uint8Array(binary.length)
  for (let i = 0; i < binary.length; i++) {
    bytes[i] = binary.charCodeAt(i)
  }
  return bytes
}

/**
 * Decodes base64 to the text a UTF-8 read of the same bytes would have produced:
 * a leading BOM is kept and invalid sequences become U+FFFD, as Node's `toString('utf-8')` does.
 */
export function decodeBase64Utf8(base64: string): string {
  return new TextDecoder('utf-8', { ignoreBOM: true }).decode(base64ToBytes(base64))
}
