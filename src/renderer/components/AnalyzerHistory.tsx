import { useState } from 'react'
import { useStore } from 'zustand'
import type { ApplicationRenderProps } from '../types'
import type { AnalyzerHistoryEntry } from '../store/createAnalyzerStore'
import type { AiHarnessRef } from '../../applications/aiHarness/renderer'
import {
  ActivityTransitionKind,
  NO_TRANSITIONS,
  useActivityStateStore,
  type ActivityTransition,
  type ActivityTransitionDetail,
} from '../store/activityState'

const KIND_COLORS: Record<string, string> = {
  analyzer: '#1a5276',
  title: '#6a0dad'
}

const TRANSITION_KIND_LABELS: Record<ActivityTransitionKind, string> = {
  [ActivityTransitionKind.ViewportChanged]: 'viewport changed',
  [ActivityTransitionKind.ViewportIdle]: 'viewport idle',
  [ActivityTransitionKind.Classification]: 'classification',
  [ActivityTransitionKind.Debugger]: 'debugger',
}

/** The kind-specific extra shown after the kind pill. */
function transitionExtra(detail: ActivityTransitionDetail): string {
  switch (detail.kind) {
    case ActivityTransitionKind.ViewportIdle: return `after ${String(detail.idleTimeoutMs)}ms`
    case ActivityTransitionKind.Classification: return detail.reason
    case ActivityTransitionKind.ViewportChanged:
    case ActivityTransitionKind.Debugger:
      return ''
  }
}

interface AnalyzerHistoryState {
  sourceTabId: string
}

function isAnalyzerHistoryState(state: unknown): state is AnalyzerHistoryState {
  return typeof state === 'object' && state !== null && 'sourceTabId' in state
}

function formatTime(timestamp: number): string {
  const d = new Date(timestamp)
  return d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' })
}

function bufferPreview(text: string): string {
  const lines = text.split('\n')
  const preview = lines.slice(0, 3).join('\n')
  return lines.length > 3 ? preview + '\n...' : preview
}

const pillStyle = (background: string): React.CSSProperties => ({
  background,
  color: '#fff',
  padding: '1px 6px',
  borderRadius: 3,
  fontSize: 11,
  fontWeight: 500,
})

/**
 * Debug view for a tab's activity: the transition log every tab records, plus the analyzer's
 * LLM calls when the source tab is an AI harness.
 */
export default function AnalyzerHistory({ tab, workspace }: ApplicationRenderProps) {
  const getTabRef = useStore(workspace, s => s.getTabRef)
  const state = tab.state
  if (!isAnalyzerHistoryState(state)) {
    return <div style={{ padding: 16, color: '#f14c4c' }}>Invalid state: missing sourceTabId</div>
  }

  const ref = getTabRef(state.sourceTabId)
  const analyzer = ref && 'analyzer' in ref ? (ref as AiHarnessRef).analyzer : null

  return (
    <div style={{ padding: 16, display: 'flex', flexDirection: 'column', gap: 24, height: '100%', overflow: 'auto' }}>
      <TransitionList sourceTabId={state.sourceTabId} workspace={workspace} />
      {analyzer && <AnalyzerHistoryContent analyzer={analyzer} workspace={workspace} />}
    </div>
  )
}

