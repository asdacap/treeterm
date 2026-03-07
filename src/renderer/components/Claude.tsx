import { useCallback, useEffect, useRef } from 'react'
import { Terminal as XTerm } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import { useWorkspaceStore } from '../store/workspace'
import { useActivityStateStore } from '../store/activityState'
import { createActivityStateDetector } from '../utils/activityStateDetector'
import TerminalToolbar from './TerminalToolbar'
import type { SandboxConfig } from '../types'
import type { ClaudeState } from '../applications/claude'
import '@xterm/xterm/css/xterm.css'

interface ClaudeProps {
  cwd: string
  workspaceId: string
  tabId: string
  sandbox?: SandboxConfig
}

export default function Claude({ cwd, workspaceId, tabId, sandbox }: ClaudeProps) {
  const containerRef = useRef<HTMLDivElement>(null)
  const terminalRef = useRef<XTerm | null>(null)
  const fitAddonRef = useRef<FitAddon | null>(null)
  const ptyIdRef = useRef<string | null>(null)
  const unsubscribeRef = useRef<(() => void) | null>(null)
  const detectorRef = useRef<ReturnType<typeof createActivityStateDetector> | null>(null)
  const isMountedRef = useRef(true)

  const workspace = useWorkspaceStore((state) => state.workspaces[workspaceId])
  const updateTabState = useWorkspaceStore((state) => state.updateTabState)
  const setTabState = useActivityStateStore((state) => state.setTabState)

  // Get existing ptyId from store for reconnection
  const tab = workspace?.tabs.find((t) => t.id === tabId)
  const existingPtyId = (tab?.state as ClaudeState | undefined)?.ptyId

  useEffect(() => {
    console.log(`[Claude ${tabId}] useEffect running`, {
      cwd,
      sandboxEnabled: sandbox?.enabled,
      workspaceId
    })

    if (!containerRef.current) return

    isMountedRef.current = true
    const isSandboxed = sandbox?.enabled ?? false

    // Create terminal with Claude-specific theme (slightly purple tint)
    const terminal = new XTerm({
      cursorBlink: true,
      fontSize: 14,
      fontFamily: 'Menlo, Monaco, "Courier New", monospace',
      theme: {
        background: isSandboxed ? '#1a1a2e' : '#1a1a24', // Slight purple tint for Claude
        foreground: '#d4d4d4',
        cursor: '#d4d4d4',
        cursorAccent: isSandboxed ? '#1a1a2e' : '#1a1a24',
        selectionBackground: '#264f78',
        black: '#000000',
        red: '#cd3131',
        green: '#0dbc79',
        yellow: '#e5e510',
        blue: '#2472c8',
        magenta: '#bc3fbc',
        cyan: '#11a8cd',
        white: '#e5e5e5',
        brightBlack: '#666666',
        brightRed: '#f14c4c',
        brightGreen: '#23d18b',
        brightYellow: '#f5f543',
        brightBlue: '#3b8eea',
        brightMagenta: '#d670d6',
        brightCyan: '#29b8db',
        brightWhite: '#ffffff'
      }
    })

    const fitAddon = new FitAddon()
    terminal.loadAddon(fitAddon)
    terminal.open(containerRef.current)
    fitAddon.fit()

    terminalRef.current = terminal
    fitAddonRef.current = fitAddon

    // Create activity state detector with Claude-specific patterns
    const detector = createActivityStateDetector(
      (state) => setTabState(tabId, state),
      {
        promptPatterns: [/>\s*$/], // Claude uses > prompt
        workingPatterns: [/[⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏]/] // Braille spinners
      }
    )
    detectorRef.current = detector

    // Helper to subscribe to PTY and set up refs
    const connectToPty = (id: string) => {
      ptyIdRef.current = id
      unsubscribeRef.current = window.electron.terminal.onData(id, (data) => {
        terminal.write(data)
        // Process data for activity state detection
        detector.processData(data)
      })
      window.electron.terminal.resize(id, terminal.cols, terminal.rows)
    }

    // Try to reconnect to existing PTY, or create a new one
    const initPty = async () => {
      // Check if we have an existing PTY that's still alive
      if (existingPtyId) {
        const isAlive = await window.electron.terminal.isAlive(existingPtyId)
        if (isAlive) {
          console.log(`[Claude ${tabId}] reconnecting to existing PTY:`, existingPtyId)
          if (!isMountedRef.current) return
          connectToPty(existingPtyId)
          return
        }
      }

      // No existing PTY or it's dead - create a new one with 'claude' command
      const id = await window.electron.terminal.create(cwd, sandbox, 'claude')
      if (!id) return

      // Check if component is still mounted
      if (!isMountedRef.current) {
        // Component unmounted during PTY creation - kill the orphaned PTY
        window.electron.terminal.kill(id)
        return
      }

      console.log(`[Claude ${tabId}] created new PTY:`, id)
      connectToPty(id)
      updateTabState<ClaudeState>(workspaceId, tabId, (state) => ({
        ...state,
        ptyId: id
      }))
    }

    initPty()

    // Forward terminal input to PTY
    const inputDisposable = terminal.onData((data) => {
      if (ptyIdRef.current) {
        window.electron.terminal.write(ptyIdRef.current, data)
      }
    })

    // Handle resize
    const resizeObserver = new ResizeObserver((entries) => {
      const entry = entries[0]
      if (!entry) return

      // Skip resize when container is hidden (0x0 dimensions)
      const { width, height } = entry.contentRect
      if (width === 0 || height === 0) return

      fitAddon.fit()
      if (ptyIdRef.current) {
        window.electron.terminal.resize(ptyIdRef.current, terminal.cols, terminal.rows)
      }
    })
    resizeObserver.observe(containerRef.current)

    // Cleanup - DON'T kill PTY here, just unsubscribe
    // PTY is explicitly killed in removeWorkspace/removeTab
    return () => {
      console.log(`[Claude ${tabId}] cleanup running (PTY preserved):`, {
        ptyId: ptyIdRef.current,
        cwd,
        sandboxEnabled: sandbox?.enabled,
        workspaceId
      })
      isMountedRef.current = false
      inputDisposable.dispose()
      resizeObserver.disconnect()
      if (unsubscribeRef.current) {
        unsubscribeRef.current()
      }
      if (detectorRef.current) {
        detectorRef.current.destroy()
      }
      // Note: We intentionally don't kill the PTY here
      // The PTY lifecycle is managed by removeWorkspace/removeTab in workspace.ts
      terminal.dispose()
    }
    // Note: existingPtyId is intentionally NOT in deps - we only check it on mount/re-run
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [cwd, tabId, sandbox?.enabled, workspaceId])

  const handleScrollDown = useCallback(() => {
    if (terminalRef.current) {
      terminalRef.current.scrollToBottom()
    }
  }, [])

  return (
    <div className="terminal-wrapper">
      <TerminalToolbar onScrollDown={handleScrollDown} />
      <div ref={containerRef} className="terminal-container" />
    </div>
  )
}
