import React, { useState } from 'react'
import { Loader2, RefreshCw } from 'lucide-react'
import { TitleRefreshStatus } from '../store/createAnalyzerStore'
import type { WorkspaceStore } from '../types'

interface RefreshTitleButtonProps {
  workspace: WorkspaceStore
}

/** Re-runs the LLM auto-labeller over the AI Harness terminal, overwriting name + description. */
export function RefreshTitleButton({ workspace }: RefreshTitleButtonProps): React.JSX.Element {
  const [refreshing, setRefreshing] = useState(false)

  const handleRefresh = async () => {
    setRefreshing(true)
    try {
      const result = await workspace.getState().refreshTitleAndDescription()
      if (result.status === TitleRefreshStatus.Failure) {
        alert(result.error)
      }
    } catch (err) {
      // The store reports expected failures as a Failure result, so a throw is a bug —
      // still the user's problem, so show it rather than leaving it in the console.
      console.error('[RefreshTitleButton] refresh threw:', err)
      alert(err instanceof Error ? err.message : String(err))
    } finally {
      setRefreshing(false)
    }
  }

  const title = refreshing ? 'Generating name and description...' : 'Regenerate name and description with the LLM'

  return (
    <button
      className="workspace-edit-btn workspace-refresh-title-btn"
      onClick={() => { void handleRefresh() }}
      disabled={refreshing}
      title={title}
      aria-label={title}
    >
      {refreshing ? <Loader2 size={12} className="spinning" /> : <RefreshCw size={12} />}
    </button>
  )
}
