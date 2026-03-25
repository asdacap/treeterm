import { useState } from 'react'
import type { ApplicationRenderProps } from '../types'
import type { AnalyzerHistoryEntry } from '../store/createAnalyzerStore'
import type { AiHarnessRef } from '../../applications/aiHarness/renderer'

const KIND_COLORS: Record<string, string> = {
  analyzer: '#1a5276',
  title: '#6a0dad'
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

export default function AnalyzerHistory({ tab, workspace }: ApplicationRenderProps) {
  const state = tab.state
  if (!isAnalyzerHistoryState(state)) {
    return <div style={{ padding: 16, color: '#f14c4c' }}>Invalid state: missing sourceTabId</div>
  }

  const ref = workspace.getState().getTabRef(state.sourceTabId) as AiHarnessRef | null
  const analyzer = ref?.analyzer ?? null
  if (!analyzer) {
    return <div style={{ padding: 16, color: '#f14c4c' }}>Analyzer not found for tab {state.sourceTabId}</div>
  }

  const [entries, setEntries] = useState<AnalyzerHistoryEntry[]>(() => analyzer.getState().getHistory())
  const [expandedEntries, setExpandedEntries] = useState<Set<number>>(new Set())
  const [expandedResponses, setExpandedResponses] = useState<Set<number>>(new Set())

  const handleRefresh = () => {
    setEntries(analyzer.getState().getHistory())
  }

  const handleDebug = (entry: AnalyzerHistoryEntry) => {
    workspace.getState().addTab<{ bufferText: string }>('system-prompt-debugger', { bufferText: entry.bufferText })
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

  const reversed = [...entries].reverse()

  return (
    <div style={{ padding: 16, display: 'flex', flexDirection: 'column', gap: 12, height: '100%', overflow: 'hidden' }}>
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

      <div style={{ flex: 1, overflow: 'auto' }}>
        {reversed.length === 0 ? (
          <div style={{ color: '#888', fontSize: 13, padding: 8 }}>No history entries yet.</div>
        ) : (
          <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
            {reversed.map((entry, i) => (
              <div
                key={entries.length - 1 - i}
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
                    onClick={() => toggleExpand(entries.length - 1 - i)}
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
                      onClick={() => toggleResponse(entries.length - 1 - i)}
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
                </div>
                <button
                  onClick={() => handleDebug(entry)}
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
