import { useEffect, useRef } from 'react'
import { Terminal as XTerm } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import { useWorkspaceStore } from '../store/workspace'
import '@xterm/xterm/css/xterm.css'

interface TerminalProps {
  cwd: string
  workspaceId: string
  terminalId: string
}

export default function Terminal({ cwd, workspaceId, terminalId }: TerminalProps) {
  const containerRef = useRef<HTMLDivElement>(null)
  const terminalRef = useRef<XTerm | null>(null)
  const fitAddonRef = useRef<FitAddon | null>(null)
  const ptyIdRef = useRef<string | null>(null)
  const unsubscribeRef = useRef<(() => void) | null>(null)
  const isMountedRef = useRef(true)

  const workspace = useWorkspaceStore((state) => state.workspaces[workspaceId])
  const sandbox = workspace?.sandbox
  const setPtyId = useWorkspaceStore((state) => state.setPtyId)

  useEffect(() => {
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

    // Create PTY with sandbox config
    window.electron.terminal.create(cwd, sandbox).then((id) => {
      if (!id) return

      // Check if component is still mounted
      if (!isMountedRef.current) {
        // Component unmounted during PTY creation - kill the orphaned PTY
        window.electron.terminal.kill(id)
        return
      }

      ptyIdRef.current = id
      setPtyId(workspaceId, terminalId, id)

      // Subscribe to PTY output
      unsubscribeRef.current = window.electron.terminal.onData(id, (data) => {
        terminal.write(data)
      })

      // Send initial resize
      window.electron.terminal.resize(id, terminal.cols, terminal.rows)
    })

    // Forward terminal input to PTY
    const inputDisposable = terminal.onData((data) => {
      if (ptyIdRef.current) {
        window.electron.terminal.write(ptyIdRef.current, data)
      }
    })

    // Handle resize
    const resizeObserver = new ResizeObserver(() => {
      fitAddon.fit()
      if (ptyIdRef.current) {
        window.electron.terminal.resize(ptyIdRef.current, terminal.cols, terminal.rows)
      }
    })
    resizeObserver.observe(containerRef.current)

    // Cleanup
    return () => {
      isMountedRef.current = false
      inputDisposable.dispose()
      resizeObserver.disconnect()
      if (unsubscribeRef.current) {
        unsubscribeRef.current()
      }
      if (ptyIdRef.current) {
        window.electron.terminal.kill(ptyIdRef.current)
      }
      terminal.dispose()
    }
  }, [cwd, terminalId, sandbox?.enabled, workspaceId])

  return <div ref={containerRef} className="terminal-container" />
}
