import { useEffect, useRef, useState } from 'react'
import { Terminal as XTerm } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import { useWorkspaceStore } from '../store/workspace'
import { useSettingsStore } from '../store/settings'
import { useActivityStateStore } from '../store/activityState'
import { createActivityStateDetector } from '../utils/activityStateDetector'
import TerminalScrollWrapper from './TerminalScrollWrapper'
import DebugRawChars from './DebugRawChars'
import type { SandboxConfig, TerminalState } from '../types'
import '@xterm/xterm/css/xterm.css'

// Base state interface that all terminal-based states should extend
export interface BaseTerminalState extends TerminalState {
  sandbox?: SandboxConfig
}

export interface BaseTerminalConfig {
  // Theme customization
  themeBackground: string
  // Activity state detector prompt patterns (optional)
  promptPatterns?: RegExp[]
  // Startup command to run when creating new PTY (optional)
  startupCommand?: string
  // Log prefix for console messages
  logPrefix: string
}

export interface BaseTerminalProps {
  cwd: string
  workspaceId: string
  tabId: string
  sandbox?: SandboxConfig
  isVisible?: boolean
  config: BaseTerminalConfig
}

interface ContextMenu {
  x: number
  y: number
}

export default function BaseTerminal({
  cwd,
  workspaceId,
  tabId,
  sandbox,
  isVisible,
  config
}: BaseTerminalProps) {
  const containerRef = useRef<HTMLDivElement>(null)
  const terminalRef = useRef<XTerm | null>(null)
  const fitAddonRef = useRef<FitAddon | null>(null)
  const ptyIdRef = useRef<string | null>(null)
  const unsubscribeRef = useRef<(() => void) | null>(null)
  const detectorRef = useRef<ReturnType<typeof createActivityStateDetector> | null>(null)
  const isMountedRef = useRef(true)
  const rawCharsRef = useRef<string>('')
  const [rawCharsDisplay, setRawCharsDisplay] = useState<string>('')
  const [contextMenu, setContextMenu] = useState<ContextMenu | null>(null)

  const workspace = useWorkspaceStore((state) => state.workspaces[workspaceId])
  const updateTabState = useWorkspaceStore((state) => state.updateTabState)
  const setTabState = useActivityStateStore((state) => state.setTabState)
  const settings = useSettingsStore((state) => state.settings)

  // Get existing ptyId from store for reconnection
  const tab = workspace?.tabs.find((t) => t.id === tabId)
  const existingPtyId = (tab?.state as BaseTerminalState | undefined)?.ptyId

  useEffect(() => {
    console.log(`[${config.logPrefix} ${tabId}] useEffect running`, {
      cwd,
      sandboxEnabled: sandbox?.enabled,
      workspaceId
    })

    if (!containerRef.current) return

    isMountedRef.current = true
    const isSandboxed = sandbox?.enabled ?? false

    // Create terminal with configurable theme
    const terminal = new XTerm({
      cursorBlink: true,
      fontSize: 14,
      fontFamily: 'Menlo, Monaco, "Courier New", monospace',
      theme: {
        background: isSandboxed ? '#1a1a2e' : config.themeBackground,
        foreground: '#d4d4d4',
        cursor: '#d4d4d4',
        cursorAccent: isSandboxed ? '#1a1a2e' : config.themeBackground,
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

    // Create activity state detector with optional custom patterns
    const detector = createActivityStateDetector(
      (state) => setTabState(tabId, state),
      config.promptPatterns ? { promptPatterns: config.promptPatterns } : undefined
    )
    detectorRef.current = detector

    // Helper to subscribe to PTY and set up refs
    const connectToPty = (id: string) => {
      ptyIdRef.current = id
      unsubscribeRef.current = window.electron.terminal.onData(id, (data) => {
        terminal.write(data)
        // Process data for activity state detection
        detector.processData(data)
        // Capture last 50 raw characters for debug display
        rawCharsRef.current = (rawCharsRef.current + data).slice(-50)
        if (settings.terminal.showRawChars) {
          setRawCharsDisplay(rawCharsRef.current)
        }
      })
      window.electron.terminal.resize(id, terminal.cols, terminal.rows)
    }

    // Try to reconnect to existing PTY, or create a new one
    const initPty = async () => {
      // Check if we have an existing PTY that's still alive
      if (existingPtyId) {
        const isAlive = await window.electron.terminal.isAlive(existingPtyId)
        if (isAlive) {
          console.log(`[${config.logPrefix} ${tabId}] reconnecting to existing PTY:`, existingPtyId)
          if (!isMountedRef.current) return
          connectToPty(existingPtyId)
          return
        }
      }

      // No existing PTY or it's dead - create a new one
      const id = await window.electron.terminal.create(cwd, sandbox, config.startupCommand)
      if (!id) return

      // Check if component is still mounted
      if (!isMountedRef.current) {
        // Component unmounted during PTY creation - kill the orphaned PTY
        window.electron.terminal.kill(id)
        return
      }

      console.log(`[${config.logPrefix} ${tabId}] created new PTY:`, id)
      connectToPty(id)
      updateTabState<BaseTerminalState>(workspaceId, tabId, (state) => ({
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
      console.log(`[${config.logPrefix} ${tabId}] cleanup running (PTY preserved):`, {
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
  }, [cwd, tabId, sandbox?.enabled, workspaceId, config.startupCommand, config.themeBackground])

  // Refresh terminal when tab becomes visible to fix blank screen issue
  useEffect(() => {
    if (isVisible && terminalRef.current && fitAddonRef.current) {
      // Re-fit and refresh the terminal when becoming visible
      fitAddonRef.current.fit()
      terminalRef.current.refresh(0, terminalRef.current.rows)
    }
  }, [isVisible])

  // Update raw chars display when setting changes
  useEffect(() => {
    if (settings.terminal.showRawChars) {
      setRawCharsDisplay(rawCharsRef.current)
    } else {
      setRawCharsDisplay('')
    }
  }, [settings.terminal.showRawChars])

  // Close context menu when clicking outside
  useEffect(() => {
    if (!contextMenu) return
    const handleClick = () => setContextMenu(null)
    document.addEventListener('click', handleClick)
    return () => document.removeEventListener('click', handleClick)
  }, [contextMenu])

  const handleContextMenu = (e: React.MouseEvent) => {
    e.preventDefault()
    setContextMenu({ x: e.clientX, y: e.clientY })
  }

  const handleCopy = async () => {
    const selection = terminalRef.current?.getSelection()
    if (selection) {
      await navigator.clipboard.writeText(selection)
    }
    setContextMenu(null)
  }

  const handlePaste = async () => {
    const text = await navigator.clipboard.readText()
    if (text && ptyIdRef.current) {
      window.electron.terminal.write(ptyIdRef.current, text)
    }
    setContextMenu(null)
  }

  return (
    <TerminalScrollWrapper terminalRef={terminalRef}>
      <div ref={containerRef} className="terminal-container" onContextMenu={handleContextMenu} />
      {settings.terminal.showRawChars && <DebugRawChars rawChars={rawCharsDisplay} />}
      {contextMenu && (
        <div
          className="context-menu"
          style={{ top: contextMenu.y, left: contextMenu.x }}
        >
          <div className="context-menu-item" onClick={handleCopy}>
            Copy
          </div>
          <div className="context-menu-item" onClick={handlePaste}>
            Paste
          </div>
        </div>
      )}
    </TerminalScrollWrapper>
  )
}
