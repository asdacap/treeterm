/**
 * Worktree hierarchy detection.
 *
 * Given a set of candidate worktrees and a root anchor worktree (the workspace the user
 * right-clicked), infer a parent/child forest from git ancestry. A worktree's parent is its
 * nearest proper ancestor among the candidate set (or the root anchor when no candidate is a
 * proper ancestor). Branches with identical tips become siblings rather than nesting, which
 * keeps the result acyclic.
 */

export interface WorktreeRef {
  path: string
  branch: string
}

export interface HierarchyNode {
  path: string
  branch: string
  /** Parent worktree path, or `null` to attach directly under the root anchor. */
  parentPath: string | null
}

/**
 * @param candidates Worktrees to place in the hierarchy (excludes the root anchor).
 * @param root The anchor worktree; detected top-level candidates become its children.
 * @param isAncestor Predicate: is `ancestorRef` an ancestor of `descendantRef`? (git merge-base --is-ancestor)
 */
export async function detectWorktreeHierarchy(
  candidates: WorktreeRef[],
  root: WorktreeRef,
  isAncestor: (ancestorRef: string, descendantRef: string) => Promise<boolean>,
): Promise<HierarchyNode[]> {
  // Cache ancestry queries — pairwise detection asks the same (a, b) repeatedly.
  const cache = new Map<string, Promise<boolean>>()
  const anc = (a: string, b: string): Promise<boolean> => {
    if (a === b) return Promise.resolve(true)
    const key = `${a}\t${b}`
    let p = cache.get(key)
    if (!p) {
      p = isAncestor(a, b)
      cache.set(key, p)
    }
    return p
  }

  // Universe = root anchor + candidates. Branches are unique per worktree (git forbids the
  // same branch checked out in two worktrees), so paths uniquely identify members.
  const universe: WorktreeRef[] = [root, ...candidates]

  // m is a *proper* ancestor of w: ancestor of w, but w is not an ancestor of m (excludes
  // equal-tip pairs, which would otherwise be mutual ancestors and create cycles).
  const properAncestor = async (m: WorktreeRef, w: WorktreeRef): Promise<boolean> => {
    if (m.path === w.path) return false
    const [mw, wm] = await Promise.all([anc(m.branch, w.branch), anc(w.branch, m.branch)])
    return mw && !wm
  }

  // rank(w) = number of proper ancestors of w within the universe. The root anchor ranks
  // lowest; a deeper fork ranks higher. A proper ancestor always has a strictly lower rank,
  // so picking the highest-ranked proper ancestor as parent yields an acyclic forest.
  const rank = new Map<string, number>()
  for (const w of universe) {
    let r = 0
    for (const m of universe) {
      if (await properAncestor(m, w)) r++
    }
    rank.set(w.path, r)
  }

  const nodes: HierarchyNode[] = []
  for (const w of candidates) {
    const ancestors: WorktreeRef[] = []
    for (const m of universe) {
      if (await properAncestor(m, w)) ancestors.push(m)
    }

    if (ancestors.length === 0) {
      // No candidate (or root) is a proper ancestor — attach under the root anchor.
      nodes.push({ path: w.path, branch: w.branch, parentPath: null })
      continue
    }

    const maxRank = Math.max(...ancestors.map((a) => rank.get(a.path) ?? 0))
    const nearest = ancestors.filter((a) => (rank.get(a.path) ?? 0) === maxRank)
    // Tie-break among equally-near ancestors: prefer the root anchor, else smallest path.
    const sorted = nearest.slice().sort((a, b) => (a.path < b.path ? -1 : a.path > b.path ? 1 : 0))
    const chosen = nearest.find((a) => a.path === root.path) ?? sorted[0] ?? root

    nodes.push({
      path: w.path,
      branch: w.branch,
      parentPath: chosen.path === root.path ? null : chosen.path,
    })
  }

  return nodes
}
