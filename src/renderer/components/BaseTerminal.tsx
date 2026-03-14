import { useEffect, useRef, useState } from 'react'
import { Terminal as XTerm } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import { useWorkspaceStore } from '../store/workspace'
import { useSettingsStore } from '../store/settings'
import { useActivityStateStore } from '../store/activityState'
import { createActivityStateDetector } from '../utils/activityStateDetector'
import TerminalScrollWrapper from './TerminalScrollWrapper'
import type { SandboxConfig, TerminalState } from '../types'
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
  // Startup command to run when creating new PTY (optional)
  startupCommand?: string
  // Log prefix for console messages
  logPrefix: string
  // Whether to show push-to-talk button
  showPushToTalk?: boolean
  // Whether to disable the scrollbar (for tools with own scrolling like opencode)
  disableScrollbar?: boolean
}

interface BaseTerminalProps {
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
  const [contextMenu, setContextMenu] = useState<ContextMenu | null>(null)

  const workspace = useWorkspaceStore((state) => state.workspaces[workspaceId])
  const updateTabState = useWorkspaceStore((state) => state.updateTabState)
  const removeTab = useWorkspaceStore((state) => state.removeTab)
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

    // Helper to subscribe to PTY and set up refs
    const connectToPty = (id: string) => {
      ptyIdRef.current = id
      const unsubscribeData = window.electron.terminal.onData(id, (data) => {
        terminal.write(data)
        // Process data for activity state detection
        detector.processData(data)
        // Log raw characters to console for debugging
        if (settings.terminal.showRawChars) {
          rawCharsRef.current = (rawCharsRef.current + data).slice(-50)
          console.log('[RAW]', formatRawChars(rawCharsRef.current))
        }
      })

      const unsubscribeExit = window.electron.terminal.onExit(id, (exitCode) => {
        console.log(`[${config.logPrefix} ${tabId}] PTY exited with code:`, exitCode)
        if (isMountedRef.current) {
          removeTab(workspaceId, tabId)
        }
      })

      unsubscribeRef.current = () => {
        unsubscribeData()
        unsubscribeExit()
      }

      console.log(`[${config.logPrefix} ${tabId}] initial PTY resize:`, {
        ptyId: id,
        cols: terminal.cols,
        rows: terminal.rows
      })
      window.electron.terminal.resize(id, terminal.cols, terminal.rows)
    }

    // Try to reconnect to existing PTY, or create a new one
    const initPty = async () => {
      // Try to attach to existing session (daemon mode)
      if (existingPtyId) {
        try {
          const result = await window.electron.terminal.attach(existingPtyId)
          if (result.success) {
            console.log(`[${config.logPrefix} ${tabId}] reattached to session:`, existingPtyId)
            if (!isMountedRef.current) return

            // Restore scrollback buffer
            if (result.scrollback && result.scrollback.length > 0) {
              console.log(`[${config.logPrefix} ${tabId}] restoring ${result.scrollback.length} scrollback chunks`)
              for (const chunk of result.scrollback) {
                terminal.write(chunk)
              }
            }

            connectToPty(existingPtyId)
            return
          }
        } catch (error) {
          console.log(`[${config.logPrefix} ${tabId}] failed to attach, trying isAlive:`, error)
        }

        // Fallback: check if PTY is alive (legacy mode or attach failed)
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

      // Helper to log scroll debug info
      const logScrollDebug = (label: string) => {
        const xtermViewport = containerRef.current?.querySelector('.xterm-viewport') as HTMLElement
        if (!xtermViewport) return

        const viewportY = terminal.buffer.active.viewportY
        const baseY = terminal.buffer.active.baseY
        const bufferLength = terminal.buffer.active.length
        const rows = terminal.rows

        const scrollTop = xtermViewport.scrollTop
        const scrollHeight = xtermViewport.scrollHeight
        const clientHeight = xtermViewport.clientHeight

        if (clientHeight === 0 || scrollHeight === 0) return

        const expectedScrollbarHeight = (clientHeight / scrollHeight) * clientHeight
        const ratio = (clientHeight / scrollHeight) * 100

        console.log(`[${config.logPrefix} ${tabId}] scroll debug (${label}):`, {
          xtermBuffer: { viewportY, baseY, bufferLength, rows },
          viewportScroll: {
            scrollTop: Math.round(scrollTop),
            scrollHeight: Math.round(scrollHeight),
            clientHeight: Math.round(clientHeight)
          },
          scrollbar: {
            expectedHeight: Math.round(expectedScrollbarHeight),
            ratio: ratio.toFixed(1) + '%'
          }
        })
      }

      // Log before resize
      logScrollDebug('before resize')

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
        console.log(`[${config.logPrefix} ${tabId}] scroll preservation (not at bottom):`, {
          before: { viewportY: prevViewportY, baseY: prevBaseY },
          wasAtBottom,
          scrollRatio,
          after: { baseY: terminal.buffer.active.baseY, newScrollLine },
          action: 'scrollToLine'
        })
        terminal.scrollToLine(newScrollLine)
      }

      // Log after resize
      logScrollDebug('after resize')

      console.log(`[${config.logPrefix} ${tabId}] resize detected:`, {
        containerSize: { width, height },
        terminalSize: { cols: terminal.cols, rows: terminal.rows },
        scrollPreserved: { prevViewportY, wasAtBottom, scrollRatio, newScrollLine: Math.round(terminal.buffer.active.baseY * scrollRatio) }
      })
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
      if (text && ptyIdRef.current) {
        window.electron.terminal.write(ptyIdRef.current, text)
      }
    } catch (error) {
      console.error(`[${config.logPrefix} ${tabId}] Failed to read from clipboard:`, error)
    } finally {
      setContextMenu(null)
    }
  }

  const handlePushToTalkTranscript = (text: string) => {
    if (ptyIdRef.current) {
      window.electron.terminal.write(ptyIdRef.current, text)
    }
  }

  const handlePushToTalkSubmit = () => {
    if (ptyIdRef.current) {
      window.electron.terminal.write(ptyIdRef.current, '\r')
    }
  }

  return (
    <TerminalScrollWrapper
      terminalRef={terminalRef}
      showPushToTalk={config.showPushToTalk}
      onPushToTalkTranscript={handlePushToTalkTranscript}
      onPushToTalkSubmit={handlePushToTalkSubmit}
      workspacePath={cwd}
      ptyId={ptyIdRef.current || undefined}
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
