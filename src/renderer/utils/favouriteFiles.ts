import type { FileEntry, WorkspaceFilesystemApi } from '../types'
import { normalizeFavouritePath, normalizeFileEntryRelativePath } from '../../shared/workspaceFavourites'

export interface FavouriteFile {
  path: string
  relativePath: string
}

export async function resolveFavouriteFiles(
  workspacePath: string,
  favouritePaths: string[],
  filesystem: WorkspaceFilesystemApi,
  isCancelled: () => boolean = () => false
): Promise<FavouriteFile[]> {
  const files = new Map<string, FavouriteFile>()
  const visitedDirectories = new Set<string>()

  const visitEntry = async (entry: FileEntry): Promise<void> => {
    if (isCancelled()) return
    if (!entry.isDirectory) {
      const relativePath = normalizeFileEntryRelativePath(entry.relativePath, workspacePath)
      files.set(relativePath, { path: entry.path, relativePath })
      return
    }
    if (visitedDirectories.has(entry.path)) return
    visitedDirectories.add(entry.path)
    const result = await filesystem.readDirectory(entry.path)
    if (isCancelled()) return
    if (!result.success) throw new Error(result.error || `Failed to load ${entry.relativePath}`)
    // Deliberately sequential: a favourite may cover a very large directory tree,
    // and flooding a local or remote daemon with one RPC per directory can starve
    // other renderer work. Cancellation stops scheduling after the in-flight read.
    for (const child of result.contents.entries) {
      await visitEntry(child)
      if (isCancelled()) return
    }
  }

  const resolveEntry = async (relativePath: string): Promise<FileEntry | undefined> => {
    const parts = relativePath.split('/')
    let directoryPath = workspacePath
    for (let index = 0; index < parts.length; index++) {
      if (isCancelled()) return undefined
      const result = await filesystem.readDirectory(directoryPath)
      if (isCancelled()) return undefined
      if (!result.success) throw new Error(result.error || `Failed to load ${relativePath}`)
      const name = parts[index]
      const entry = result.contents.entries.find((candidate) => candidate.name === name)
      if (!entry) return undefined
      if (index === parts.length - 1) return entry
      if (!entry.isDirectory) return undefined
      directoryPath = entry.path
    }
    return undefined
  }

  for (const rawPath of favouritePaths) {
    if (isCancelled()) break
    const relativePath = normalizeFavouritePath(rawPath)
    if (!relativePath) continue
    const entry = await resolveEntry(relativePath)
    // A favourite can legitimately be absent in a child branch. Keep loading the
    // remaining inherited favourites instead of inventing a file entry for it.
    if (entry) await visitEntry(entry)
  }

  return Array.from(files.values()).sort((a, b) => a.relativePath.localeCompare(b.relativePath))
}
