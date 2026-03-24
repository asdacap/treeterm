import { useEffect, useRef, useState } from 'react'
import { Terminal as XTerm } from '@xterm/xterm'
import { useStore } from 'zustand'
import { fitTerminal } from '../utils/fitTerminal'
import { useSettingsStore } from '../store/settings'
import { useAppStore } from '../store/app'
import { useActivityStateStore } from '../store/activityState'
import { useSessionApi } from '../contexts/SessionStoreContext'
import { createActivityStateDetector } from '../utils/activityStateDetector'
import type { Tty } from '../store/createTtyStore'
import TerminalScrollWrapper from './TerminalScrollWrapper'
import type { SandboxConfig, TerminalState, WorkspaceStore } from '../types'
import { useContextMenuStore } from '../store/contextMenu'
import ContextMenu from './ContextMenu'
import '@xterm/xterm/css/xterm.css'

// ANSI sequences that manipulate scrollback or clear the screen
const SCROLL_MANIPULATION_PATTERNS: { pattern: RegExp; name: string }[] = [
  { pattern: /\x1b\[3J/g, name: 'Clear scrollback buffer (CSI 3J)' },
  { pattern: /\x1b\[2J/g, name: 'Clear entire screen (CSI 2J)' },
  { pattern: /\x1b\[0?J/g, name: 'Clear cursor to end of screen (CSI J / CSI 0J)' },
  { pattern: /\x1b\[1J/g, name: 'Clear cursor to beginning of screen (CSI 1J)' },
  { pattern: /\x1b\[\d+;\d+r/g, name: 'Set scroll region (CSI n;m r)' },
  { pattern: /\x1b\[\d*S/g, name: 'Scroll up (CSI S)' },
  { pattern: /\x1b\[\d*T/g, name: 'Scroll down (CSI T)' },
]

function detectScrollManipulation(data: string): { name: string; match: string }[] {
  const matches: { name: string; match: string }[] = []
  for (const { pattern, name } of SCROLL_MANIPULATION_PATTERNS) {
    // Reset lastIndex since we reuse the regex
    pattern.lastIndex = 0
    let m: RegExpExecArray | null
    while ((m = pattern.exec(data)) !== null) {
      matches.push({ name, match: m[0] })
    }
  }
  return matches
}

// Utility to format raw chars for console debugging
function formatRawChars(str: string): string {
  let result = ''
  for (let i = 0; i < str.length; i++) {
    const code = str.charCodeAt(i)
    if (code === 0x1b) {
      result += '\\x1b'
    } else if (code === 0x0d) {
      result += '\\r'
    } else if (code === 0x0a) {
      result += '\\n'
    } else if (code === 0x09) {
      result += '\\t'
    } else if (code < 0x20) {
      result += `\\x${code.toString(16).padStart(2, '0')}`
    } else {
      result += str[i]
    }
  }
  return result
}

// Base state interface that all terminal-based states should extend
export interface BaseTerminalState extends TerminalState {
  sandbox?: SandboxConfig
}

export interface BaseTerminalConfig {
  // Theme customization
  themeBackground: string
  // Activity state detector prompt patterns (optional)
  promptPatterns?: RegExp[]
  // Log prefix for console messages
  logPrefix: string
  // Whether to disable the scrollbar (for tools with own scrolling like opencode)
  disableScrollbar?: boolean
  // Whether to strip CSI 3J (clear scrollback) from PTY data before writing to xterm
  stripScrollbackClear?: boolean
  // Whether to disable the regex-based activity state detector (e.g. when using LLM-based analysis)
  disableActivityDetector?: boolean
  // Callback when terminal is ready, provides terminal instance and data version ref
  onTerminalReady?: (terminal: XTerm, dataVersionRef: React.MutableRefObject<number>) => void
}

interface BaseTerminalProps {
  workspace: WorkspaceStore
  tabId: string
  config: BaseTerminalConfig
  extraButtons?: React.ReactNode
}

export default function BaseTerminal({
  workspace,
  tabId,
  config,
  extraButtons,
}: BaseTerminalProps) {
  const { workspace: wsData, removeTab } = useStore(workspace)
  const workspaceId = wsData.id
  const containerRef = useRef<HTMLDivElement>(null)
  const terminalRef = useRef<XTerm | null>(null)
  const ttyRef = useRef<Tty | null>(null)
  const dataVersionRef = useRef(0)
  const [overlay, setOverlay] = useState<{ message: string; type: 'info' | 'error' } | null>(null)
  const [loading, setLoading] = useState(true)

  const sessionStore = useSessionApi()
  const setTabState = useActivityStateStore((state) => state.setTabState)
  const settings = useSettingsStore((state) => state.settings)
  const clipboard = useAppStore((state) => state.clipboard)

  // Get existing ptyId from store for reconnection
  const appState = wsData?.appStates[tabId]
  const existingPtyId = (appState?.state as BaseTerminalState | undefined)?.ptyId

  useEffect(() => {
    console.log(`[${config.logPrefix} ${tabId}] useEffect running`, {
      existingPtyId,
      workspaceId
    })

    if (!containerRef.current) return

    let cancelled = false
    let terminal: XTerm | null = null
    let resizeTimeout: ReturnType<typeof setTimeout> | null = null
    let inputDisposable: { dispose(): void } | null = null
    let resizeObserver: ResizeObserver | null = null
    let detector: ReturnType<typeof createActivityStateDetector> | null = null
    let unsubscribe: (() => void) | null = null
    let rawChars = ''

    let initialResizeDone = false

    const setupResizeObserver = (term: XTerm, resize: (cols: number, rows: number) => void) => {
      resizeObserver = new ResizeObserver((entries) => {
        const entry = entries[0]
        if (!entry) return
        if (!initialResizeDone) return

        console.log(`[${config.logPrefix} ${tabId}] resize (observer):`, { cols: term.cols, rows: term.rows })
        fitTerminal(term, resize)
      })
      resizeObserver.observe(containerRef.current!)
    }

    const session = sessionStore.getState()

    const init = async () => {
      // Phase 1: Resolve TTY
      if (!existingPtyId) {
        setLoading(false)
        setOverlay({ message: 'No PTY available for this terminal', type: 'error' })
        return
      }

      let tty: Tty
      let scrollback: string[] | undefined
      let exitCode: number | undefined
      try {
        const result = await session.openTtyStream(existingPtyId)
        console.log(`[${config.logPrefix} ${tabId}] reattached to session:`, existingPtyId)
        tty = result.tty
        scrollback = result.scrollback
        exitCode = result.exitCode
      } catch (error) {
        console.log(`[${config.logPrefix} ${tabId}] failed to attach to PTY:`, existingPtyId, error)
        if (cancelled) return
        setLoading(false)
        setOverlay({ message: `Failed to reattach terminal: ${error instanceof Error ? error.message : 'Unknown error'}`, type: 'error' })
        return
      }

      if (cancelled) return
      if (!containerRef.current) return

      // Phase 2: Create terminal
      setLoading(false)
      terminal = new XTerm({
        cursorBlink: true,
        fontSize: 14,
        fontFamily: 'Menlo, Monaco, "Courier New", monospace',
        scrollback: 50000,
        theme: {
          background: config.themeBackground,
          foreground: '#d4d4d4',
          cursor: '#d4d4d4',
          cursorAccent: config.themeBackground,
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

      terminal.open(containerRef.current)

      const resize = tty.getState().resize

      terminalRef.current = terminal

      // Create activity state detector with optional custom patterns
      // Skip when LLM-based analysis handles state (disableActivityDetector)
      detector = config.disableActivityDetector
        ? null
        : createActivityStateDetector(
            (state) => setTabState(tabId, state),
            config.promptPatterns ? { promptPatterns: config.promptPatterns } : undefined
          )
      // Notify parent that terminal is ready
      config.onTerminalReady?.(terminal, dataVersionRef)

      // Restore scrollback buffer
      if (scrollback && scrollback.length > 0) {
        console.log(`[${config.logPrefix} ${tabId}] restoring ${scrollback.length} scrollback chunks`)
        for (const chunk of scrollback) {
          terminal.write(chunk)
        }
      } else {
        setOverlay({ message: 'No scrollback buffer available', type: 'info' })
      }

      // If session already exited, show exit message and don't subscribe for live data
      if (exitCode !== undefined) {
        terminal.write(`\r\n\x1b[2mProcess exited with exit code ${exitCode}\x1b[0m\r\n`)
        setupResizeObserver(terminal, resize)
        return
      }

      // Phase 3: Connect live TTY
      ttyRef.current = tty
      const ttyState = tty.getState()
      const connectedAt = Date.now()

      const unsubscribeData = ttyState.onData((data) => {
        // Track data version for terminal analyzer
        dataVersionRef.current++
        // Dismiss overlay once live data starts flowing
        setOverlay(null)
        // Strip CSI 3J (clear scrollback) if configured
        const dataToWrite = config.stripScrollbackClear
          ? data.replace(/\x1b\[3J/g, '')
          : data
        // Detect ANSI sequences that manipulate scrollback/screen
        const scrollMatches = detectScrollManipulation(data)
        if (scrollMatches.length > 0) {
          const bufBefore = {
            baseY: terminal!.buffer.active.baseY,
            viewportY: terminal!.buffer.active.viewportY,
            length: terminal!.buffer.active.length,
          }
          terminal!.write(dataToWrite)
          const bufAfter = {
            baseY: terminal!.buffer.active.baseY,
            viewportY: terminal!.buffer.active.viewportY,
            length: terminal!.buffer.active.length,
          }
          for (const { name, match } of scrollMatches) {
            console.warn(`[SCROLL-MANIP] ${name}`, {
              rawSequence: formatRawChars(match),
              dataContext: formatRawChars(data.slice(0, 200)),
              bufferBefore: bufBefore,
              bufferAfter: bufAfter,
            })
          }
        } else {
          terminal!.write(dataToWrite)
        }
        // Process data for activity state detection
        if (detector) detector.processData(data)
        // Log raw characters to console for debugging
        if (settings.terminal.showRawChars) {
          rawChars = (rawChars + data).slice(-50)
          console.log('[RAW]', formatRawChars(rawChars))
        }
      })

      const unsubscribeExit = ttyState.onExit((exitCode) => {
        console.log(`[${config.logPrefix} ${tabId}] PTY exited with code:`, exitCode)
        if (!cancelled) {
          const currentTab = wsData?.appStates[tabId]
          const keepOnExit = (currentTab?.state as BaseTerminalState | undefined)?.keepOnExit
          const immediateFailure = exitCode !== 0 && (Date.now() - connectedAt) < 1000
          if (immediateFailure) {
            setOverlay({ message: `Process exited immediately with code ${exitCode}`, type: 'error' })
          } else if (keepOnExit) {
            terminal!.write(`\r\n\x1b[2mProcess exited with exit code ${exitCode}\x1b[0m\r\n`)
          } else {
            removeTab(tabId)
          }
        }
      })

      unsubscribe = () => {
        unsubscribeData()
        unsubscribeExit()
      }

      // Forward terminal input to PTY
      inputDisposable = terminal.onData((data) => {
        ttyRef.current!.getState().write(data)
      })

      setupResizeObserver(terminal, resize)

      // Debounce initial resize to get settled container dimensions
      if (resizeTimeout) clearTimeout(resizeTimeout)
      resizeTimeout = setTimeout(() => {
        fitTerminal(terminal!, resize)
        console.log(`[${config.logPrefix} ${tabId}] resize (initial):`, { cols: terminal!.cols, rows: terminal!.rows })
        initialResizeDone = true
      }, 100)
    }

    init()

    // Cleanup - DON'T kill PTY here, just unsubscribe
    // PTY is explicitly killed in removeWorkspace/removeTab
    return () => {
      console.log(`[${config.logPrefix} ${tabId}] cleanup running (PTY preserved):`, {
        ptyId: ttyRef.current?.getState().ptyId,
        workspaceId
      })
      cancelled = true
      if (resizeTimeout) clearTimeout(resizeTimeout)
      inputDisposable?.dispose()
      resizeObserver?.disconnect()
      unsubscribe?.()
      detector?.destroy()
      terminalRef.current = null
      ttyRef.current = null
      // Note: We intentionally don't kill the PTY here
      // The PTY lifecycle is managed by removeWorkspace/removeTab in workspace.ts
      terminal?.dispose()
    }
    // Note: existingPtyId is intentionally NOT in deps - we only check it on mount/re-run
  }, [tabId, workspaceId, config.themeBackground])

  const openContextMenu = useContextMenuStore((s) => s.open)
  const closeContextMenu = useContextMenuStore((s) => s.close)
  const contextMenuId = `terminal-${tabId}`

  const handleContextMenu = (e: React.MouseEvent) => {
    e.preventDefault()
    openContextMenu(contextMenuId, e.clientX, e.clientY)
  }

  const handleCopy = () => {
    const selection = terminalRef.current?.getSelection()
    console.log(`[${config.logPrefix} ${tabId}] copy:`, { selection: selection ?? '(no selection)', hasTerminal: !!terminalRef.current })
    if (selection) {
      clipboard.writeText(selection)
    }
    closeContextMenu()
  }

  const handlePaste = () => {
    const text = clipboard.readText()
    console.log(`[${config.logPrefix} ${tabId}] paste:`, { clipboardText: text ?? '(empty)', hasTty: !!ttyRef.current })
    if (text && ttyRef.current) {
      ttyRef.current.getState().write(text)
    }
    closeContextMenu()
  }

  const handleReflow = () => {
    const terminal = terminalRef.current
    if (terminal && ttyRef.current) {
      // Force reflow by resizing to 1 col narrower then back
      const { cols, rows } = terminal
      const resize = ttyRef.current.getState().resize
      terminal.resize(cols - 1, rows)
      terminal.resize(cols, rows)
      resize(cols, rows)
    }
  }

  const floatingButtons = (
    <>
      {extraButtons}
      <button className="reflow-btn" onClick={handleReflow} title="Reflow terminal">
        ⇔
      </button>
    </>
  )

  return (
    <TerminalScrollWrapper
      terminalRef={terminalRef}
      extraButtons={floatingButtons}
    >
      <div className="terminal-padding-wrapper">
        <div
          ref={containerRef}
          className={`terminal-container${config.disableScrollbar ? ' disable-scrollbar' : ''}`}
          onContextMenu={handleContextMenu}
        />
      </div>

      {loading && (
        <div className="terminal-overlay terminal-overlay-info">
          Loading terminal...
        </div>
      )}

      {overlay && (
        <div
          className={`terminal-overlay terminal-overlay-${overlay.type}`}
          onClick={() => setOverlay(null)}
        >
          {overlay.message}
        </div>
      )}

      <ContextMenu menuId={contextMenuId}>
        <div className="context-menu-item" onClick={handleCopy}>
          Copy
        </div>
        <div className="context-menu-item" onClick={handlePaste}>
          Paste
        </div>
      </ContextMenu>
    </TerminalScrollWrapper>
  )
}
