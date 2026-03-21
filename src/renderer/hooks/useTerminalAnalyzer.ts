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

export function useTerminalAnalyzer(
  terminal: XTerm | null,
  dataVersionRef: React.MutableRefObject<number> | null,
  cwd: string
): TerminalAiState {
  const [aiState, setAiState] = useState<TerminalAiState>('idle')
  const lastVersionRef = useRef(0)
  const isAnalyzingRef = useRef(false)
  const debounceTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const settings = useSettingsStore((s) => s.settings)

  useEffect(() => {
    if (!terminal || !dataVersionRef) return
    if (!settings.llm.apiKey || !settings.terminalAnalyzer.model) return

    const analyze = async () => {
      if (isAnalyzingRef.current) return
      isAnalyzingRef.current = true

      try {
        const numLines = settings.terminalAnalyzer.bufferLines || 10
        const buffer = terminal.buffer.active
        const lineCount = buffer.length
        const startLine = Math.max(0, lineCount - numLines)
        const lines: string[] = []
        for (let i = startLine; i < lineCount; i++) {
          const line = buffer.getLine(i)
          if (line) lines.push(line.translateToString(true))
        }

        if (lines.every((l) => l.trim() === '')) return

        console.debug('[terminal-analyzer] buffer:', lines)

        const result = await window.electron.llm.analyzeTerminal(lines, cwd, {
          baseUrl: settings.llm.baseUrl,
          apiKey: settings.llm.apiKey,
          model: settings.terminalAnalyzer.model,
          systemPrompt: settings.terminalAnalyzer.systemPrompt,
          disableReasoning: settings.terminalAnalyzer.disableReasoning,
          safePaths: settings.terminalAnalyzer.safePaths
        })

        if ('state' in result) {
          setAiState(result.state as TerminalAiState)
        }
      } catch (err) {
        console.warn('[terminal-analyzer] LLM call failed:', err)
      } finally {
        isAnalyzingRef.current = false
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

  return aiState
}
