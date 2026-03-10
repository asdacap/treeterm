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
  const [contextMenu, setContextMenu] = useState<ContextMenu | null>(null)
  const [scrollDebug, setScrollDebug] = useState({
    viewportY: 0,
    baseY: 0,
    bufferLength: 0,
    rows: 0,
    scrollTop: 0,
    scrollHeight: 0,
    clientHeight: 0,
    expectedScrollbarHeight: 0
  })

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

    // Delayed fit to ensure proper dimensions after layout stabilizes
    // This helps fix scrollbar viewport calculation issues
    setTimeout(() => {
      if (isMountedRef.current && fitAddonRef.current) {
        fitAddonRef.current.fit()
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
      // Focus the terminal so keyboard input works immediately
      terminalRef.current.focus()
    }
  }, [isVisible])

  // Debug scroll state - monitor xterm scroll properties
  useEffect(() => {
    const updateScrollDebug = () => {
      if (!terminalRef.current || !containerRef.current) return

      const terminal = terminalRef.current
      const xtermViewport = containerRef.current.querySelector('.xterm-viewport') as HTMLElement

      if (!xtermViewport) return

      const viewportY = terminal.buffer.active.viewportY
      const baseY = terminal.buffer.active.baseY
      const bufferLength = terminal.buffer.active.length
      const rows = terminal.rows

      const scrollTop = xtermViewport.scrollTop
      const scrollHeight = xtermViewport.scrollHeight
      const clientHeight = xtermViewport.clientHeight

      // Calculate expected scrollbar height based on viewport ratio
      const expectedScrollbarHeight = (clientHeight / scrollHeight) * clientHeight

      setScrollDebug({
        viewportY,
        baseY,
        bufferLength,
        rows,
        scrollTop,
        scrollHeight,
        clientHeight,
        expectedScrollbarHeight
      })
    }

    // Update on scroll events
    const xtermViewport = containerRef.current?.querySelector('.xterm-viewport') as HTMLElement
    if (xtermViewport) {
      xtermViewport.addEventListener('scroll', updateScrollDebug)
    }

    // Also update periodically in case of programmatic changes
    const interval = setInterval(updateScrollDebug, 100)

    // Initial update
    updateScrollDebug()

    return () => {
      clearInterval(interval)
      if (xtermViewport) {
        xtermViewport.removeEventListener('scroll', updateScrollDebug)
      }
    }
  }, [])

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
    >
      <div ref={containerRef} className="terminal-container" onContextMenu={handleContextMenu} />

      {/* Debug overlay for scroll state */}
      <div className="scroll-debug-overlay">
        <div className="scroll-debug-title">Scroll Debug</div>
        <div className="scroll-debug-section">
          <div className="scroll-debug-label">XTerm Buffer:</div>
          <div>viewportY: {scrollDebug.viewportY}</div>
          <div>baseY: {scrollDebug.baseY}</div>
          <div>bufferLength: {scrollDebug.bufferLength}</div>
          <div>rows: {scrollDebug.rows}</div>
        </div>
        <div className="scroll-debug-section">
          <div className="scroll-debug-label">Viewport Scroll:</div>
          <div>scrollTop: {scrollDebug.scrollTop.toFixed(0)}px</div>
          <div>scrollHeight: {scrollDebug.scrollHeight.toFixed(0)}px</div>
          <div>clientHeight: {scrollDebug.clientHeight.toFixed(0)}px</div>
        </div>
        <div className="scroll-debug-section">
          <div className="scroll-debug-label">Scrollbar:</div>
          <div>Expected height: {scrollDebug.expectedScrollbarHeight.toFixed(0)}px</div>
          <div>Ratio: {((scrollDebug.clientHeight / scrollDebug.scrollHeight) * 100).toFixed(1)}%</div>
        </div>
      </div>

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
