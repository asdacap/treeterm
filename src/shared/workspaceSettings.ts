import type { Workspace } from './workspaceFile'

/** Default application ids set on a workspace and its ancestors, nearest first.
 * Resolved at use time so a change on any ancestor reaches every descendant
 * that has not overridden it. */
export function getInheritedDefaultApplicationIds(
  workspace: Workspace,
  lookupWorkspace: (id: string) => Workspace | undefined
): string[] {
  const ids: string[] = []
  const seenWorkspaces = new Set<string>()
  let current: Workspace | undefined = workspace

  while (current && !seenWorkspaces.has(current.id)) {
    seenWorkspaces.add(current.id)
    if (current.settings.defaultApplicationId) ids.push(current.settings.defaultApplicationId)
    current = current.parentId ? lookupWorkspace(current.parentId) : undefined
  }

  return ids
}
