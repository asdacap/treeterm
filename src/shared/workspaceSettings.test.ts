import { describe, expect, it } from 'vitest'
import { makeWorkspace } from './test-fixtures/workspace'
import { getInheritedDefaultApplicationIds } from './workspaceSettings'

describe('getInheritedDefaultApplicationIds', () => {
  it('skips unset ancestors and returns the nearest setting first', () => {
    const a = makeWorkspace({ id: 'a', settings: { defaultApplicationId: 'app-a' } })
    const b = makeWorkspace({ id: 'b', parentId: 'a', settings: { defaultApplicationId: '' } })
    const c = makeWorkspace({ id: 'c', parentId: 'b', settings: { defaultApplicationId: 'app-c' } })
    const workspaces = new Map([a, b, c].map((workspace) => [workspace.id, workspace]))
    const lookup = (id: string) => workspaces.get(id)

    expect(getInheritedDefaultApplicationIds(b, lookup)).toEqual(['app-a'])
    expect(getInheritedDefaultApplicationIds(c, lookup)).toEqual(['app-c', 'app-a'])
  })

  it('returns nothing when no ancestor sets one or the parent is unknown', () => {
    const orphan = makeWorkspace({ id: 'orphan', parentId: 'missing', settings: { defaultApplicationId: '' } })
    expect(getInheritedDefaultApplicationIds(orphan, () => undefined)).toEqual([])
  })

  it('stops on a parent cycle', () => {
    const a = makeWorkspace({ id: 'a', parentId: 'b', settings: { defaultApplicationId: 'app-a' } })
    const b = makeWorkspace({ id: 'b', parentId: 'a', settings: { defaultApplicationId: 'app-b' } })
    const workspaces = new Map([a, b].map((workspace) => [workspace.id, workspace]))

    expect(getInheritedDefaultApplicationIds(a, (id) => workspaces.get(id))).toEqual(['app-a', 'app-b'])
  })
})
