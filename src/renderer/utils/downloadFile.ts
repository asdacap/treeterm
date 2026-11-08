/**
 * Saves bytes to the user's machine through the browser's download flow. Electron
 * shows a Save dialog for it, so the file lands locally even when the workspace
 * lives on a remote daemon.
 */
export function downloadBytes(fileName: string, bytes: Uint8Array<ArrayBuffer>, mimeType: string): void {
  const url = URL.createObjectURL(new Blob([bytes], { type: mimeType }))
  const anchor = document.createElement('a')
  anchor.href = url
  anchor.download = fileName
  anchor.click()
  // The navigation captured the blob when it started, so the URL can go right away.
  URL.revokeObjectURL(url)
}
