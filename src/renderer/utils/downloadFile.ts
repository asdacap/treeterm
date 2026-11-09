/**
 * Saves bytes to the user's machine through the browser's download flow. Electron
 * shows a Save dialog for it, so the file lands locally even when the workspace
 * lives on a remote daemon.
 *
 * The blob is typed as opaque bytes whatever the file is, so the browser never adjusts the
 * name to suit a MIME type (adding `.txt` to a `Makefile`, say): the copy keeps `fileName`.
 */
export function downloadBytes(fileName: string, bytes: Uint8Array<ArrayBuffer>): void {
  const url = URL.createObjectURL(new Blob([bytes], { type: 'application/octet-stream' }))
  const anchor = document.createElement('a')
  anchor.href = url
  anchor.download = fileName
  anchor.click()
  // The navigation captured the blob when it started, so the URL can go right away.
  URL.revokeObjectURL(url)
}
