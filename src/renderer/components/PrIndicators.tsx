/* eslint-disable custom/no-string-literal-comparison -- TODO: migrate existing string-literal comparisons to enums */
import React from 'react'
import { Loader2, XCircle, CheckCircle2, AlertTriangle } from 'lucide-react'
import { useStore } from 'zustand'
import type { GitController } from '../store/createGitControllerStore'
import type { GitHubPrInfo } from '../types'

export enum PrSignal {
  None = 'none',
  MergeConflict = 'merge-conflict',
  CiFailure = 'ci-failure',
  CiRunning = 'ci-running',
  ReadyToMerge = 'ready-to-merge',
}

export const PR_STATE_CLASS: Record<GitHubPrInfo['state'], string> = {
  OPEN: 'tree-item-pr-number--open',
  CLOSED: 'tree-item-pr-number--closed',
  MERGED: 'tree-item-pr-number--merged',
}

const PR_SIGNAL_ICON: Record<PrSignal, () => React.JSX.Element | null> = {
  [PrSignal.None]: () => null,
  [PrSignal.MergeConflict]: () => <AlertTriangle size={12} className="tree-item-pr-signal tree-item-pr-signal--merge-conflict" />,
  [PrSignal.CiFailure]: () => <XCircle size={12} className="tree-item-pr-signal tree-item-pr-signal--ci-failure" />,
  [PrSignal.CiRunning]: () => <Loader2 size={12} className="tree-item-pr-signal tree-item-pr-signal--ci-running spinning" />,
  [PrSignal.ReadyToMerge]: () => <CheckCircle2 size={12} className="tree-item-pr-signal tree-item-pr-signal--ready" />,
}

export function getPrSignal(prInfo: GitHubPrInfo | null | undefined, hasConflictsWithParent: boolean): PrSignal {
  if (hasConflictsWithParent) return PrSignal.MergeConflict
  if (!prInfo) return PrSignal.None
  if (prInfo.checkRuns.some(c => c.status === 'COMPLETED' && c.conclusion === 'FAILURE')) {
    return PrSignal.CiFailure
  }
  if (prInfo.checkRuns.some(c => c.status !== 'COMPLETED')) {
    return PrSignal.CiRunning
  }
  if (prInfo.state !== 'OPEN') return PrSignal.None
  const hasApproval = prInfo.reviews.some(r => r.state === 'APPROVED')
  const hasChangesRequested = prInfo.reviews.some(r => r.state === 'CHANGES_REQUESTED')
  if (hasApproval && !hasChangesRequested && prInfo.unresolvedCount === 0) {
    return PrSignal.ReadyToMerge
  }
  return PrSignal.None
}

export function PrIndicators({ gitController }: { gitController: GitController }): React.JSX.Element {
  const prNumber = useStore(gitController, s => s.prInfo?.number)
  const prState = useStore(gitController, s => s.prInfo?.state)
  const prSignal = useStore(gitController, s => getPrSignal(s.prInfo, s.hasConflictsWithParent))
  return (
    <>
      {prNumber !== undefined && (
        <span className={`tree-item-pr-number ${prState ? PR_STATE_CLASS[prState] : ''}`}>{`#${String(prNumber)}`}</span>
      )}
      {PR_SIGNAL_ICON[prSignal]()}
    </>
  )
}
