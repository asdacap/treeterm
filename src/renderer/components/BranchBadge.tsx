import React, { useState } from 'react'
import { Loader2 } from 'lucide-react'
import ContextMenu from './ContextMenu'
import { useContextMenuStore } from '../store/contextMenu'
import { TitleRefreshStatus } from '../store/createAnalyzerStore'
import type { ClipboardApi, WorkspaceStore } from '../types'

const BRANCH_BADGE_MENU_ID = 'branch-badge'

interface BranchBadgeProps {
  branch: string
  worktreePath: string
  workspace: WorkspaceStore
  /** Only child worktrees own their branch; renaming a root repo's branch is off-limits. */
  canAutoRename: boolean
  clipboard: ClipboardApi
}

/** Workspace header branch pill: click copies, right-click offers path copy and LLM branch rename. */
export function BranchBadge({ branch, worktreePath, workspace, canAutoRename, clipboard }: BranchBadgeProps): React.JSX.Element {
  const openContextMenu = useContextMenuStore((s) => s.open)
  const closeContextMenu = useContextMenuStore((s) => s.close)
  const activeMenuId = useContextMenuStore((s) => s.activeMenuId)
  const menuPosition = useContextMenuStore((s) => s.position)
  const [copied, setCopied] = useState(false)
  const [renaming, setRenaming] = useState(false)

  const handleAutoRename = async () => {
    closeContextMenu()
    setRenaming(true)
    try {
      const result = await workspace.getState().refreshBranchName()
      if (result.status === TitleRefreshStatus.Failure) {
        alert(result.error)
      }
    } catch (err) {
      // The store reports expected failures as a Failure result, so a throw is a bug —
      // still the user's problem, so show it rather than leaving it in the console.
      console.error('[BranchBadge] auto rename threw:', err)
      alert(err instanceof Error ? err.message : String(err))
    } finally {
      setRenaming(false)
    }
  }

  const label = renaming
    ? <><Loader2 size={10} className="spinning" /> Renaming...</>
    : copied ? 'Copied!' : branch

  return (
    <>
      <span
        className={`workspace-branch${copied ? ' copied' : ''}`}
        onClick={() => {
          clipboard.writeText(branch)
          setCopied(true)
          setTimeout(() => { setCopied(false); }, 1500)
        }}
        onContextMenu={(e) => {
          e.preventDefault()
          e.stopPropagation()
          openContextMenu(BRANCH_BADGE_MENU_ID, e.clientX, e.clientY)
        }}
        title="Copy branch name"
      >{label}</span>
      <ContextMenu menuId={BRANCH_BADGE_MENU_ID} activeMenuId={activeMenuId} position={menuPosition}>
        <div className="context-menu-item" onClick={() => {
          closeContextMenu()
          clipboard.writeText(worktreePath)
        }}>
          Copy worktree path
        </div>
        {canAutoRename && !renaming && (
          <div className="context-menu-item" onClick={() => { void handleAutoRename() }}>
            Auto rename branch
          </div>
        )}
      </ContextMenu>
    </>
  )
}
