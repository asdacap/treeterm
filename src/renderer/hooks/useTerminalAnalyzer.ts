import { useEffect, useRef, useState } from 'react'
import type { Terminal as XTerm } from '@xterm/xterm'
import { useSettingsStore } from '../store/settings'

export type TerminalAiState =
  | 'idle'
  | 'working'
  | 'user_input_required'
  | 'permission_request'
  | 'safe_permission_requested'
  | 'completed'
  | 'error'

export function useTerminalAnalyzer(
  terminal: XTerm | null,
  dataVersionRef: React.MutableRefObject<number> | null,
  cwd: string
): { aiState: TerminalAiState; analyzing: boolean } {
  const [aiState, setAiState] = useState<TerminalAiState>('idle')
  const [analyzing, setAnalyzing] = useState(false)
  const lastVersionRef = useRef(0)
  const debounceTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const settings = useSettingsStore((s) => s.settings)

  useEffect(() => {
    if (!terminal || !dataVersionRef) return
    if (!settings.llm.apiKey || !settings.terminalAnalyzer.model) return

    const analyze = async () => {
      try {
        const numLines = settings.terminalAnalyzer.bufferLines || 10
        const xtermBuffer = terminal.buffer.normal
        const contentEnd = xtermBuffer.baseY + xtermBuffer.cursorY + 1
        const startLine = Math.max(0, contentEnd - numLines)
        const lines: string[] = []
        for (let i = startLine; i < contentEnd; i++) {
          const line = xtermBuffer.getLine(i)
          if (line) lines.push(line.translateToString(true))
        }

        const buffer = lines.join('\n')
        if (!buffer.trim()) return
        console.debug('[terminal-analyzer] buffer:', buffer)

        setAnalyzing(true)
        const result = await window.electron.llm.analyzeTerminal(buffer, cwd, {
          baseUrl: settings.llm.baseUrl,
          apiKey: settings.llm.apiKey,
          model: settings.terminalAnalyzer.model,
          systemPrompt: settings.terminalAnalyzer.systemPrompt,
          disableReasoning: settings.terminalAnalyzer.disableReasoning,
          safePaths: settings.terminalAnalyzer.safePaths
        })

        console.debug('[terminal-analyzer] result:', result)
        if ('state' in result) {
          console.debug('[terminal-analyzer] state set:', result.state)
          setAiState(result.state as TerminalAiState)
        } else if ('error' in result) {
          console.error('[terminal-analyzer] error:', result.error)
          setAiState('error')
        } else {
          console.debug('[terminal-analyzer] ignored (no state in result)')
        }
      } catch (err) {
        console.error('[terminal-analyzer] LLM call failed:', err)
        setAiState('error')
      } finally {
        setAnalyzing(false)
      }
    }

    const interval = setInterval(() => {
      if (dataVersionRef.current === lastVersionRef.current) return

      lastVersionRef.current = dataVersionRef.current
      setAiState('working')

      if (debounceTimerRef.current) clearTimeout(debounceTimerRef.current)
      debounceTimerRef.current = setTimeout(analyze, 500)
    }, 200)

    return () => {
      clearInterval(interval)
      if (debounceTimerRef.current) clearTimeout(debounceTimerRef.current)
    }
  }, [terminal, dataVersionRef, cwd, settings.llm.apiKey, settings.llm.baseUrl, settings.terminalAnalyzer.model, settings.terminalAnalyzer.systemPrompt, settings.terminalAnalyzer.disableReasoning, settings.terminalAnalyzer.safePaths, settings.terminalAnalyzer.bufferLines])

  return { aiState, analyzing }
}
