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
import { ScrollPosition } from '../types'
import type { CachedTerminal, TerminalAppRef, PtyEvent, SandboxConfig, TerminalState, WorkspaceStore } from '../types'
import { useContextMenuStore } from '../store/contextMenu'
import ContextMenu from './ContextMenu'
import '@xterm/xterm/css/xterm.css'

const WRITE_CHUNK_SIZE = 1024

function writeChunked(tty: { write(data: string): void }, data: string): void {
  for (let i = 0; i < data.length; i += WRITE_CHUNK_SIZE) {
    tty.write(data.slice(i, i + WRITE_CHUNK_SIZE))
  }
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
      result += str[i] ?? ''
    }
  }
  return result
}

/**
 * Background event handler for cached terminals.
 * Dispatches to mountedHandler when the component is mounted,
 * falls back to minimal buffer writes when unmounted.
 */
function handleCachedEvent(cache: CachedTerminal, event: PtyEvent): void {
  // When component is mounted, forward everything to the full UI handler
  if (cache.mountedHandler) {
    cache.mountedHandler(event)
    return
  }

  // Unmounted fallback: keep terminal buffer up to date
  switch (event.type) {
    case 'data': {
      cache.dataVersion++
      if (cache.stripScrollbackClear) {
        const decoder = new TextDecoder('utf-8', { fatal: false })
        const stripped = decoder.decode(event.data).replace(/\x1b\[3J/g, '')
        cache.terminal.write(stripped)
      } else {
        cache.terminal.write(event.data)
      }
      break
    }
    case 'resize':
      cache.terminal.resize(event.cols, event.rows)
      break
    case 'exit':
      cache.onExitUnmounted(event.exitCode)
      break
    case 'error':
    case 'end':
      // Store for overlay display on next mount — handled by mountedHandler when re-mounted
      break
  }
}

const TERMINAL_THEME_COLORS = {
  foreground: '#d4d4d4',
  cursor: '#d4d4d4',
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
} as const

// Base state interface that all terminal-based states should extend
export interface BaseTerminalState extends TerminalState {
  sandbox?: SandboxConfig
}

export interface BaseTerminalConfig {
  // Theme customization
  themeBackground: string
  // Log prefix for console messages
  logPrefix: string
  // Whether to disable the scrollbar (for tools with own scrolling like opencode)
  disableScrollbar?: boolean
  // Whether to strip CSI 3J (clear scrollback) from PTY data before writing to xterm
  stripScrollbackClear?: boolean
  // Whether to disable the regex-based activity state detector (e.g. when using LLM-based analysis)
  disableActivityDetector?: boolean
  // Callback when terminal is ready, provides terminal instance
  onTerminalReady?: (terminal: XTerm) => void
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
  const wsData = useStore(workspace, s => s.workspace)
  const removeTab = useStore(workspace, s => s.removeTab)
  const workspaceId = wsData.id
  const containerRef = useRef<HTMLDivElement>(null)
  const terminalRef = useRef<XTerm | null>(null)
  const ttyRef = useRef<Tty | null>(null)
  const [overlay, setOverlay] = useState<{ message: string; type: 'info' | 'error' } | null>(null)
  const [loading, setLoading] = useState(true)
  const [scrollPosition, setScrollPosition] = useState(ScrollPosition.Bottom)
  const scrollPositionRef = useRef(ScrollPosition.Bottom)
  const [pinnedToBottom, setPinnedToBottom] = useState(false)
  const [isAlternateScreen, setIsAlternateScreen] = useState(false)
  const [sizeMismatch, setSizeMismatch] = useState<{ requested: { cols: number; rows: number }; actual: { cols: number; rows: number } } | null>(null)
  const [refreshCounter, setRefreshCounter] = useState(0)

  const sessionStore = useSessionApi()
  const setTabState = useActivityStateStore((state) => state.setTabState)
  const settings = useSettingsStore((state) => state.settings)
  const clipboard = useAppStore((state) => state.clipboard)
  const openExternal = useAppStore((state) => state.openExternal)

  // Get existing ptyId from store for reconnection
  // eslint-disable-next-line @typescript-eslint/no-non-null-assertion -- tabId guaranteed to exist in appStates
  const appState = wsData.appStates[tabId]!
  const existingPtyId = (appState.state as BaseTerminalState | undefined)?.ptyId
  const existingPtyIdRef = useRef(existingPtyId)
  existingPtyIdRef.current = existingPtyId

  useEffect(() => {
    const currentExistingPtyId = existingPtyIdRef.current
    if (!containerRef.current) return

    let cancelled = false
    let resizeTimeout: ReturnType<typeof setTimeout> | null = null
    let inputDisposable: { dispose(): void } | null = null
    let scrollDisposable: { dispose(): void } | null = null
    let bufferChangeDisposable: { dispose(): void } | null = null
    let resizeObserver: ResizeObserver | null = null
    let detector: ReturnType<typeof createActivityStateDetector> | null = null
    let unsubscribeFocus: (() => void) | null = null
    let rawChars = ''
    let initialResizeDone = false
    let requestedSize: { cols: number; rows: number } | null = null

    const wsState = workspace.getState()
    const termAppRef = wsState.getTabRef(tabId) as TerminalAppRef | null
    const existingCache = termAppRef?.cachedTerminal ?? null

    /** Attach all DOM-level handlers to a terminal. Shared by first mount and remount. */
    const attachMountedState = (terminal: XTerm, tty: Tty, cache: CachedTerminal) => {
      terminalRef.current = terminal
      ttyRef.current = tty

      // eslint-disable-next-line @typescript-eslint/unbound-method
      const { resize: rawResize } = tty.getState()
      const resize = (cols: number, rows: number) => {
        requestedSize = { cols, rows }
        rawResize(cols, rows)
      }

      // Scroll tracking — listen on the DOM viewport element instead of terminal.onScroll,
      // because xterm.js suppresses onScroll for user-initiated scrolling (mouse wheel, touch).
      const viewportEl = terminal.element?.querySelector('.xterm-viewport')
      if (viewportEl) {
        const onViewportScroll = (): void => {
          const buf = terminal.buffer.active
          let pos: ScrollPosition
          if (buf.baseY === 0) {
            pos = ScrollPosition.Bottom
          } else if (buf.viewportY === 0) {
            pos = ScrollPosition.Top
          } else if (buf.baseY - buf.viewportY <= 1) {
            pos = ScrollPosition.Bottom
          } else {
            pos = ScrollPosition.Middle
          }
          scrollPositionRef.current = pos
          setScrollPosition(pos)

          // Pin enforcement: if pinned and not at bottom, force scroll back
          if (cache.pinnedToBottom && pos !== ScrollPosition.Bottom) {
            terminal.scrollToBottom()
          }
        }

        // Auto-unpin on intentional mouse wheel scroll up
        const onWheel = (e: Event): void => {
          const wheelEvent = e as WheelEvent
          if (wheelEvent.deltaY < 0 && cache.pinnedToBottom) {
            cache.pinnedToBottom = false
            setPinnedToBottom(false)
          }
        }

        viewportEl.addEventListener('scroll', onViewportScroll)
        viewportEl.addEventListener('wheel', onWheel)
        scrollDisposable = { dispose: () => {
          viewportEl.removeEventListener('scroll', onViewportScroll)
          viewportEl.removeEventListener('wheel', onWheel)
        } }
      }

      bufferChangeDisposable = terminal.buffer.onBufferChange((buf) => {
        setIsAlternateScreen(buf.type === 'alternate')
      })

      // Activity state detector
      detector = config.disableActivityDetector
        ? null
        : createActivityStateDetector(
            (state) => { setTabState(tabId, state); }
          )

      // Focus when this tab becomes active
      unsubscribeFocus = workspace.subscribe((state) => {
        if (state.workspace.activeTabId === tabId && terminalRef.current) {
          terminalRef.current.focus()
        }
      })
      if (workspace.getState().workspace.activeTabId === tabId) {
        terminal.focus()
      }

      // Set mounted handler — the background subscription forwards all events here
      cache.mountedHandler = (event: PtyEvent) => {
        switch (event.type) {
          case 'data': {
            cache.dataVersion++
            setOverlay(null)

            const shouldScrollToBottom = cache.pinnedToBottom || scrollPositionRef.current === ScrollPosition.Bottom

            const afterWrite = () => {
              if (shouldScrollToBottom) {
                terminal.scrollToBottom()
                scrollPositionRef.current = ScrollPosition.Bottom
                setScrollPosition(ScrollPosition.Bottom)
              }
            }

            if (cache.stripScrollbackClear) {
              const decoder = new TextDecoder('utf-8', { fatal: false })
              const dataStr = decoder.decode(event.data)
              const stripped = dataStr.replace(/\x1b\[3J/g, '')
              terminal.write(stripped, afterWrite)
            } else {
              terminal.write(event.data, afterWrite)
            }

            setIsAlternateScreen(terminal.buffer.active.type === 'alternate')

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
              // eslint-disable-next-line @typescript-eslint/no-non-null-assertion -- tabId guaranteed to exist in appStates
              const currentTab = workspace.getState().workspace.appStates[tabId]!
              const keepOnExit = (currentTab.state as BaseTerminalState | undefined)?.keepOnExit
              const immediateFailure = event.exitCode !== 0 && (Date.now() - cache.connectedAt) < 1000
              if (immediateFailure) {
                setOverlay({ message: `Process exited immediately with code ${String(event.exitCode)}`, type: 'error' })
              } else if (keepOnExit) {
                terminal.write(`\r\n\x1b[2mProcess exited with exit code ${String(event.exitCode)}\x1b[0m\r\n`)
              } else {
                void removeTab(tabId)
              }
            }
            break
          }
          case 'resize': {
            const shouldStayAtBottom = cache.pinnedToBottom || scrollPositionRef.current === ScrollPosition.Bottom
            const buf = terminal.buffer.active
            const scrollRatio = buf.baseY > 0 ? buf.viewportY / buf.baseY : 0

            terminal.resize(event.cols, event.rows)

            // Check if daemon-echoed size matches what we requested
            const req = requestedSize
            if (req && (req.cols !== event.cols || req.rows !== event.rows)) {
              setSizeMismatch({ requested: req, actual: { cols: event.cols, rows: event.rows } })
            } else {
              setSizeMismatch(null)
            }

            if (shouldStayAtBottom) {
              terminal.scrollToBottom()
              scrollPositionRef.current = ScrollPosition.Bottom
              setScrollPosition(ScrollPosition.Bottom)
            } else {
              const newScrollLine = Math.round(terminal.buffer.active.baseY * scrollRatio)
              terminal.scrollToLine(newScrollLine)
            }
            break
          }
          case 'error': {
            console.error(`[${config.logPrefix} ${tabId}] PTY stream error:`, event.message)
            setOverlay({ message: event.message, type: 'error' })
            break
          }
          case 'end': {
            console.log(`[${config.logPrefix} ${tabId}] PTY stream ended`)
            setOverlay((prev) => prev ?? { message: 'Terminal disconnected', type: 'error' })
            break
          }
        }
      }

      // ResizeObserver — gated on initialResizeDone to prevent resize during mount
      resizeObserver = new ResizeObserver(() => {
        if (!initialResizeDone) return
        fitTerminal(terminal, resize, getComputedStyle)
      })
      if (containerRef.current) {
        resizeObserver.observe(containerRef.current)
      }

      // Wait for layout to settle, then fit (only sends resize if size actually changed)
      if (resizeTimeout) clearTimeout(resizeTimeout)
      resizeTimeout = setTimeout(() => {
        if (cancelled) return
        fitTerminal(terminal, resize, getComputedStyle)
        console.log(`[${config.logPrefix} ${tabId}] resize (initial):`, { cols: terminal.cols, rows: terminal.rows })
        initialResizeDone = true
      }, 100)

      // Forward terminal input to PTY only when this tab is active.
      // Inactive tabs can't receive keystrokes so any onData during
      // replay is an auto-response (OSC 11, DA, etc.) — drop it.
      inputDisposable = terminal.onData((data) => {
        if (workspace.getState().workspace.activeTabId !== tabId) return
        if (ttyRef.current) {
          writeChunked(ttyRef.current.getState(), data)
        }
      })
    }

    if (existingCache) {
      // === REMOUNT PATH: reuse cached terminal ===
      console.log(`[${config.logPrefix} ${tabId}] remount from cache`)
      const terminal = existingCache.terminal

      // Apply current settings (font, cursor, theme may have changed)
      terminal.options.fontSize = settings.terminal.fontSize
      terminal.options.fontFamily = settings.terminal.fontFamily
      terminal.options.cursorBlink = settings.terminal.cursorBlink
      terminal.options.cursorStyle = settings.terminal.cursorStyle
      terminal.options.theme = {
        ...TERMINAL_THEME_COLORS,
        background: config.themeBackground,
        cursorAccent: config.themeBackground,
      }

      // Restore pinned state from cache
      setPinnedToBottom(existingCache.pinnedToBottom)

      // Reparent terminal element to the new container
      setLoading(false)
      if (terminal.element) {
        containerRef.current.appendChild(terminal.element)
        terminal.refresh(0, terminal.rows - 1)
      }

      attachMountedState(terminal, existingCache.tty, existingCache)
    } else {
      // === FIRST MOUNT PATH: create terminal and cache it ===
      console.log(`[${config.logPrefix} ${tabId}] first mount`, {
        existingPtyId: currentExistingPtyId,
        workspaceId
      })

      const session = sessionStore.getState()

      const init = async () => {
        if (!currentExistingPtyId) {
          setLoading(false)
          setOverlay({ message: 'No PTY available for this terminal', type: 'error' })
          return
        }

        if (!containerRef.current) return

        setLoading(false)
        const terminal = new XTerm({
          cursorBlink: settings.terminal.cursorBlink,
          cursorStyle: settings.terminal.cursorStyle,
          fontSize: settings.terminal.fontSize,
          fontFamily: settings.terminal.fontFamily,
          scrollback: 50000,
          linkHandler: {
            activate: (_event, uri) => { openExternal(uri) }
          },
          theme: {
            ...TERMINAL_THEME_COLORS,
            background: config.themeBackground,
            cursorAccent: config.themeBackground,
          }
        })

        terminal.open(containerRef.current)

        // Create cache entry — event handler is registered before the stream starts
        const cache: CachedTerminal = {
          terminal,
          tty: undefined as unknown as Tty,  // set after openTtyStream resolves
          unsubscribeEvents: () => {},
          mountedHandler: null,
          stripScrollbackClear: config.stripScrollbackClear ?? false,
          connectedAt: Date.now(),
          dataVersion: 0,
          pinnedToBottom: false,
          badgeClickTimer: null,
          onExitUnmounted: (exitCode: number) => {
            console.log(`[${config.logPrefix} ${tabId}] PTY exited while unmounted, code:`, exitCode)
            // eslint-disable-next-line @typescript-eslint/no-non-null-assertion -- tabId guaranteed to exist in appStates
            const currentTab = workspace.getState().workspace.appStates[tabId]!
            const keepOnExit = (currentTab.state as BaseTerminalState | undefined)?.keepOnExit
            if (keepOnExit) {
              terminal.write(`\r\n\x1b[2mProcess exited with exit code ${String(exitCode)}\x1b[0m\r\n`)
            } else {
              void workspace.getState().removeTab(tabId)
            }
          },
        }

        let tty: Tty
        try {
          const result = await session.openTtyStream(currentExistingPtyId, (event) => {
            handleCachedEvent(cache, event)
          })
          console.log(`[${config.logPrefix} ${tabId}] reattached to session:`, currentExistingPtyId)
          tty = result.tty
        } catch (error) {
          console.log(`[${config.logPrefix} ${tabId}] failed to attach to PTY:`, currentExistingPtyId, error)
          if (cancelled) return
          setOverlay({ message: `Failed to reattach terminal: ${error instanceof Error ? error.message : 'Unknown error'}`, type: 'error' })
          return
        }

        if (cancelled) return

        cache.tty = tty

        if (termAppRef) termAppRef.cachedTerminal = cache

        // Notify parent that terminal is ready (first mount only — handlers persist on the cached terminal)
        config.onTerminalReady?.(terminal)

        attachMountedState(terminal, tty, cache)
      }

      void init()
    }

    // Cleanup — detach mounted state but keep terminal + TTY subscription alive
    return () => {
      console.log(`[${config.logPrefix} ${tabId}] cleanup (terminal cached, PTY preserved)`)
      cancelled = true
      if (resizeTimeout) clearTimeout(resizeTimeout)

      // Disconnect mounted UI state
      inputDisposable?.dispose()
      scrollDisposable?.dispose()
      bufferChangeDisposable?.dispose()
      resizeObserver?.disconnect()
      unsubscribeFocus?.()
      detector?.destroy()

      // Clear mounted handler so background fallback takes over
      const currentRef = workspace.getState().getTabRef(tabId) as TerminalAppRef | null
      if (currentRef?.cachedTerminal) {
        currentRef.cachedTerminal.mountedHandler = null
        if (currentRef.cachedTerminal.badgeClickTimer !== null) {
          clearTimeout(currentRef.cachedTerminal.badgeClickTimer)
          currentRef.cachedTerminal.badgeClickTimer = null
        }
      }

      terminalRef.current = null
      ttyRef.current = null
      // Do NOT dispose terminal or unsubscribe TTY — they stay cached
    }
  }, [tabId, workspaceId, config, settings, removeTab, setTabState, sessionStore, workspace, refreshCounter, openExternal])

  const handleScrollDown = useCallback(() => {
    terminalRef.current?.scrollToBottom()
  }, [terminalRef])

  const handleScrollToTop = useCallback(() => {
    terminalRef.current?.scrollToTop()
  }, [terminalRef])

  const handleRefreshStream = useCallback(() => {
    const ref = workspace.getState().getTabRef(tabId) as TerminalAppRef | null
    ref?.disposeCachedTerminal()
    if (containerRef.current) {
      containerRef.current.innerHTML = ''
    }
    setLoading(true)
    setOverlay(null)
    setRefreshCounter((c) => c + 1)
  }, [tabId, workspace])

  const handleBadgeClick = scrollPosition === ScrollPosition.Bottom ? handleScrollToTop : handleScrollDown

  const openContextMenu = useContextMenuStore((s) => s.open)
  const closeContextMenu = useContextMenuStore((s) => s.close)
  const activeMenuId = useContextMenuStore((s) => s.activeMenuId)
  const menuPosition = useContextMenuStore((s) => s.position)
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
    console.log(`[${config.logPrefix} ${tabId}] paste:`, { clipboardText: text || '(empty)', hasTty: !!ttyRef.current })
    if (text && ttyRef.current) {
      writeChunked(ttyRef.current.getState(), text)
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
          onClick={() => { setOverlay(null); }}
        >
          {overlay.message}
        </div>
      )}

      {settings.debug.showBadge && (
        <span className="debug-badge">DEBUG</span>
      )}

      <ContextMenu menuId={contextMenuId} activeMenuId={activeMenuId} position={menuPosition}>
        <div className="context-menu-item" onClick={handleCopy}>
          Copy
        </div>
        <div className="context-menu-item" onClick={() => { void handlePaste(); }}>
          Paste
        </div>
      </ContextMenu>

      <button
        className={`scroll-position-badge scroll-position-${scrollPosition}${pinnedToBottom ? ' pinned' : ''}`}
        onClick={() => {
          if (pinnedToBottom) {
            // Unpin immediately, stay at bottom (normal scroll resumes)
            const ref = workspace.getState().getTabRef(tabId) as TerminalAppRef | null
            if (ref?.cachedTerminal) ref.cachedTerminal.pinnedToBottom = false
            setPinnedToBottom(false)
          } else {
            // Unpinned: use timer to distinguish single-click from double-click
            const ref = workspace.getState().getTabRef(tabId) as TerminalAppRef | null
            const cache = ref?.cachedTerminal
            if (cache) {
              if (cache.badgeClickTimer !== null) {
                // Second click within window — activate pin
                clearTimeout(cache.badgeClickTimer)
                cache.badgeClickTimer = null
                cache.pinnedToBottom = true
                setPinnedToBottom(true)
                terminalRef.current?.scrollToBottom()
              } else {
                // First click — delay to allow double-click
                cache.badgeClickTimer = setTimeout(() => {
                  cache.badgeClickTimer = null
                  handleBadgeClick()
                }, 250)
              }
            }
          }
        }}
        title={pinnedToBottom ? 'Pinned to bottom (click to unpin)' : (scrollPosition === ScrollPosition.Bottom ? 'Scroll to top (double-click to pin)' : 'Scroll to bottom (double-click to pin)')}
      >
        {pinnedToBottom ? 'PINNED' : scrollPosition.toUpperCase()}
      </button>
      {isAlternateScreen && (
        <span className="alt-screen-badge" title="Terminal is in alternate screen mode (no scrollback)">
          ALT SCREEN
        </span>
      )}
      {sizeMismatch && (
        <span
          className="size-mismatch-badge"
          title={`Requested: ${String(sizeMismatch.requested.cols)}x${String(sizeMismatch.requested.rows)}, Stream: ${String(sizeMismatch.actual.cols)}x${String(sizeMismatch.actual.rows)}`}
        >
          {sizeMismatch.actual.cols}x{sizeMismatch.actual.rows}
        </span>
      )}
      <div className="terminal-floating-buttons">
        {extraButtons}
        <button className="terminal-circle-btn" onClick={handleRefreshStream} title="Refresh stream">
          ↻
        </button>
        <button
          className="scroll-down-btn terminal-circle-btn"
          onClick={() => {
            terminalRef.current?.scrollToBottom()
          }}
          title="Scroll to bottom"
        >
          ↓
        </button>
      </div>
    </div>
  )
}
