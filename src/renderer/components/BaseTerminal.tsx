import { useCallback, useEffect, useRef, useState } from 'react'
import { Terminal as XTerm } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import { useSettingsStore } from '../store/settings'
import { useActivityStateStore } from '../store/activityState'
import { useTerminalApi } from '../contexts/TerminalApiContext'
import { createActivityStateDetector } from '../utils/activityStateDetector'
import TerminalScrollWrapper from './TerminalScrollWrapper'
import type { SandboxConfig, TerminalState, WorkspaceHandle } from '../types'
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
  // Startup command to run when creating new PTY (optional)
  startupCommand?: string
  // Log prefix for console messages
  logPrefix: string
  // Whether to disable the scrollbar (for tools with own scrolling like opencode)
  disableScrollbar?: boolean
  // Whether to strip CSI 3J (clear scrollback) from PTY data before writing to xterm
  stripScrollbackClear?: boolean
}

interface BaseTerminalProps {
  cwd: string
  workspace: WorkspaceHandle
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
  workspace,
  tabId,
  sandbox,
  isVisible,
  config,
}: BaseTerminalProps) {
  const workspaceId = workspace.id
  const containerRef = useRef<HTMLDivElement>(null)
  const terminalRef = useRef<XTerm | null>(null)
  const fitAddonRef = useRef<FitAddon | null>(null)
  // handle is the ephemeral stream identifier used for write/resize/onData/onExit
  const handleRef = useRef<string | null>(null)
  // sessionId is the daemon PTY identifier used for persistence and kill
  const sessionIdRef = useRef<string | null>(null)
  const unsubscribeRef = useRef<(() => void) | null>(null)
  const detectorRef = useRef<ReturnType<typeof createActivityStateDetector> | null>(null)
  const isMountedRef = useRef(true)
  const rawCharsRef = useRef<string>('')
  const [contextMenu, setContextMenu] = useState<ContextMenu | null>(null)

  const terminalApi = useTerminalApi()
  const setTabState = useActivityStateStore((state) => state.setTabState)
  const settings = useSettingsStore((state) => state.settings)

  // Get existing ptyId from store for reconnection
  const wsData = workspace.data
  const appState = wsData?.appStates[tabId]
  const existingPtyId = (appState?.state as BaseTerminalState | undefined)?.ptyId

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
      scrollback: 50000, // Increase scrollback buffer to handle long outputs
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
    console.log(`[${config.logPrefix} ${tabId}] initial terminal size:`, {
      cols: terminal.cols,
      rows: terminal.rows
    })

    // Delayed fit to ensure proper dimensions after layout stabilizes
    // This helps fix scrollbar viewport calculation issues
    setTimeout(() => {
      if (isMountedRef.current && fitAddonRef.current && terminalRef.current) {
        const term = terminalRef.current

        // Save scroll position as ratio before fit to prevent scroll jumping
        const prevViewportY = term.buffer.active.viewportY
        const prevBaseY = term.buffer.active.baseY
        // Consider "at bottom" if within 3 lines of the bottom (accounts for partial scrolls)
        const wasAtBottom = prevBaseY - prevViewportY <= 3
        const scrollRatio = prevBaseY > 0 ? prevViewportY / prevBaseY : 0

        fitAddonRef.current.fit()

        // Restore scroll position after fit
        if (wasAtBottom) {
          term.scrollToBottom()
        } else {
          // Use ratio to calculate new scroll position after buffer reflow
          const newScrollLine = Math.round(term.buffer.active.baseY * scrollRatio)
          term.scrollToLine(newScrollLine)
        }

        console.log(`[${config.logPrefix} ${tabId}] delayed fit terminal size:`, {
          cols: term.cols,
          rows: term.rows
        })
      }
    }, 100)

    terminalRef.current = terminal
    fitAddonRef.current = fitAddon

    // Create activity state detector with optional custom patterns
    const detector = createActivityStateDetector(
      (state) => setTabState(tabId, state),
      config.promptPatterns ? { promptPatterns: config.promptPatterns } : undefined
    )
    detectorRef.current = detector

    // Helper to subscribe to PTY stream and set up refs
    const connectToPty = (handle: string) => {
      handleRef.current = handle
      const unsubscribeData = terminalApi.onData(handle, (data) => {
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
        detector.processData(data)
        // Log raw characters to console for debugging
        if (settings.terminal.showRawChars) {
          rawCharsRef.current = (rawCharsRef.current + data).slice(-50)
          console.log('[RAW]', formatRawChars(rawCharsRef.current))
        }
      })

      const unsubscribeExit = terminalApi.onExit(handle, (exitCode) => {
        console.log(`[${config.logPrefix} ${tabId}] PTY exited with code:`, exitCode)
        if (isMountedRef.current) {
          const currentTab = workspace.data?.appStates[tabId]
          const keepOnExit = (currentTab?.state as BaseTerminalState | undefined)?.keepOnExit
          if (keepOnExit) {
            terminal.write(`\r\n\x1b[2mProcess exited with exit code ${exitCode}\x1b[0m\r\n`)
          } else {
            workspace.removeTab(tabId)
          }
        }
      })

      unsubscribeRef.current = () => {
        unsubscribeData()
        unsubscribeExit()
      }

      console.log(`[${config.logPrefix} ${tabId}] initial PTY resize:`, {
        handle,
        cols: terminal.cols,
        rows: terminal.rows
      })
      terminalApi.resize(handle, terminal.cols, terminal.rows)
    }

    // Try to reconnect to existing PTY, or create a new one
    const initPty = async () => {
      // Try to attach to existing session (daemon mode)
      if (existingPtyId) {
        try {
          const result = await terminalApi.attach(existingPtyId)
          if (result.success && result.handle) {
            console.log(`[${config.logPrefix} ${tabId}] reattached to session:`, existingPtyId)
            if (!isMountedRef.current) return

            sessionIdRef.current = existingPtyId

            // Restore scrollback buffer
            if (result.scrollback && result.scrollback.length > 0) {
              console.log(`[${config.logPrefix} ${tabId}] restoring ${result.scrollback.length} scrollback chunks`)
              for (const chunk of result.scrollback) {
                terminal.write(chunk)
              }
            } else {
              terminal.write('\x1b[2mno buffer\x1b[0m\r\n')
            }

            // If session already exited, show exit message and don't subscribe for live data
            if (result.exitCode !== undefined) {
              terminal.write(`\r\n\x1b[2mProcess exited with exit code ${result.exitCode}\x1b[0m\r\n`)
              handleRef.current = result.handle
              return
            }

            connectToPty(result.handle)
            workspace.updateTabState<BaseTerminalState>(tabId, (state) => ({
              ...state,
              ptyHandle: result.handle!
            }))
            return
          }
        } catch (error) {
          console.log(`[${config.logPrefix} ${tabId}] failed to attach to PTY:`, existingPtyId, error)
          terminal.write(`\x1b[2merror attaching to tty: ${error instanceof Error ? error.message : 'Unknown error'}\x1b[0m\r\n`)
        }
      }

      // No existing PTY or it's dead - create a new one
      const result = await terminalApi.create(cwd, sandbox, config.startupCommand)
      if (!result) return

      // Check if component is still mounted
      if (!isMountedRef.current) {
        // Component unmounted during PTY creation - kill the orphaned PTY
        terminalApi.kill(result.sessionId)
        return
      }

      console.log(`[${config.logPrefix} ${tabId}] created new PTY:`, result.sessionId, 'handle:', result.handle)
      sessionIdRef.current = result.sessionId
      connectToPty(result.handle)
      workspace.updateTabState<BaseTerminalState>(tabId, (state) => ({
        ...state,
        ptyId: result.sessionId,
        ptyHandle: result.handle
      }))
    }

    initPty()

    // Forward terminal input to PTY
    const inputDisposable = terminal.onData((data) => {
      if (handleRef.current) {
        terminalApi.write(handleRef.current, data)
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

      if (handleRef.current) {
        terminalApi.resize(handleRef.current, terminal.cols, terminal.rows)
      }
    })
    resizeObserver.observe(containerRef.current)

    // Cleanup - DON'T kill PTY here, just unsubscribe
    // PTY is explicitly killed in removeWorkspace/removeTab
    return () => {
      console.log(`[${config.logPrefix} ${tabId}] cleanup running (PTY preserved):`, {
        handle: handleRef.current,
        sessionId: sessionIdRef.current,
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
  }, [cwd, tabId, sandbox?.enabled, workspaceId, config.startupCommand, config.themeBackground])

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
    setContextMenu({ x: e.clientX, y: e.clientY })
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
      if (text && handleRef.current) {
        terminalApi.write(handleRef.current, text)
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
      <div
        ref={containerRef}
        className={`terminal-container${config.disableScrollbar ? ' disable-scrollbar' : ''}`}
        onContextMenu={handleContextMenu}
      />

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
