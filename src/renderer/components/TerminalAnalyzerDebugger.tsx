import { useState } from 'react'
import { useSettingsStore } from '../store/settings'

export default function TerminalAnalyzerDebugger() {
  const settings = useSettingsStore((s) => s.settings)
  const [systemPrompt, setSystemPrompt] = useState(settings.terminalAnalyzer.systemPrompt)
  const [bufferText, setBufferText] = useState('')
  const [result, setResult] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const handleTest = async () => {
    if (!settings.llm.apiKey || !settings.terminalAnalyzer.model) {
      setError('LLM API key and terminal analyzer model must be configured in settings.')
      return
    }

    const lines = bufferText.split('\n')
    setLoading(true)
    setError(null)
    setResult(null)

    try {
      const response = await window.electron.llm.analyzeTerminal(lines, '', {
        baseUrl: settings.llm.baseUrl,
        apiKey: settings.llm.apiKey,
        model: settings.terminalAnalyzer.model,
        systemPrompt,
        disableReasoning: settings.terminalAnalyzer.disableReasoning,
        safePaths: settings.terminalAnalyzer.safePaths
      })

      if ('error' in response) {
        setError(response.error)
      } else {
        setResult(JSON.stringify(response, null, 2))
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setLoading(false)
    }
  }

  return (
    <div style={{ padding: 16, display: 'flex', flexDirection: 'column', gap: 12, height: '100%', overflow: 'auto' }}>
      <h3 style={{ margin: 0, color: '#ccc' }}>Terminal Analyzer Debugger</h3>

      <label style={{ color: '#aaa', fontSize: 12 }}>System Prompt</label>
      <textarea
        value={systemPrompt}
        onChange={(e) => setSystemPrompt(e.target.value)}
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
        onChange={(e) => setBufferText(e.target.value)}
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

      <button
        onClick={handleTest}
        disabled={loading}
        style={{
          alignSelf: 'flex-start',
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