function TransitionList({ sourceTabId, workspace }: { sourceTabId: string; workspace: ApplicationRenderProps['workspace'] }) {
  const transitions = useActivityStateStore((s) => s.transitions[sourceTabId] ?? NO_TRANSITIONS)
  const [expanded, setExpanded] = useState(new Set<number>())

  const toggle = (index: number) => {
    setExpanded(prev => {
      const next = new Set(prev)
      if (next.has(index)) next.delete(index)
      else next.add(index)
      return next
    })
  }

  const handleDebug = (transition: ActivityTransition) => {
    workspace.getState().addTab('system-prompt-debugger', { bufferText: transition.detail.snapshot })
  }

  const reversed = [...transitions].reverse()

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
        <h3 style={{ margin: 0, color: '#ccc' }}>Activity Transitions</h3>
        <span style={{ color: '#888', fontSize: 12 }}>{transitions.length} entries</span>
      </div>
      {reversed.length === 0 ? (
        <div style={{ color: '#888', fontSize: 13, padding: 8 }}>No transitions yet.</div>
      ) : (
        <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
          {reversed.map((transition, i) => {
            const index = transitions.length - 1 - i
            const extra = transitionExtra(transition.detail)
            return (
              <div
                key={index}
                style={{
                  background: '#1e1e1e',
                  border: '1px solid #333',
                  borderRadius: 4,
                  padding: '8px 12px',
                  display: 'flex',
                  gap: 12,
                  alignItems: 'flex-start'
                }}
              >
                <div style={{ display: 'flex', flexDirection: 'column', gap: 4, minWidth: 0, flex: 1 }}>
                  <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                    <span style={{ color: '#888', fontSize: 11, fontFamily: 'monospace' }}>
                      {formatTime(transition.timestamp)}
                    </span>
                    <span style={{ color: '#ccc', fontSize: 11, fontFamily: 'monospace' }}>
                      {transition.from} → {transition.to}
                    </span>
                    <span style={pillStyle('#555')}>{TRANSITION_KIND_LABELS[transition.detail.kind]}</span>
                    {extra && <span style={{ color: '#888', fontSize: 11 }}>{extra}</span>}
                  </div>
                  <pre
                    onClick={() => { toggle(index); }}
                    style={{
                      margin: 0,
                      color: '#666',
                      fontSize: 11,
                      fontFamily: 'monospace',
                      whiteSpace: 'pre-wrap',
                      overflow: 'hidden',
                      textOverflow: 'ellipsis',
                      cursor: 'pointer'
                    }}
                  >
                    {expanded.has(index) ? transition.detail.snapshot : bufferPreview(transition.detail.snapshot)}
                  </pre>
                </div>
                <button
                  onClick={() => { handleDebug(transition); }}
                  style={{
                    padding: '4px 8px',
                    background: '#333',
                    color: '#ccc',
                    border: '1px solid #555',
                    borderRadius: 4,
                    cursor: 'pointer',
                    fontSize: 11,
                    whiteSpace: 'nowrap',
                    flexShrink: 0
                  }}
                >
                  Debug
                </button>
              </div>
            )
          })}
        </div>
      )}
    </div>
  )
}

