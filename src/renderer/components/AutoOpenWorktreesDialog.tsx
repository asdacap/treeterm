import { useState, useEffect, useCallback } from 'react'
import { useStore } from 'zustand'
import type { WorktreeInfo, WorkspaceStore } from '../types'
import { useGitApi, useWorktreeRegistryApi } from '../hooks/useWorkspaceApis'
import type { WorktreeRegistryEntry } from '../lib/worktreeRegistry'
import { detectWorktreeHierarchy, type HierarchyNode } from '../lib/worktreeHierarchy'
import type { AutoOpenWorktreeItem } from '../store/createSessionStore'

interface AutoOpenWorktreesDialogProps {
  rootWorkspace: WorkspaceStore
  /** Worktree paths that are already open — excluded from the suggestion. */
  openWorktreePaths: string[]
  onConfirm: (items: AutoOpenWorktreeItem[]) => Promise<{ success: boolean; error?: string }>
  onCancel: () => void
}

/** A detected worktree plus its registry metadata (null when not previously known). */
interface DetectedRow {
  node: HierarchyNode
  entry: WorktreeRegistryEntry | null
}

export default function AutoOpenWorktreesDialog({
  rootWorkspace,
  openWorktreePaths,
  onConfirm,
  onCancel,
}: AutoOpenWorktreesDialogProps) {
  const rootWs = useStore(rootWorkspace, s => s.workspace)
  const git = useGitApi(rootWorkspace)
  const worktreeRegistry = useWorktreeRegistryApi(rootWorkspace)

  const [includeUnknown, setIncludeUnknown] = useState(false)
  const [rows, setRows] = useState<DetectedRow[]>([])
  const [checkedPaths, setCheckedPaths] = useState<Set<string>>(new Set())
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [isProcessing, setIsProcessing] = useState(false)

  const rootPath = rootWs.path
  const rootBranch = rootWs.gitBranch ?? ''

  // Detection re-runs whenever the include-unknown toggle changes. openWorktreePaths is a
  // fresh array every parent render, so it is intentionally not a dependency — the value is
  // captured at run time inside the async load.
  const openPathsKey = openWorktreePaths.join('\n')
  useEffect(() => {
    const openPaths = openPathsKey ? openPathsKey.split('\n') : []
    let cancelled = false
    const load = async (): Promise<void> => {
      setLoading(true)
      setError(null)
      try {
        const [worktrees, registry] = await Promise.all([
          git.listWorktrees(),
          worktreeRegistry.list(),
        ])

        const entriesByPath = new Map(registry.map(e => [e.path, e]))
        const excluded = new Set([rootPath, ...openPaths])

        // Candidates = live worktrees that are neither the root anchor nor already open.
        // Known = in the recent registry; unknown only included when the box is checked.
        const candidates: WorktreeInfo[] = worktrees.filter(wt => {
          if (excluded.has(wt.path)) return false
          if (entriesByPath.has(wt.path)) return true
          return includeUnknown
        })

        const nodes = await detectWorktreeHierarchy(
          candidates.map(wt => ({ path: wt.path, branch: wt.branch })),
          { path: rootPath, branch: rootBranch },
          (a, b) => git.isAncestor(a, b),
        )
        if (cancelled) return

        const detected: DetectedRow[] = nodes.map(node => ({
          node,
          entry: entriesByPath.get(node.path) ?? null,
        }))
        setRows(detected)
        setCheckedPaths(new Set(detected.map(r => r.node.path)))
      } catch (err) {
        if (cancelled) return
        setError(`Failed to detect worktrees: ${err instanceof Error ? err.message : String(err)}`)
        setRows([])
      } finally {
        if (!cancelled) setLoading(false)
      }
    }
    void load()
    return () => { cancelled = true }
  }, [git, worktreeRegistry, rootPath, rootBranch, includeUnknown, openPathsKey])

  const toggleChecked = useCallback((path: string) => {
    setCheckedPaths(prev => {
      const next = new Set(prev)
      if (next.has(path)) next.delete(path)
      else next.add(path)
      return next
    })
  }, [])

  const handleConfirm = async (): Promise<void> => {
    const nodeByPath = new Map(rows.map(r => [r.node.path, r.node]))
    const entryByPath = new Map(rows.map(r => [r.node.path, r.entry]))

    // If a node's detected parent is unchecked, attach it to the nearest checked ancestor
    // (or the root anchor) so the loaded tree stays connected.
    const resolveParent = (path: string): string | null => {
      let parent = nodeByPath.get(path)?.parentPath ?? null
      while (parent) {
        if (checkedPaths.has(parent)) return parent
        parent = nodeByPath.get(parent)?.parentPath ?? null
      }
      return null
    }

    const items: AutoOpenWorktreeItem[] = rows
      .filter(r => checkedPaths.has(r.node.path))
      .map(r => {
        const entry = entryByPath.get(r.node.path) ?? null
        return {
          path: r.node.path,
          branch: r.node.branch,
          name: entry?.displayName ?? r.node.branch,
          parentPath: resolveParent(r.node.path),
          displayName: entry?.displayName ?? undefined,
          description: entry?.description ?? undefined,
        }
      })

    setIsProcessing(true)
    setError(null)
    const result = await onConfirm(items)
    if (!result.success) {
      setError(result.error ?? 'Failed to open worktrees')
      setIsProcessing(false)
    }
  }

  const handleKeyDown = (e: React.KeyboardEvent) => {
    // eslint-disable-next-line custom/no-string-literal-comparison -- KeyboardEvent.key values are DOM constants, not domain enums
    if (e.key === 'Escape') onCancel()
  }

  // Build child lists keyed by parentPath (null = directly under the root anchor).
  const childrenByParent = new Map<string | null, DetectedRow[]>()
  for (const row of rows) {
    const key = row.node.parentPath
    const list = childrenByParent.get(key) ?? []
    list.push(row)
    childrenByParent.set(key, list)
  }

  const renderRows = (parentPath: string | null, depth: number): React.ReactNode[] => {
    const list = childrenByParent.get(parentPath) ?? []
    return list.flatMap(row => {
      const { node, entry } = row
      const displayName = entry?.displayName ?? node.branch
      return [
        <div
          key={node.path}
          className="create-child-worktree-item"
          style={{ paddingLeft: 12 + depth * 20 }}
          onClick={() => { if (!isProcessing) toggleChecked(node.path) }}
        >
          <input
            type="checkbox"
            checked={checkedPaths.has(node.path)}
            onChange={() => { toggleChecked(node.path) }}
            onClick={(e) => { e.stopPropagation() }}
            disabled={isProcessing}
            style={{ marginRight: 8, flexShrink: 0 }}
          />
          <span className="worktree-name">{displayName}</span>
          <span className="worktree-branch">{node.branch}</span>
          {!entry && <span className="worktree-badge">unknown</span>}
        </div>,
        ...renderRows(node.path, depth + 1),
      ]
    })
  }

  const checkedCount = checkedPaths.size

  return (
    <div className="dialog-overlay" onClick={onCancel}>
      <div className="create-child-dialog" onClick={(e) => { e.stopPropagation(); }} onKeyDown={handleKeyDown}>
        <div className="create-child-dialog-header">
          <h2>Auto Open All Recent Worktrees</h2>
          <button className="dialog-close" onClick={onCancel}>x</button>
        </div>

        <div className="create-child-dialog-content">
          <div className="create-child-dialog-info">
            <span className="create-child-label">Under:</span>
            <span className="create-child-value">{rootWs.name}</span>
          </div>

          <div className="create-child-existing-list">
            {loading ? (
              <div className="create-child-loading">Detecting worktrees...</div>
            ) : rows.length === 0 ? (
              <div className="create-child-empty">No recent worktrees found</div>
            ) : (
              renderRows(null, 0)
            )}
          </div>

          <div className="create-child-detached-checkbox">
            <label>
              <input
                type="checkbox"
                checked={includeUnknown}
                onChange={(e) => { setIncludeUnknown(e.target.checked); }}
                disabled={isProcessing}
              />
              <span>Include unknown worktrees (not in recent history)</span>
            </label>
          </div>

          {error && <div className="create-child-error">{error}</div>}
        </div>

        <div className="create-child-dialog-actions">
          <button className="dialog-btn cancel" onClick={onCancel} disabled={isProcessing}>
            Cancel
          </button>
          <button
            className="dialog-btn create"
            onClick={() => { void handleConfirm(); }}
            disabled={isProcessing || loading || checkedCount === 0}
          >
            {isProcessing ? 'Opening... Please wait' : `Ok (${String(checkedCount)})`}
          </button>
        </div>
      </div>
    </div>
  )
}
