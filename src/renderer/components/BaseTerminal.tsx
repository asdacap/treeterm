import { useCallback, useEffect, useRef, useState } from 'react'
import { Terminal as XTerm } from '@xterm/xterm'
import { useStore } from 'zustand'
import { fitTerminal } from '../utils/fitTerminal'
import { useSettingsStore } from '../store/settings'
import { useAppStore } from '../store/app'
import { useActivityStateStore } from '../store/activityState'
import { useSessionApi } from '../contexts/SessionStoreContext'
import { createActivityStateDetector } from '../utils/activityStateDetector'
import type { Tty } from '../store/createTtyStore'
import type { SandboxConfig, TerminalState, WorkspaceStore } from '../types'
import { useContextMenuStore } from '../store/contextMenu'
import ContextMenu from './ContextMenu'
import '@xterm/xterm/css/xterm.css'

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
  const [scrollPosition, setScrollPosition] = useState<'top' | 'bottom' | 'middle'>('middle')
  const [isAlternateScreen, setIsAlternateScreen] = useState(false)
  const [sizeMismatch, setSizeMismatch] = useState<{ requested: { cols: number; rows: number }; actual: { cols: number; rows: number } } | null>(null)
  const requestedSizeRef = useRef<{ cols: number; rows: number } | null>(null)

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
    let scrollDisposable: { dispose(): void } | null = null
    let bufferChangeDisposable: { dispose(): void } | null = null
    let resizeObserver: ResizeObserver | null = null
    let detector: ReturnType<typeof createActivityStateDetector> | null = null
    let unsubscribe: (() => void) | null = null
    let unsubscribeFocus: (() => void) | null = null
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
      try {
        const result = await session.openTtyStream(existingPtyId)
        console.log(`[${config.logPrefix} ${tabId}] reattached to session:`, existingPtyId)
        tty = result.tty
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
        cursorBlink: settings.terminal.cursorBlink,
        cursorStyle: settings.terminal.cursorStyle,
        fontSize: settings.terminal.fontSize,
        fontFamily: settings.terminal.fontFamily,
        scrollback: 50000,
        linkHandler: {
          activate: (_event, uri) => window.open(uri, '_blank')
        },
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

      scrollDisposable = terminal.onScroll(() => {
        const buf = terminal!.buffer.active
        if (buf.baseY === 0) {
          setScrollPosition('middle')
        } else if (buf.viewportY === 0) {
          setScrollPosition('top')
        } else if (buf.baseY - buf.viewportY <= 1) {
          setScrollPosition('bottom')
        } else {
          setScrollPosition('middle')
        }
      })

      bufferChangeDisposable = terminal.buffer.onBufferChange((buf) => {
        setIsAlternateScreen(buf.type === 'alternate')
      })

      const rawResize = tty.getState().resize
      const resize = (cols: number, rows: number) => {
        requestedSizeRef.current = { cols, rows }
        rawResize(cols, rows)
      }

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

      // Subscribe to focus requests — focuses terminal when focusTabId matches
      unsubscribeFocus = workspace.subscribe((state) => {
        if (state.focusTabId === tabId && terminalRef.current) {
          terminalRef.current.focus()
          state.clearFocusRequest()
        }
      })
      // Check if focus was already requested before subscribing
      const wsState = workspace.getState()
      if (wsState.focusTabId === tabId) {
        terminal.focus()
        wsState.clearFocusRequest()
      }

      setupResizeObserver(terminal, resize)
      // Wait for layout to settle before fitting and connecting the stream.
      // All events (scrollback, resize, exit, live data) flow through onEvent uniformly —
      // the preload layer buffers them until we subscribe.
      if (resizeTimeout) clearTimeout(resizeTimeout)
      resizeTimeout = setTimeout(() => {
        if (cancelled) return
        fitTerminal(terminal!, resize)
        console.log(`[${config.logPrefix} ${tabId}] resize (initial):`, { cols: terminal!.cols, rows: terminal!.rows })
        initialResizeDone = true

        // Connect TTY — buffered events (scrollback, resize, possibly exit) flush immediately
        ttyRef.current = tty
        const ttyState = tty.getState()
        const connectedAt = Date.now()

        unsubscribe = ttyState.onEvent((event) => {
          switch (event.type) {
            case 'data': {
              dataVersionRef.current++
              setOverlay(null)

              const buf = terminal!.buffer.active
              const wasAtBottom = buf.baseY - buf.viewportY <= 1

              if (config.stripScrollbackClear) {
                const decoder = new TextDecoder('utf-8', { fatal: false })
                const dataStr = decoder.decode(event.data)
                const stripped = dataStr.replace(/\x1b\[3J/g, '')
                terminal!.write(stripped)
              } else {
                terminal!.write(event.data)
              }

              if (wasAtBottom && terminal!.buffer.active.baseY - terminal!.buffer.active.viewportY > 1) {
                terminal!.scrollToBottom()
              }

              setIsAlternateScreen(terminal!.buffer.active.type === 'alternate')

              if (detector) {
                const decoder = new TextDecoder('utf-8', { fatal: false })
                detector.processData(decoder.decode(event.data))
              }
              if (settings.terminal.showRawChars) {
                const decoder = new TextDecoder('utf-8', { fatal: false })
                rawChars = (rawChars + decoder.decode(event.data)).slice(-50)
                console.log('[RAW]', formatRawChars(rawChars))
              }
              break
            }
            case 'exit': {
              console.log(`[${config.logPrefix} ${tabId}] PTY exited with code:`, event.exitCode)
              if (!cancelled) {
                const currentTab = wsData?.appStates[tabId]
                const keepOnExit = (currentTab?.state as BaseTerminalState | undefined)?.keepOnExit
                const immediateFailure = event.exitCode !== 0 && (Date.now() - connectedAt) < 1000
                if (immediateFailure) {
                  setOverlay({ message: `Process exited immediately with code ${event.exitCode}`, type: 'error' })
                } else if (keepOnExit) {
                  terminal!.write(`\r\n\x1b[2mProcess exited with exit code ${event.exitCode}\x1b[0m\r\n`)
                } else {
                  removeTab(tabId)
                }
              }
              break
            }
            case 'resize': {
              const buf = terminal!.buffer.active
              const prevViewportY = buf.viewportY
              const prevBaseY = buf.baseY
              const wasAtBottom = prevBaseY - prevViewportY <= 3
              const scrollRatio = prevBaseY > 0 ? prevViewportY / prevBaseY : 0

              terminal!.resize(event.cols, event.rows)

              // Check if daemon-echoed size matches what we requested
              const req = requestedSizeRef.current
              if (req && (req.cols !== event.cols || req.rows !== event.rows)) {
                setSizeMismatch({ requested: req, actual: { cols: event.cols, rows: event.rows } })
              } else {
                setSizeMismatch(null)
              }

              if (wasAtBottom) {
                terminal!.scrollToBottom()
              } else {
                const newScrollLine = Math.round(terminal!.buffer.active.baseY * scrollRatio)
                terminal!.scrollToLine(newScrollLine)
              }
              break
            }
          }
        })

        // Forward terminal input to PTY
        inputDisposable = terminal!.onData((data) => {
          ttyRef.current!.getState().write(data)
        })
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
      scrollDisposable?.dispose()
      bufferChangeDisposable?.dispose()
      resizeObserver?.disconnect()
      unsubscribe?.()
      unsubscribeFocus?.()
      detector?.destroy()
      terminalRef.current = null
      ttyRef.current = null
      // Note: We intentionally don't kill the PTY here
      // The PTY lifecycle is managed by removeWorkspace/removeTab in workspace.ts
      terminal?.dispose()
    }
    // Note: existingPtyId is intentionally NOT in deps - we only check it on mount/re-run
  }, [tabId, workspaceId, config.themeBackground, settings])

  const handleScrollDown = useCallback(() => {
    terminalRef.current?.scrollToBottom()
  }, [terminalRef])

  const handleScrollToTop = useCallback(() => {
    terminalRef.current?.scrollToTop()
  }, [terminalRef])

  const handleBadgeClick = scrollPosition === 'bottom' ? handleScrollToTop : handleScrollDown

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

  const handlePaste = async () => {
    const text = await clipboard.readText()
    console.log(`[${config.logPrefix} ${tabId}] paste:`, { clipboardText: text ?? '(empty)', hasTty: !!ttyRef.current })
    if (text && ttyRef.current) {
      ttyRef.current.getState().write(text)
    }
    closeContextMenu()
  }

  return (
    <div className="terminal-wrapper">
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

      {settings.debug.showBadge && (
        <span className="debug-badge">DEBUG</span>
      )}

      <ContextMenu menuId={contextMenuId}>
        <div className="context-menu-item" onClick={handleCopy}>
          Copy
        </div>
        <div className="context-menu-item" onClick={handlePaste}>
          Paste
        </div>
      </ContextMenu>

      <button
        className={`scroll-position-badge scroll-position-${scrollPosition}`}
        onClick={handleBadgeClick}
        title={scrollPosition === 'bottom' ? 'Scroll to top' : 'Scroll to bottom'}
      >
        {scrollPosition.toUpperCase()}
      </button>
      {isAlternateScreen && (
        <span className="alt-screen-badge" title="Terminal is in alternate screen mode (no scrollback)">
          ALT SCREEN
        </span>
      )}
      {sizeMismatch && (
        <span
          className="size-mismatch-badge"
          title={`Requested: ${sizeMismatch.requested.cols}x${sizeMismatch.requested.rows}, Stream: ${sizeMismatch.actual.cols}x${sizeMismatch.actual.rows}`}
        >
          {sizeMismatch.actual.cols}x{sizeMismatch.actual.rows}
        </span>
      )}
      <div className="terminal-floating-buttons">
        {extraButtons}
        <button className="scroll-down-btn terminal-circle-btn" onClick={handleScrollDown} title="Scroll to bottom">
          ↓
        </button>
      </div>
    </div>
  )
}