function AnalyzerHistoryContent({ analyzer, workspace }: { analyzer: AiHarnessRef['analyzer']; workspace: ApplicationRenderProps['workspace'] }) {
  const [entries, setEntries] = useState<AnalyzerHistoryEntry[]>(() => analyzer.getState().getHistory())
  const [expandedEntries, setExpandedEntries] = useState(new Set())
  const [expandedResponses, setExpandedResponses] = useState(new Set())
  const [expandedPrompts, setExpandedPrompts] = useState(new Set())

  const handleRefresh = () => {
    setEntries(analyzer.getState().getHistory())
  }

  const handleDebug = (entry: AnalyzerHistoryEntry) => {
    workspace.getState().addTab('system-prompt-debugger', { bufferText: entry.bufferText })
  }

  const toggleExpand = (index: number) => {
    setExpandedEntries(prev => {
      const next = new Set(prev)
      if (next.has(index)) next.delete(index)
      else next.add(index)
      return next
    })
  }

  const toggleResponse = (index: number) => {
    setExpandedResponses(prev => {
      const next = new Set(prev)
      if (next.has(index)) next.delete(index)
      else next.add(index)
      return next
    })
  }

  const togglePrompt = (index: number) => {
    setExpandedPrompts(prev => {
      const next = new Set(prev)
      if (next.has(index)) next.delete(index)
      else next.add(index)
      return next
    })
  }

  const reversed = [...entries].reverse()

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
        <h3 style={{ margin: 0, color: '#ccc' }}>Analyzer History</h3>
        <span style={{ color: '#888', fontSize: 12 }}>{entries.length} entries</span>
        <button
          onClick={handleRefresh}
          style={{
            padding: '4px 12px',
            background: '#333',
            color: '#ccc',
            border: '1px solid #555',
            borderRadius: 4,
            cursor: 'pointer',
            fontSize: 12
          }}
        >
          Refresh
        </button>
      </div>

      <div>
        {reversed.length === 0 ? (
          <div style={{ color: '#888', fontSize: 13, padding: 8 }}>No history entries yet.</div>
        ) : (
          <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
            {reversed.map((entry, i) => (
              <div
                key={`${String(entry.timestamp)}-${entry.kind}`}
                style={{
                  background: '#1e1e1e',
                  border: '1px solid #333',
                  borderRadius: 4,
                  padding: '8px 12px',
                  display: 'flex',
                  gap: 12,
                  alignItems: 'flex-start'
                }}
              >
                <div style={{ display: 'flex', flexDirection: 'column', gap: 4, minWidth: 0, flex: 1 }}>
                  <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                    <span style={{ color: '#888', fontSize: 11, fontFamily: 'monospace' }}>
                      {formatTime(entry.timestamp)}
                    </span>
                    <span
                      style={{
                        background: KIND_COLORS[entry.kind] ?? '#555',
                        color: '#fff',
                        padding: '1px 6px',
                        borderRadius: 3,
                        fontSize: 11,
                        fontWeight: 500
                      }}
                    >
                      {entry.kind}
                    </span>
                    <span style={{ color: '#888', fontSize: 11, fontFamily: 'monospace' }}>
                      {entry.model}
                    </span>
                    {entry.cached && (
                      <span
                        style={{
                          background: '#2e7d32',
                          color: '#fff',
                          padding: '1px 6px',
                          borderRadius: 3,
                          fontSize: 11,
                          fontWeight: 500
                        }}
                      >
                        cached
                      </span>
                    )}
                    {entry.durationMs !== undefined && (
                      <span
                        style={{
                          background: '#555',
                          color: '#ddd',
                          padding: '1px 6px',
                          borderRadius: 3,
                          fontSize: 11,
                          fontWeight: 500,
                          fontFamily: 'monospace'
                        }}
                      >
                        {String(entry.durationMs)}ms
                      </span>
                    )}
                    {entry.error && (
                      <span
                        style={{
                          background: '#f44747',
                          color: '#fff',
                          padding: '1px 6px',
                          borderRadius: 3,
                          fontSize: 11,
                          fontWeight: 500
                        }}
                      >
                        error
                      </span>
                    )}
                  </div>
                  {entry.error && (
                    <div style={{ color: '#f44747', fontSize: 12 }}>{entry.error}</div>
                  )}
                  <pre
                    onClick={() => { toggleExpand(entries.length - 1 - i); }}
                    style={{
                      margin: 0,
                      color: '#666',
                      fontSize: 11,
                      fontFamily: 'monospace',
                      whiteSpace: 'pre-wrap',
                      overflow: 'hidden',
                      textOverflow: 'ellipsis',
                      cursor: 'pointer'
                    }}
                  >
                    {expandedEntries.has(entries.length - 1 - i) ? entry.bufferText : bufferPreview(entry.bufferText)}
                  </pre>
                  {entry.response && (
                    <pre
                      onClick={() => { toggleResponse(entries.length - 1 - i); }}
                      style={{
                        margin: 0,
                        color: '#23d18b',
                        fontSize: 11,
                        fontFamily: 'monospace',
                        whiteSpace: 'pre-wrap',
                        overflow: 'hidden',
                        textOverflow: 'ellipsis',
                        cursor: 'pointer',
                        background: '#1a1a1a',
                        padding: 4,
                        borderRadius: 3
                      }}
                    >
                      {expandedResponses.has(entries.length - 1 - i) ? entry.response : entry.response.slice(0, 80) + (entry.response.length > 80 ? '...' : '')}
                    </pre>
                  )}
                  {entry.systemPrompt && (
                    <pre
                      onClick={() => { togglePrompt(entries.length - 1 - i); }}
                      style={{
                        margin: 0,
                        color: '#b5cea8',
                        fontSize: 11,
                        fontFamily: 'monospace',
                        whiteSpace: 'pre-wrap',
                        overflow: 'hidden',
                        textOverflow: 'ellipsis',
                        cursor: 'pointer',
                        background: '#1a1a1a',
                        padding: 4,
                        borderRadius: 3
                      }}
                    >
                      {expandedPrompts.has(entries.length - 1 - i) ? entry.systemPrompt : 'system prompt (click to expand)'}
                    </pre>
                  )}
                </div>
                <button
                  onClick={() => { handleDebug(entry); }}
                  style={{
                    padding: '4px 8px',
                    background: '#333',
                    color: '#ccc',
                    border: '1px solid #555',
                    borderRadius: 4,
                    cursor: 'pointer',
                    fontSize: 11,
                    whiteSpace: 'nowrap',
                    flexShrink: 0
                  }}
                >
                  Debug
                </button>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  )
}
