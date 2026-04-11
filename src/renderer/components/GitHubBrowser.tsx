import { useState } from 'react'
import { useStore } from 'zustand'
import { RefreshCw, Loader2, ExternalLink, CheckCircle2, XCircle, Clock, AlertCircle, MessageSquare } from 'lucide-react'
import { useAppStore } from '../store/app'
import type { WorkspaceStore } from '../types'
import type { GitHubPrInfo, GitHubReview, GitHubCheckRun, GitHubReviewThread } from '../types'

interface GitHubBrowserProps {
  workspace: WorkspaceStore
  isVisible: boolean
}

export default function GitHubBrowser({ workspace, isVisible }: GitHubBrowserProps) {
  const gitController = useStore(workspace, s => s.gitController)
  const prInfo = useStore(gitController, s => s.prInfo)
  const refreshPrStatus = useStore(gitController, s => s.refreshPrStatus)
  const openExternal = useAppStore((s) => s.openExternal)
  const [refreshing, setRefreshing] = useState(false)

  const handleRefresh = async () => {
    setRefreshing(true)
    try {
      await refreshPrStatus()
    } finally {
      setRefreshing(false)
    }
  }

  if (!isVisible) return null

  if (!prInfo) {
    return (
      <div className="github-browser">
        <div className="github-browser-empty">
          <p>No open pull request found for this branch.</p>
          <button className="workspace-action-btn" onClick={() => { void handleRefresh(); }} disabled={refreshing}>
            {refreshing ? <Loader2 size={14} className="spinning" /> : <RefreshCw size={14} />}
            <span style={{ marginLeft: 4 }}>Refresh</span>
          </button>
        </div>
      </div>
    )
  }

  return (
    <div className="github-browser">
      <div className="github-browser-header">
        <div className="github-browser-title-row">
          <h2 className="github-browser-title">
            #{String(prInfo.number)} {prInfo.title}
          </h2>
          <div className="github-browser-actions">
            <button className="workspace-action-btn" onClick={() => { void handleRefresh(); }} disabled={refreshing} title="Refresh">
              {refreshing ? <Loader2 size={14} className="spinning" /> : <RefreshCw size={14} />}
            </button>
            <button
              className="workspace-action-btn"
              onClick={() => { openExternal(prInfo.url) }}
              title="Open on GitHub"
            >
              <ExternalLink size={14} />
            </button>
          </div>
        </div>
        <PrStateBadge state={prInfo.state} />
      </div>

      <div className="github-browser-sections">
        <ReviewsSection reviews={prInfo.reviews} />
        <CheckRunsSection checkRuns={prInfo.checkRuns} />
        <UnresolvedThreadsSection threads={prInfo.unresolvedThreads} prUrl={prInfo.url} openExternal={openExternal} />
      </div>
    </div>
  )
}

function PrStateBadge({ state }: { state: GitHubPrInfo['state'] }) {
  const config = {
    OPEN: { label: 'Open', className: 'github-badge-open' },
    CLOSED: { label: 'Closed', className: 'github-badge-closed' },
    MERGED: { label: 'Merged', className: 'github-badge-merged' },
  }[state]

  return <span className={`github-badge ${config.className}`}>{config.label}</span>
}

function ReviewsSection({ reviews }: { reviews: GitHubReview[] }) {
  if (reviews.length === 0) return null

  return (
    <div className="github-section">
      <h3 className="github-section-title">Reviews</h3>
      <div className="github-section-list">
        {reviews.map((review) => (
          <div key={`${review.author}-${review.state}`} className="github-review-item">
            <ReviewStateIcon state={review.state} />
            <span className="github-review-author">{review.author}</span>
            <span className="github-review-state">{formatReviewState(review.state)}</span>
          </div>
        ))}
      </div>
    </div>
  )
}

function ReviewStateIcon({ state }: { state: GitHubReview['state'] }) {
  switch (state) {
    case 'APPROVED': return <CheckCircle2 size={14} className="github-icon-approved" />
    case 'CHANGES_REQUESTED': return <XCircle size={14} className="github-icon-changes" />
    case 'COMMENTED': return <MessageSquare size={14} className="github-icon-commented" />
    case 'PENDING': return <Clock size={14} className="github-icon-pending" />
    case 'DISMISSED': return <AlertCircle size={14} className="github-icon-dismissed" />
  }
}

function formatReviewState(state: GitHubReview['state']): string {
  return {
    APPROVED: 'Approved',
    CHANGES_REQUESTED: 'Changes requested',
    COMMENTED: 'Commented',
    PENDING: 'Pending',
    DISMISSED: 'Dismissed',
  }[state]
}

function CheckRunsSection({ checkRuns }: { checkRuns: GitHubCheckRun[] }) {
  if (checkRuns.length === 0) return null

  return (
    <div className="github-section">
      <h3 className="github-section-title">Checks</h3>
      <div className="github-section-list">
        {checkRuns.map((check) => (
          <div key={check.name} className="github-check-item">
            <CheckRunIcon check={check} />
            <span className="github-check-name">{check.name}</span>
          </div>
        ))}
      </div>
    </div>
  )
}

function CheckRunIcon({ check }: { check: GitHubCheckRun }) {
  if (check.status !== 'COMPLETED') {
    return <Loader2 size={14} className="spinning github-icon-pending" />
  }
  switch (check.conclusion) {
    case 'SUCCESS': return <CheckCircle2 size={14} className="github-icon-approved" />
    case 'FAILURE': return <XCircle size={14} className="github-icon-changes" />
    case 'NEUTRAL':
    case 'SKIPPED': return <AlertCircle size={14} className="github-icon-dismissed" />
    default: return <AlertCircle size={14} className="github-icon-pending" />
  }
}

function UnresolvedThreadsSection({ threads, prUrl, openExternal }: { threads: GitHubReviewThread[]; prUrl: string; openExternal: (url: string) => void }) {
  if (threads.length === 0) {
    return (
      <div className="github-section">
        <h3 className="github-section-title">Unresolved Comments</h3>
        <p className="github-section-empty">No unresolved comments</p>
      </div>
    )
  }

  return (
    <div className="github-section">
      <h3 className="github-section-title">Unresolved Comments ({threads.length})</h3>
      <div className="github-section-list">
        {threads.map((thread) => (
          <div key={`${thread.path}:${String(thread.line ?? 'general')}-${thread.author}`} className="github-thread-item" onClick={() => { openExternal(prUrl) }}>
            <div className="github-thread-header">
              <span className="github-thread-path">{thread.path}{thread.line ? `:${String(thread.line)}` : ''}</span>
              <span className="github-thread-author">{thread.author}</span>
            </div>
            <div className="github-thread-body">{thread.body}</div>
          </div>
        ))}
      </div>
    </div>
  )
}
