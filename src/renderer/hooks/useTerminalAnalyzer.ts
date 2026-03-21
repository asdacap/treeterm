import { useEffect, useRef } from 'react'
import type { Terminal as XTerm } from '@xterm/xterm'
import { useSettingsStore } from '../store/settings'
import type { ActivityState, AiHarnessState } from '../types'

export function useTerminalAnalyzer(
  terminal: XTerm | null,
  dataVersionRef: React.MutableRefObject<number> | null,
  cwd: string,
  updateTabState: <T>(tabId: string, updater: (state: T) => T) => void,
  tabId: string
): void {
  const lastVersionRef = useRef(0)
  const debounceTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const settings = useSettingsStore((s) => s.settings)

  useEffect(() => {
    if (!terminal || !dataVersionRef) return
    if (!settings.llm.apiKey || !settings.terminalAnalyzer.model) return

    const analyze = async () => {
      const requestVersion = dataVersionRef.current
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

        updateTabState<AiHarnessState>(tabId, (s) => ({ ...s, analyzing: true }))
        const result = await window.electron.llm.analyzeTerminal(buffer, cwd, {
          baseUrl: settings.llm.baseUrl,
          apiKey: settings.llm.apiKey,
          model: settings.terminalAnalyzer.model,
          systemPrompt: settings.terminalAnalyzer.systemPrompt,
          reasoningEffort: settings.terminalAnalyzer.reasoningEffort,
          safePaths: settings.terminalAnalyzer.safePaths
        })

        if (dataVersionRef.current !== requestVersion) {
          console.debug('[terminal-analyzer] discarding stale response')
          updateTabState<AiHarnessState>(tabId, (s) => ({ ...s, analyzing: false }))
          return
        }

        console.debug('[terminal-analyzer] result:', result)
        if ('state' in result) {
          console.debug('[terminal-analyzer] state set:', result.state, 'reason:', result.reason)
          updateTabState<AiHarnessState>(tabId, (s) => ({
            ...s,
            aiState: result.state as ActivityState,
            reason: result.reason,
            analyzing: false
          }))
        } else if ('error' in result) {
          console.error('[terminal-analyzer] error:', result.error)
          updateTabState<AiHarnessState>(tabId, (s) => ({ ...s, aiState: 'error', analyzing: false }))
        } else {
          console.debug('[terminal-analyzer] ignored (no state in result)')
          updateTabState<AiHarnessState>(tabId, (s) => ({ ...s, analyzing: false }))
        }
      } catch (err) {
        console.error('[terminal-analyzer] LLM call failed:', err)
        updateTabState<AiHarnessState>(tabId, (s) => ({ ...s, aiState: 'error', analyzing: false }))
      }
    }

    const interval = setInterval(() => {
      if (dataVersionRef.current === lastVersionRef.current) return

      lastVersionRef.current = dataVersionRef.current
      updateTabState<AiHarnessState>(tabId, (s) => ({ ...s, aiState: 'working' }))

      if (debounceTimerRef.current) clearTimeout(debounceTimerRef.current)
      debounceTimerRef.current = setTimeout(analyze, 500)
    }, 200)

    return () => {
      clearInterval(interval)
      if (debounceTimerRef.current) clearTimeout(debounceTimerRef.current)
    }
  }, [terminal, dataVersionRef, cwd, updateTabState, tabId, settings.llm.apiKey, settings.llm.baseUrl, settings.terminalAnalyzer.model, settings.terminalAnalyzer.systemPrompt, settings.terminalAnalyzer.reasoningEffort, settings.terminalAnalyzer.safePaths, settings.terminalAnalyzer.bufferLines])
}
