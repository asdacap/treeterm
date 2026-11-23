import type { FileEntry } from '../types'

/**
 * The entries whose name does not start with '.'. The daemon lists every entry of a
 * directory; Electron's views never show the hidden ones.
 */
export function withoutHiddenEntries(entries: FileEntry[]): FileEntry[] {
  return entries.filter((entry) => !entry.name.startsWith('.'))
}
