import { describe, it, expect } from 'vitest'
import { toStoredWorkspaceFile, parseWorkspaceFile, StoredWorkspaceFileSchema } from './workspaceFile'
import { makeWorkspace } from './test-fixtures/workspace'

describe('workspaceFile', () => {
  it('round-trips a workspace through serialize → parse, dropping parentHash', () => {
    const ws = makeWorkspace({
      id: 'ws-1',
      path: '/repo',
      name: 'feature',
      metadata: { displayName: 'Feature' },
      appStates: { 'tab-1': { applicationId: 'terminal', title: 'Terminal', state: { foo: 1 } } },
      activeTabId: 'tab-1',
    })
    const json = JSON.stringify(toStoredWorkspaceFile(ws, 'parent-sha'))

    const parsed = parseWorkspaceFile('ws-1', '/repo', json)

    expect(parsed).toEqual(ws)
    // parentHash is bookkeeping, not store state — it must not survive the read.
    expect('parentHash' in parsed).toBe(false)
  })

  it('embeds parentHash in the on-disk body', () => {
    const ws = makeWorkspace({ id: 'ws-1', path: '/repo' })
    const stored = toStoredWorkspaceFile(ws, 'abc123')
    expect(stored.parentHash).toBe('abc123')
    // id/path are ref-only and never persisted in the body.
    expect('id' in stored).toBe(false)
    expect('path' in stored).toBe(false)
  })

  it('rejects a body missing parentHash (fail loudly)', () => {
    const withoutParent: Partial<ReturnType<typeof toStoredWorkspaceFile>> = { ...toStoredWorkspaceFile(makeWorkspace(), '') }
    delete withoutParent.parentHash
    expect(() => StoredWorkspaceFileSchema.parse(withoutParent)).toThrow()
    expect(() => parseWorkspaceFile('ws-1', '/repo', JSON.stringify(withoutParent))).toThrow()
  })

  it('throws on invalid JSON', () => {
    expect(() => parseWorkspaceFile('ws-1', '/repo', '{ not json')).toThrow()
  })
})
