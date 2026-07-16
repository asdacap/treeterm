import type { Workspace } from './workspaceFile'

export function normalizeFavouritePath(path: string): string {
  let normalized: string = path
  if (normalized.startsWith('./')) normalized = normalized.slice(2)
  while (normalized.startsWith('/')) normalized = normalized.slice(1)
  while (normalized.endsWith('/')) normalized = normalized.slice(0, -1)
  return normalized
}

/** FileEntry.relativePath uses the daemon host's native separator. Persisted
 * favourite paths use `/` so they remain portable across local/remote hosts. */
export function normalizeFileEntryRelativePath(path: string, workspacePath: string): string {
  const isWindowsWorkspace = /^[A-Za-z]:[\\/]/.test(workspacePath)
    || workspacePath.startsWith('\\\\')
    || workspacePath.startsWith('//')
  const portablePath = isWindowsWorkspace ? path.split('\\').join('/') : path
  return normalizeFavouritePath(portablePath)
}

export function getWorkspaceFavouritePaths(
  workspace: Workspace,
  lookupWorkspace: (id: string) => Workspace | undefined
): string[] {
  const paths: string[] = []
  const seenPaths = new Set<string>()
  const seenWorkspaces = new Set<string>()
  let current: Workspace | undefined = workspace

  while (current && !seenWorkspaces.has(current.id)) {
    seenWorkspaces.add(current.id)
    for (const rawPath of current.favouritePaths) {
      const path = normalizeFavouritePath(rawPath)
      if (path && !seenPaths.has(path)) {
        seenPaths.add(path)
        paths.push(path)
      }
    }
    current = current.parentId ? lookupWorkspace(current.parentId) : undefined
  }

  return paths
}

export function isFavouritePath(path: string, favouritePaths: string[]): boolean {
  const normalized = normalizeFavouritePath(path)
  return favouritePaths.some((favouritePath) =>
    normalized === favouritePath || normalized.startsWith(`${favouritePath}/`)
  )
}
