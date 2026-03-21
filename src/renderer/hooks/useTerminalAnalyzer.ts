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
  const settings = useSettingsStore((s) => s.settings)

  useEffect(() => {
    if (!terminal || !dataVersionRef) return
    if (!settings.llm.apiKey || !settings.terminalAnalyzer.model) return

    const interval = setInterval(async () => {
      // Skip if no new data since last check
      if (dataVersionRef.current === lastVersionRef.current) return
      // Skip if already analyzing
      if (isAnalyzingRef.current) return

      lastVersionRef.current = dataVersionRef.current
      isAnalyzingRef.current = true

      try {
        // Read last N lines from xterm buffer
        const numLines = settings.terminalAnalyzer.bufferLines || 10
        const buffer = terminal.buffer.active
        const lineCount = buffer.length
        const startLine = Math.max(0, lineCount - numLines)
        const lines: string[] = []
        for (let i = startLine; i < lineCount; i++) {
          const line = buffer.getLine(i)
          if (line) lines.push(line.translateToString(true))
        }

        // Skip if all lines are empty
        if (lines.every((l) => l.trim() === '')) return

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
    }, 10_000)

    return () => clearInterval(interval)
  }, [terminal, dataVersionRef, cwd, settings.llm.apiKey, settings.llm.baseUrl, settings.terminalAnalyzer.model, settings.terminalAnalyzer.systemPrompt, settings.terminalAnalyzer.disableReasoning, settings.terminalAnalyzer.safePaths])

  return aiState
}
