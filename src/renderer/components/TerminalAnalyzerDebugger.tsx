import { useEffect, useState } from 'react'
import { useSettingsStore } from '../store/settings'
import type { ApplicationRenderProps, ReasoningEffort } from '../types'
import { createLlmClient } from '../lib/llmClient'

const llm = createLlmClient()

interface DebuggerState {
  bufferText?: string
}

export default function TerminalAnalyzerDebugger({ tab }: ApplicationRenderProps) {
  const settings = useSettingsStore((s) => s.settings)
  const debuggerState = tab.state as DebuggerState | undefined
  const [systemPrompt, setSystemPrompt] = useState(settings.terminalAnalyzer.systemPrompt)
  const [bufferText, setBufferText] = useState(debuggerState?.bufferText ?? '')
  const [model, setModel] = useState(settings.terminalAnalyzer.model)
  const [reasoningEffort, setReasoningEffort] = useState(settings.terminalAnalyzer.reasoningEffort)
  const [result, setResult] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [duration, setDuration] = useState<number | null>(null)

  // Sync buffer text when tab state changes (e.g. opened from context menu)
  useEffect(() => {
    if (debuggerState?.bufferText) {
      setBufferText(debuggerState.bufferText)
    }
  }, [debuggerState?.bufferText])

  const handleTest = async () => {
    if (!model) {
      setError('Model must be configured.')
      return
    }

    await llm.clearAnalyzerCache()
    const buffer = bufferText
    setLoading(true)
    setError(null)
    setResult(null)
    setDuration(null)

    const start = Date.now()
    try {
      const response = await llm.analyzeTerminal(buffer, '', {
        baseUrl: settings.llm.baseUrl,
        apiKey: settings.llm.apiKey,
        model,
        systemPrompt,
        reasoningEffort,
        safePaths: settings.terminalAnalyzer.safePaths
      })
      setDuration(Date.now() - start)

      if ('error' in response) {
        setError(response.error)
      } else {
        setResult(JSON.stringify(response, null, 2))
      }
    } catch (err) {
      setDuration(Date.now() - start)
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setLoading(false)
    }
  }

  return (
    <div style={{ padding: 16, display: 'flex', flexDirection: 'column', gap: 12, height: '100%', overflow: 'auto' }}>
      <h3 style={{ margin: 0, color: '#ccc' }}>Terminal Analyzer Debugger</h3>

      <div style={{ display: 'flex', gap: 12, alignItems: 'center' }}>
        <div style={{ flex: 1 }}>
          <label style={{ color: '#aaa', fontSize: 12 }}>Model</label>
          <input
            type="text"
            value={model}
            onChange={(e) => { setModel(e.target.value); }}
            style={{
              width: '100%',
              background: '#1e1e1e',
              color: '#d4d4d4',
              border: '1px solid #333',
              borderRadius: 4,
              padding: '4px 8px',
              fontFamily: 'monospace',
              fontSize: 12,
              boxSizing: 'border-box'
            }}
          />
        </div>
        <label style={{ color: '#aaa', fontSize: 12, display: 'flex', alignItems: 'center', gap: 4, cursor: 'pointer', whiteSpace: 'nowrap' }}>
          Reasoning
          <select
            value={reasoningEffort}
            onChange={(e) => { setReasoningEffort(e.target.value as ReasoningEffort); }}
            style={{
              background: '#1e1e1e',
              color: '#d4d4d4',
              border: '1px solid #333',
              borderRadius: 4,
              padding: '2px 4px',
              fontSize: 12
            }}
          >
            <option value="off">Off</option>
            <option value="low">Low</option>
            <option value="medium">Medium</option>
            <option value="high">High</option>
          </select>
        </label>
      </div>

      <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
        <label style={{ color: '#aaa', fontSize: 12 }}>System Prompt</label>
        <button
          onClick={() => {
            useSettingsStore.getState().updateSetting('terminalAnalyzer', 'systemPrompt', systemPrompt)
          }}
          style={{
            padding: '2px 8px',
            background: '#333',
            color: '#ccc',
            border: '1px solid #555',
            borderRadius: 4,
            cursor: 'pointer',
            fontSize: 11
          }}
        >
          Update
        </button>
      </div>
      <textarea
        value={systemPrompt}
        onChange={(e) => { setSystemPrompt(e.target.value); }}
        style={{
          minHeight: 120,
          background: '#1e1e1e',
          color: '#d4d4d4',
          border: '1px solid #333',
          borderRadius: 4,
          padding: 8,
          fontFamily: 'monospace',
          fontSize: 12,
          resize: 'vertical'
        }}
      />

      <label style={{ color: '#aaa', fontSize: 12 }}>Buffer (paste from debug console)</label>
      <textarea
        value={bufferText}
        onChange={(e) => { setBufferText(e.target.value); }}
        placeholder="Paste terminal buffer lines here..."
        style={{
          minHeight: 120,
          background: '#1e1e1e',
          color: '#d4d4d4',
          border: '1px solid #333',
          borderRadius: 4,
          padding: 8,
          fontFamily: 'monospace',
          fontSize: 12,
          resize: 'vertical'
        }}
      />

      <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
        <button
          onClick={() => { void handleTest() }}
          disabled={loading}
          style={{
            padding: '6px 16px',
            background: loading ? '#333' : '#2472c8',
            color: '#fff',
            border: 'none',
            borderRadius: 4,
            cursor: loading ? 'not-allowed' : 'pointer',
            fontSize: 13
          }}
        >
          {loading ? 'Analyzing...' : 'Test'}
        </button>
        {duration !== null && (
          <span style={{ color: '#888', fontSize: 12 }}>{String(duration)}ms</span>
        )}
      </div>

      {error && (
        <div style={{ color: '#f14c4c', fontSize: 12, fontFamily: 'monospace', whiteSpace: 'pre-wrap' }}>
          {error}
        </div>
      )}

      {result && (
        <pre style={{
          background: '#1e1e1e',
          color: '#23d18b',
          border: '1px solid #333',
          borderRadius: 4,
          padding: 8,
          fontSize: 12,
          margin: 0,
          whiteSpace: 'pre-wrap'
        }}>
          {result}
        </pre>
      )}
    </div>
  )
}
