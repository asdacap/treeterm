// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor, fireEvent } from '@testing-library/react'
import { createStore } from 'zustand/vanilla'
import AutoOpenWorktreesDialog from './AutoOpenWorktreesDialog'
import type { WorkspaceStoreState } from '../store/createWorkspaceStore'
import type { WorkspaceStore } from '../types'
import { makeWorkspace } from '../../shared/test-fixtures/workspace'
import { createMockGitApi, createMockWorktreeRegistryApi } from '../../shared/mockApis'
import type { WorktreeRegistryEntry } from '../lib/worktreeRegistry'

function makeRootStore() {
  const gitApi = createMockGitApi()
  const registry = createMockWorktreeRegistryApi()
  const store = createStore<WorkspaceStoreState>()(() => ({
    workspace: makeWorkspace({ id: 'ws-root', name: 'repo', path: '/repo', isGitRepo: true, gitRootPath: '/repo', gitBranch: 'master' }),
    gitApi,
    worktreeRegistryApi: registry,
  } as WorkspaceStoreState))
  return { store: store as unknown as WorkspaceStore, gitApi, registry }
}

const entry = (path: string, displayName: string): WorktreeRegistryEntry =>
  ({ path, branch: '', displayName, description: null, lastUsedAt: 1 })

const featEntry = entry('/repo/.worktrees/feat', 'Feature')
const subEntry = entry('/repo/.worktrees/sub', 'Sub')

// master -> feat -> sub ; unk is unrelated/unknown
function setupGraph(gitApi: ReturnType<typeof createMockGitApi>) {
  vi.mocked(gitApi.listWorktrees).mockResolvedValue([
    { path: '/repo', branch: 'master' },
    { path: '/repo/.worktrees/feat', branch: 'feat' },
    { path: '/repo/.worktrees/sub', branch: 'sub' },
    { path: '/repo/.worktrees/unk', branch: 'unk' },
  ])
  const proper: Record<string, string[]> = {
    feat: ['master'],
    sub: ['master', 'feat'],
    // unk has no ancestors in the set
  }
  vi.mocked(gitApi.isAncestor).mockImplementation((a: string, b: string) =>
    Promise.resolve(a === b || (proper[b] ?? []).includes(a)))
}

describe('AutoOpenWorktreesDialog', () => {
  beforeEach(() => { vi.clearAllMocks() })

  it('shows known worktrees, excluding the root and already-open paths', async () => {
    const { store, gitApi, registry } = makeRootStore()
    setupGraph(gitApi)
    vi.mocked(registry.list).mockResolvedValue([featEntry, subEntry])

    render(
      <AutoOpenWorktreesDialog
        rootWorkspace={store}
        openWorktreePaths={['/repo/.worktrees/sub']}
        onConfirm={vi.fn().mockResolvedValue({ success: true })}
        onCancel={vi.fn()}
      />
    )

    await waitFor(() => { expect(screen.getByText('feat')).toBeTruthy() })
    // sub is already open → excluded; master is the root → excluded
    expect(screen.queryByText('sub')).toBeNull()
    expect(screen.queryByText('master')).toBeNull()
  })

  it('toggling "Include unknown worktrees" reveals worktrees not in the registry', async () => {
    const { store, gitApi, registry } = makeRootStore()
    setupGraph(gitApi)
    vi.mocked(registry.list).mockResolvedValue([featEntry, subEntry])

    render(
      <AutoOpenWorktreesDialog
        rootWorkspace={store}
        openWorktreePaths={[]}
        onConfirm={vi.fn().mockResolvedValue({ success: true })}
        onCancel={vi.fn()}
      />
    )

    await waitFor(() => { expect(screen.getByText('feat')).toBeTruthy() })
    expect(screen.queryAllByText('unk')).toEqual([])

    fireEvent.click(screen.getByLabelText(/Include unknown worktrees/i))
    await waitFor(() => { expect(screen.getAllByText('unk').length).toBeGreaterThan(0) })
  })

  it('Ok emits items with the detected hierarchy', async () => {
    const { store, gitApi, registry } = makeRootStore()
    setupGraph(gitApi)
    vi.mocked(registry.list).mockResolvedValue([featEntry, subEntry])
    const onConfirm = vi.fn().mockResolvedValue({ success: true })

    render(
      <AutoOpenWorktreesDialog
        rootWorkspace={store}
        openWorktreePaths={[]}
        onConfirm={onConfirm}
        onCancel={vi.fn()}
      />
    )

    await waitFor(() => { expect(screen.getByText('feat')).toBeTruthy() })
    fireEvent.click(screen.getByText(/^Ok \(/))

    await waitFor(() => { expect(onConfirm).toHaveBeenCalled() })
    const items = onConfirm.mock.calls[0]![0] as { path: string; parentPath: string | null }[]
    const byPath = new Map(items.map(i => [i.path, i]))
    expect(byPath.get('/repo/.worktrees/feat')?.parentPath).toBeNull()
    expect(byPath.get('/repo/.worktrees/sub')?.parentPath).toBe('/repo/.worktrees/feat')
  })

  it('unchecking a parent reparents its child to the nearest checked ancestor (root)', async () => {
    const { store, gitApi, registry } = makeRootStore()
    setupGraph(gitApi)
    vi.mocked(registry.list).mockResolvedValue([featEntry, subEntry])
    const onConfirm = vi.fn().mockResolvedValue({ success: true })

    render(
      <AutoOpenWorktreesDialog
        rootWorkspace={store}
        openWorktreePaths={[]}
        onConfirm={onConfirm}
        onCancel={vi.fn()}
      />
    )

    await waitFor(() => { expect(screen.getByText('feat')).toBeTruthy() })
    // Uncheck the feat row (its checkbox is the first checkbox in the list)
    const featRow = screen.getByText('feat').closest('.create-child-worktree-item')!
    const featCheckbox = featRow.querySelector('input[type="checkbox"]')!
    fireEvent.click(featCheckbox)

    fireEvent.click(screen.getByText(/^Ok \(/))
    await waitFor(() => { expect(onConfirm).toHaveBeenCalled() })
    const items = onConfirm.mock.calls[0]![0] as { path: string; parentPath: string | null }[]
    // feat excluded; sub's parent (feat) is unchecked → falls back to root (null)
    expect(items.map(i => i.path)).toEqual(['/repo/.worktrees/sub'])
    expect(items[0]!.parentPath).toBeNull()
  })
})
