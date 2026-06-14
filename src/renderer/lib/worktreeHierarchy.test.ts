import { describe, it, expect } from 'vitest'
import { detectWorktreeHierarchy, type WorktreeRef } from './worktreeHierarchy'

// ---------------------------------------------------------------------------
// Fake ancestry graph
// ---------------------------------------------------------------------------

interface Graph {
  /** Commit id each branch points at — equal ids mean equal tips (mutual ancestors). */
  tip: Record<string, string>
  /** Proper ancestors per branch (excludes self and equal-tip branches). */
  properAncestors: Record<string, string[]>
}

function makeIsAncestor(graph: Graph) {
  return (a: string, b: string): Promise<boolean> => {
    if (a === b) return Promise.resolve(true)
    const ta = graph.tip[a]
    const tb = graph.tip[b]
    if (ta !== undefined && tb !== undefined && ta === tb) return Promise.resolve(true)
    return Promise.resolve((graph.properAncestors[b] ?? []).includes(a))
  }
}

const wt = (branch: string): WorktreeRef => ({ path: `/wt/${branch}`, branch })
const root = wt('master')

describe('detectWorktreeHierarchy', () => {
  it('attaches a direct child under the root anchor (parentPath null)', async () => {
    const graph: Graph = {
      tip: { master: 'c0', feat: 'c1' },
      properAncestors: { feat: ['master'] },
    }
    const nodes = await detectWorktreeHierarchy([wt('feat')], root, makeIsAncestor(graph))
    expect(nodes).toEqual([{ path: '/wt/feat', branch: 'feat', parentPath: null }])
  })

  it('nests a grandchild under its nearest ancestor', async () => {
    const graph: Graph = {
      tip: { master: 'c0', feat: 'c1', sub: 'c2' },
      properAncestors: { feat: ['master'], sub: ['master', 'feat'] },
    }
    const nodes = await detectWorktreeHierarchy([wt('feat'), wt('sub')], root, makeIsAncestor(graph))
    const byPath = new Map(nodes.map((n) => [n.path, n]))
    expect(byPath.get('/wt/feat')?.parentPath).toBeNull()
    expect(byPath.get('/wt/sub')?.parentPath).toBe('/wt/feat')
  })

  it('places incomparable branches as siblings under the root', async () => {
    const graph: Graph = {
      tip: { master: 'c0', a: 'c1', b: 'c2' },
      properAncestors: { a: ['master'], b: ['master'] },
    }
    const nodes = await detectWorktreeHierarchy([wt('a'), wt('b')], root, makeIsAncestor(graph))
    expect(nodes.every((n) => n.parentPath === null)).toBe(true)
  })

  it('breaks equal-tip ties by attaching both to the root (no nesting, no cycle)', async () => {
    // a and b point at the same commit — they are mutual ancestors.
    const graph: Graph = {
      tip: { master: 'c0', a: 'c1', b: 'c1' },
      properAncestors: { a: ['master'], b: ['master'] },
    }
    const nodes = await detectWorktreeHierarchy([wt('a'), wt('b')], root, makeIsAncestor(graph))
    expect(nodes.map((n) => n.parentPath)).toEqual([null, null])
  })

  it('falls back to root for an unrelated (orphan) worktree', async () => {
    const graph: Graph = {
      tip: { master: 'c0', orphan: 'x9' },
      properAncestors: {}, // master is not an ancestor of orphan and vice-versa
    }
    const nodes = await detectWorktreeHierarchy([wt('orphan')], root, makeIsAncestor(graph))
    expect(nodes).toEqual([{ path: '/wt/orphan', branch: 'orphan', parentPath: null }])
  })

  it('chooses the deeper of two proper ancestors as the parent', async () => {
    // master -> mid -> leaf ; both master and mid are ancestors of leaf, mid is nearer.
    const graph: Graph = {
      tip: { master: 'c0', mid: 'c1', leaf: 'c2' },
      properAncestors: { mid: ['master'], leaf: ['master', 'mid'] },
    }
    const nodes = await detectWorktreeHierarchy([wt('leaf'), wt('mid')], root, makeIsAncestor(graph))
    const byPath = new Map(nodes.map((n) => [n.path, n]))
    expect(byPath.get('/wt/leaf')?.parentPath).toBe('/wt/mid')
    expect(byPath.get('/wt/mid')?.parentPath).toBeNull()
  })

  it('returns an empty list when there are no candidates', async () => {
    const graph: Graph = { tip: { master: 'c0' }, properAncestors: {} }
    const nodes = await detectWorktreeHierarchy([], root, makeIsAncestor(graph))
    expect(nodes).toEqual([])
  })
})
