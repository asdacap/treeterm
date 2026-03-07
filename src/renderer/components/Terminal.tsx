import { useEffect, useRef } from 'react'
import { Terminal as XTerm } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import { useWorkspaceStore } from '../store/workspace'
import { useSettingsStore } from '../store/settings'
import TerminalScrollWrapper from './TerminalScrollWrapper'
import type { TerminalState, SandboxConfig } from '../types'
import '@xterm/xterm/css/xterm.css'

interface TerminalProps {
  cwd: string
  workspaceId: string
  tabId: string
  config?: Record<string, unknown>
  sandbox?: SandboxConfig
}

export default function Terminal({ cwd, workspaceId, tabId, config, sandbox }: TerminalProps) {
  const containerRef = useRef<HTMLDivElement>(null)
  const terminalRef = useRef<XTerm | null>(null)
  const fitAddonRef = useRef<FitAddon | null>(null)
  const ptyIdRef = useRef<string | null>(null)
  const unsubscribeRef = useRef<(() => void) | null>(null)
  const isMountedRef = useRef(true)

  const workspace = useWorkspaceStore((state) => state.workspaces[workspaceId])
  const updateTabState = useWorkspaceStore((state) => state.updateTabState)
  const settings = useSettingsStore((state) => state.settings)

  // Get existing ptyId from store for reconnection
  const tab = workspace?.tabs.find((t) => t.id === tabId)
  const existingPtyId = (tab?.state as TerminalState | undefined)?.ptyId

  useEffect(() => {
    console.log(`[Terminal ${tabId}] useEffect running`, {
      cwd,
      sandboxEnabled: sandbox?.enabled,
      workspaceId
    })

    if (!containerRef.current) return

    isMountedRef.current = true
    const isSandboxed = sandbox?.enabled ?? false

    // Create terminal with sandbox-aware theme
    const terminal = new XTerm({
      cursorBlink: true,
      fontSize: 14,
      fontFamily: 'Menlo, Monaco, "Courier New", monospace',
      theme: {
        background: isSandboxed ? '#1a1a2e' : '#1e1e1e', // Slightly different bg for sandboxed
        foreground: '#d4d4d4',
        cursor: '#d4d4d4',
        cursorAccent: isSandboxed ? '#1a1a2e' : '#1e1e1e',
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

    // Helper to subscribe to PTY and set up refs
    const connectToPty = (id: string) => {
      ptyIdRef.current = id
      unsubscribeRef.current = window.electron.terminal.onData(id, (data) => {
        terminal.write(data)
      })
      window.electron.terminal.resize(id, terminal.cols, terminal.rows)
    }

    // Try to reconnect to existing PTY, or create a new one
    const initPty = async () => {
      // Check if we have an existing PTY that's still alive
      if (existingPtyId) {
        const isAlive = await window.electron.terminal.isAlive(existingPtyId)
        if (isAlive) {
          console.log(`[Terminal ${tabId}] reconnecting to existing PTY:`, existingPtyId)
          if (!isMountedRef.current) return
          connectToPty(existingPtyId)
          return
        }
      }

      // No existing PTY or it's dead - create a new one
      // Determine startup command from config
      const startupCommand = (config?.command as string) || undefined

      const id = await window.electron.terminal.create(cwd, sandbox, startupCommand)
      if (!id) return

      // Check if component is still mounted
      if (!isMountedRef.current) {
        // Component unmounted during PTY creation - kill the orphaned PTY
        window.electron.terminal.kill(id)
        return
      }

      console.log(`[Terminal ${tabId}] created new PTY:`, id)
      connectToPty(id)
      updateTabState<TerminalState>(workspaceId, tabId, (state) => ({
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
      console.log(`[Terminal ${tabId}] cleanup running (PTY preserved):`, {
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
      // Note: We intentionally don't kill the PTY here
      // The PTY lifecycle is managed by removeWorkspace/removeTab in workspace.ts
      terminal.dispose()
    }
  // Note: existingPtyId is intentionally NOT in deps - we only check it on mount/re-run
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [cwd, tabId, sandbox?.enabled, workspaceId, config])

  return (
    <TerminalScrollWrapper terminalRef={terminalRef}>
      <div ref={containerRef} className="terminal-container" />
    </TerminalScrollWrapper>
  )
}
