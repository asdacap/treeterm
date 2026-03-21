import { useEffect, useRef, useState } from 'react'
import { Terminal as XTerm } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import { useStore } from 'zustand'
import { useSettingsStore } from '../store/settings'
import { useActivityStateStore } from '../store/activityState'
import { useSessionApi } from '../contexts/SessionStoreContext'
import { createActivityStateDetector } from '../utils/activityStateDetector'
import type { Tty } from '../store/createTtyStore'
import TerminalScrollWrapper from './TerminalScrollWrapper'
import type { SandboxConfig, TerminalState, WorkspaceStore } from '../types'
import { clampContextMenuPosition } from '../utils/contextMenuPosition'
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
  isVisible?: boolean
  config: BaseTerminalConfig
}

interface ContextMenu {
  x: number
  y: number
}

export default function BaseTerminal({
  workspace,
  tabId,
  isVisible,
  config,
}: BaseTerminalProps) {
  const { workspace: wsData, removeTab } = useStore(workspace)
  const workspaceId = wsData.id
  const containerRef = useRef<HTMLDivElement>(null)
  const terminalRef = useRef<XTerm | null>(null)
  const fitAddonRef = useRef<FitAddon | null>(null)
  const ttyRef = useRef<Tty | null>(null)
  const unsubscribeRef = useRef<(() => void) | null>(null)
  const detectorRef = useRef<ReturnType<typeof createActivityStateDetector> | null>(null)
  const rawCharsRef = useRef<string>('')
  const dataVersionRef = useRef(0)
  const [contextMenu, setContextMenu] = useState<ContextMenu | null>(null)
  const [overlay, setOverlay] = useState<{ message: string; type: 'info' | 'error' } | null>(null)

  const sessionStore = useSessionApi()
  const setTabState = useActivityStateStore((state) => state.setTabState)
  const settings = useSettingsStore((state) => state.settings)

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

    // Create terminal with configurable theme
    const terminal = new XTerm({
      cursorBlink: true,
      fontSize: 14,
      fontFamily: 'Menlo, Monaco, "Courier New", monospace',
      scrollback: 50000, // Increase scrollback buffer to handle long outputs
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

    const fitAddon = new FitAddon()
    terminal.loadAddon(fitAddon)
    terminal.open(containerRef.current)
    fitAddon.fit()
    console.log(`[${config.logPrefix} ${tabId}] initial terminal size:`, {
      cols: terminal.cols,
      rows: terminal.rows
    })

    terminalRef.current = terminal
    fitAddonRef.current = fitAddon

    // Notify parent that terminal is ready
    config.onTerminalReady?.(terminal, dataVersionRef)

    // Create activity state detector with optional custom patterns
    // Skip when LLM-based analysis handles state (disableActivityDetector)
    const detector = config.disableActivityDetector
      ? null
      : createActivityStateDetector(
          (state) => setTabState(tabId, state),
          config.promptPatterns ? { promptPatterns: config.promptPatterns } : undefined
        )
    detectorRef.current = detector

    // Helper to subscribe to Tty sub-store and set up refs
    const connectToTty = (tty: Tty) => {
      ttyRef.current = tty
      const ttyState = tty.getState()

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
            baseY: terminal.buffer.active.baseY,
            viewportY: terminal.buffer.active.viewportY,
            length: terminal.buffer.active.length,
          }
          terminal.write(dataToWrite)
          const bufAfter = {
            baseY: terminal.buffer.active.baseY,
            viewportY: terminal.buffer.active.viewportY,
            length: terminal.buffer.active.length,
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
          terminal.write(dataToWrite)
        }
        // Process data for activity state detection
        if (detector) detector.processData(data)
        // Log raw characters to console for debugging
        if (settings.terminal.showRawChars) {
          rawCharsRef.current = (rawCharsRef.current + data).slice(-50)
          console.log('[RAW]', formatRawChars(rawCharsRef.current))
        }
      })

      const unsubscribeExit = ttyState.onExit((exitCode) => {
        console.log(`[${config.logPrefix} ${tabId}] PTY exited with code:`, exitCode)
        if (!cancelled) {
          const currentTab = wsData?.appStates[tabId]
          const keepOnExit = (currentTab?.state as BaseTerminalState | undefined)?.keepOnExit
          if (keepOnExit) {
            terminal.write(`\r\n\x1b[2mProcess exited with exit code ${exitCode}\x1b[0m\r\n`)
          } else {
            removeTab(tabId)
          }
        }
      })

      unsubscribeRef.current = () => {
        unsubscribeData()
        unsubscribeExit()
      }

      console.log(`[${config.logPrefix} ${tabId}] initial PTY resize:`, {
        ptyId: ttyState.ptyId,
        cols: terminal.cols,
        rows: terminal.rows
      })
      ttyState.resize(terminal.cols, terminal.rows)
    }

    const session = sessionStore.getState()

    // Attach to existing PTY
    const initPty = async () => {
      if (!existingPtyId) {
        setOverlay({ message: 'No PTY available for this terminal', type: 'error' })
        return
      }

      try {
        const result = await session.attachTty(existingPtyId)
        console.log(`[${config.logPrefix} ${tabId}] reattached to session:`, existingPtyId)
        if (cancelled) return

        // Restore scrollback buffer
        if (result.scrollback && result.scrollback.length > 0) {
          console.log(`[${config.logPrefix} ${tabId}] restoring ${result.scrollback.length} scrollback chunks`)
          for (const chunk of result.scrollback) {
            terminal.write(chunk)
          }
        } else {
          setOverlay({ message: 'No scrollback buffer available', type: 'info' })
        }

        // If session already exited, show exit message and don't subscribe for live data
        if (result.exitCode !== undefined) {
          terminal.write(`\r\n\x1b[2mProcess exited with exit code ${result.exitCode}\x1b[0m\r\n`)
          return
        }

        const tty = session.getTty(existingPtyId)
        if (!tty) return
        connectToTty(tty)
      } catch (error) {
        console.log(`[${config.logPrefix} ${tabId}] failed to attach to PTY:`, existingPtyId, error)
        setOverlay({ message: `Failed to reattach terminal: ${error instanceof Error ? error.message : 'Unknown error'}`, type: 'error' })
      }
    }

    initPty()

    // Forward terminal input to PTY
    const inputDisposable = terminal.onData((data) => {
      if (ttyRef.current) {
        ttyRef.current.getState().write(data)
      }
    })

    // Handle resize
    const resizeObserver = new ResizeObserver((entries) => {
      const entry = entries[0]
      if (!entry) return

      // Skip resize when container is hidden (0x0 dimensions)
      const { width, height } = entry.contentRect
      if (width === 0 || height === 0) return

      // Save scroll position as ratio before fit to prevent scroll jumping
      const prevViewportY = terminal.buffer.active.viewportY
      const prevBaseY = terminal.buffer.active.baseY
      // Consider "at bottom" if within 3 lines of the bottom (accounts for partial scrolls)
      const wasAtBottom = prevBaseY - prevViewportY <= 3
      const scrollRatio = prevBaseY > 0 ? prevViewportY / prevBaseY : 0

      fitAddon.fit()

      // Restore scroll position after fit (unless user was at bottom, then stay at bottom)
      if (wasAtBottom) {
        terminal.scrollToBottom()
      } else {
        // Use ratio to calculate new scroll position after buffer reflow
        const newScrollLine = Math.round(terminal.buffer.active.baseY * scrollRatio)
        terminal.scrollToLine(newScrollLine)
      }

      if (ttyRef.current) {
        ttyRef.current.getState().resize(terminal.cols, terminal.rows)
      }
    })
    resizeObserver.observe(containerRef.current)

    // Cleanup - DON'T kill PTY here, just unsubscribe
    // PTY is explicitly killed in removeWorkspace/removeTab
    return () => {
      console.log(`[${config.logPrefix} ${tabId}] cleanup running (PTY preserved):`, {
        ptyId: ttyRef.current?.getState().ptyId,
        workspaceId
      })
      cancelled = true
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
  }, [tabId, workspaceId, config.themeBackground])

  // Refresh terminal when tab becomes visible to fix blank screen issue
  useEffect(() => {
    if (isVisible && terminalRef.current && fitAddonRef.current) {
      const terminal = terminalRef.current

      // Save scroll position as ratio before fit to prevent scroll jumping
      const prevViewportY = terminal.buffer.active.viewportY
      const prevBaseY = terminal.buffer.active.baseY
      // Consider "at bottom" if within 3 lines of the bottom (accounts for partial scrolls)
      const wasAtBottom = prevBaseY - prevViewportY <= 3
      const scrollRatio = prevBaseY > 0 ? prevViewportY / prevBaseY : 0

      // Re-fit and refresh the terminal when becoming visible
      fitAddonRef.current.fit()
      terminal.refresh(0, terminal.rows)

      // Restore scroll position after fit
      if (wasAtBottom) {
        terminal.scrollToBottom()
      } else {
        // Use ratio to calculate new scroll position after buffer reflow
        const newScrollLine = Math.round(terminal.buffer.active.baseY * scrollRatio)
        terminal.scrollToLine(newScrollLine)
      }

      // Focus the terminal so keyboard input works immediately
      terminal.focus()
    }
  }, [isVisible])


  // Close context menu when clicking outside
  useEffect(() => {
    if (!contextMenu) return
    const handleClick = (e: MouseEvent) => {
      // Don't close if clicking on the context menu itself
      const target = e.target as HTMLElement
      if (target.closest('.context-menu')) return
      setContextMenu(null)
    }
    // Use capture phase so the event is caught before xterm.js can stop propagation
    document.addEventListener('click', handleClick, true)
    return () => document.removeEventListener('click', handleClick, true)
  }, [contextMenu])

  const handleContextMenu = (e: React.MouseEvent) => {
    e.preventDefault()
    setContextMenu(clampContextMenuPosition(e.clientX, e.clientY))
  }

  const handleCopy = async () => {
    try {
      const selection = terminalRef.current?.getSelection()
      if (selection) {
        await navigator.clipboard.writeText(selection)
      }
    } catch (error) {
      console.error(`[${config.logPrefix} ${tabId}] Failed to write to clipboard:`, error)
    } finally {
      setContextMenu(null)
    }
  }

  const handlePaste = async () => {
    try {
      const text = await navigator.clipboard.readText()
      if (text && ttyRef.current) {
        ttyRef.current.getState().write(text)
      }
    } catch (error) {
      console.error(`[${config.logPrefix} ${tabId}] Failed to read from clipboard:`, error)
    } finally {
      setContextMenu(null)
    }
  }

  return (
    <TerminalScrollWrapper
      terminalRef={terminalRef}
    >
      <div className="terminal-padding-wrapper">
        <div
          ref={containerRef}
          className={`terminal-container${config.disableScrollbar ? ' disable-scrollbar' : ''}`}
          onContextMenu={handleContextMenu}
        />
      </div>

      {overlay && (
        <div
          className={`terminal-overlay terminal-overlay-${overlay.type}`}
          onClick={() => setOverlay(null)}
        >
          {overlay.message}
        </div>
      )}

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
