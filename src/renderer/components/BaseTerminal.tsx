import { useCallback, useEffect, useRef, useState } from 'react'
import { useStore } from 'zustand'
import { log } from '../utils/logger'
import { useSettingsStore } from '../store/settings'
import { useAppStore } from '../store/app'
import { useActivityStateStore } from '../store/activityState'
import { useSessionApi } from '../contexts/SessionStoreContext'
import { createActivityStateDetector } from '../utils/activityStateDetector'
import type { Tty } from '../store/createTtyStore'
import { ScrollPosition } from '../types'
import type { CachedTerminal, TerminalAppRef, PtyEvent, SandboxConfig, TerminalState, WorkspaceStore } from '../types'
import type { TerminalBufferHost, TerminalEngine, TerminalEngineFactory } from '../terminal/engine'
import { snapshotViewport } from '../terminal/engine'
import { PtyEventType } from '../../shared/ipc-types'
import { DisposableStore, thenRegisterOrDispose } from '../../shared/lifecycle'
import { useContextMenuStore } from '../store/contextMenu'
import ContextMenu from './ContextMenu'

const SCROLLBACK_LINES = 50000
/** Let the container's layout settle before the first fit. */
const INITIAL_FIT_DELAY_MS = 100

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
    case PtyEventType.Data: {
      cache.dataVersion++
      cache.engine.write(event.data)
      break
    }
    case PtyEventType.Resize:
      cache.engine.resize(event.cols, event.rows)
      break
    case PtyEventType.Exit:
      cache.onExitUnmounted(event.exitCode)
      break
    case PtyEventType.Error:
    case PtyEventType.End:
      // Store for overlay display on next mount — handled by mountedHandler when re-mounted
      break
  }
}

/**
 * Neither GPU renderer keeps a DOM row per line, so e2e can no longer scrape row elements for
 * text. The engine's terminal is published on its container element instead, and tests read
 * `terminal.buffer` through it. See `e2e/helpers.ts#getTerminalText`.
 */
export interface TerminalContainerElement extends HTMLDivElement {
  terminal?: TerminalBufferHost
}

// Base state interface that all terminal-based states should extend
export interface BaseTerminalState extends TerminalState {
  sandbox?: SandboxConfig
}

export interface BaseTerminalConfig {
  // Which terminal frontend backs this tab (xterm.js or ghostty-web)
  createEngine: TerminalEngineFactory
  // Theme customization
  themeBackground: string
  // Log prefix for console messages
  logPrefix: string
  // Whether to disable the scrollbar (for tools with own scrolling like opencode)
  disableScrollbar?: boolean
  // Whether to disable the regex-based activity state detector (e.g. when using LLM-based analysis)
  disableActivityDetector?: boolean
  // Callback when terminal is ready, provides the engine
  onTerminalReady?: (engine: TerminalEngine) => void
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
  const engineRef = useRef<TerminalEngine | null>(null)
  const ttyRef = useRef<Tty | null>(null)
  const [overlay, setOverlay] = useState<{ message: string; type: 'info' | 'error' } | null>(null)
  const [loading, setLoading] = useState(true)
  const [scrollPosition, setScrollPosition] = useState(ScrollPosition.Bottom)
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
    let wheelDisposable: { dispose(): void } | null = null
    let resizeObserver: ResizeObserver | null = null
    let detector: ReturnType<typeof createActivityStateDetector> | null = null
    let unsubscribeFocus: (() => void) | null = null
    let rawChars = ''
    let initialResizeDone = false
    let requestedSize: { cols: number; rows: number } | null = null

    const wsState = workspace.getState()
    const termAppRef = wsState.getTabRef(tabId) as TerminalAppRef | null
    const existingCache = termAppRef?.cachedTerminal ?? null

    const displayOptions = {
      fontSize: settings.terminal.fontSize,
      fontFamily: settings.terminal.fontFamily,
      cursorBlink: settings.terminal.cursorBlink,
      cursorStyle: settings.terminal.cursorStyle,
      themeBackground: config.themeBackground,
      allowOsc52Clipboard: settings.terminal.allowOsc52Clipboard,
    }

    /** Propose a fit to the daemon. The daemon echoes the size back, and only then does the
     *  terminal resize — see the Resize event below. Nothing resizes the terminal locally. */
    const applyFit = (engine: TerminalEngine, resize: (cols: number, rows: number) => void): void => {
      const dimensions = engine.proposeDimensions(getComputedStyle)
      if (!dimensions) return
      if (dimensions.cols === engine.cols && dimensions.rows === engine.rows) return
      resize(dimensions.cols, dimensions.rows)
    }

    /** Attach all DOM-level handlers to an engine. Shared by first mount and remount. */
    const attachMountedState = (engine: TerminalEngine, tty: Tty, cache: CachedTerminal) => {
      engineRef.current = engine
      ttyRef.current = tty

      // eslint-disable-next-line @typescript-eslint/unbound-method
      const { resize: rawResize } = tty.getState()
      const resize = (cols: number, rows: number) => {
        requestedSize = { cols, rows }
        rawResize(cols, rows)
      }

      // The buffer may have switched screens while this tab was unmounted.
      setIsAlternateScreen(engine.isAlternateScreen())

      scrollDisposable = engine.onScroll(() => {
        const position = engine.getScrollPosition()
        setScrollPosition(position)

        // Pin enforcement: if pinned and not at bottom, force scroll back
        if (cache.pinnedToBottom && position !== ScrollPosition.Bottom) {
          engine.scrollToBottom()
        }
      })

      // Auto-unpin on intentional mouse wheel scroll up
      wheelDisposable = engine.onWheel((deltaY) => {
        if (deltaY < 0 && cache.pinnedToBottom) {
          cache.pinnedToBottom = false
          setPinnedToBottom(false)
        }
      })

      // Activity state detector
      detector = config.disableActivityDetector
        ? null
        : createActivityStateDetector(
            (state) => { setTabState(tabId, state); }
          )

      // Focus when this tab becomes active
      unsubscribeFocus = workspace.subscribe((state) => {
        if (state.workspace.activeTabId === tabId && engineRef.current) {
          engineRef.current.focus()
        }
      })
      if (workspace.getState().workspace.activeTabId === tabId) {
        engine.focus()
      }

      // Set mounted handler — the background subscription forwards all events here
      cache.mountedHandler = (event: PtyEvent) => {
        switch (event.type) {
          case PtyEventType.Data: {
            cache.dataVersion++
            setOverlay(null)

            // Query the engine's live scroll position instead of a cached copy. The cached
            // scrollPosition only updates on the async DOM 'scroll' event, but xterm moves the
            // viewport synchronously on wheel. With a continuously-repainting TUI (Pi runs in the
            // main buffer), a data frame arrives before the scroll event fires, so a stale value
            // would read Bottom and yank the user back down every frame.
            const shouldScrollToBottom = cache.pinnedToBottom || engine.getScrollPosition() === ScrollPosition.Bottom

            const afterWrite = () => {
              if (shouldScrollToBottom) {
                engine.scrollToBottom()
                setScrollPosition(ScrollPosition.Bottom)
              }
              // Snapshot after the write lands in the buffer (xterm parses asynchronously). The
              // detector treats an unchanged viewport as idle, so an app that repaints identical
              // content stops reading as Working.
              if (detector) {
                detector.processData(snapshotViewport(engine))
              }
            }

            engine.write(event.data, afterWrite)

            setIsAlternateScreen(engine.isAlternateScreen())
            if (settings.terminal.showRawChars) {
              const decoder = new TextDecoder('utf-8', { fatal: false })
              rawChars = (rawChars + decoder.decode(event.data)).slice(-50)
              console.log('[RAW]', formatRawChars(rawChars))
            }
            break
          }
          case PtyEventType.Exit: {
            log.debug(`[${config.logPrefix} ${tabId}] PTY exited with code:`, event.exitCode)
            // Tab already removed (PTY exit arrived after tab close) — nothing to update.
            const currentTab = cancelled ? undefined : workspace.getState().workspace.appStates[tabId]
            if (currentTab) {
              const keepOnExit = (currentTab.state as BaseTerminalState | undefined)?.keepOnExit
              const immediateFailure = event.exitCode !== 0 && (Date.now() - cache.connectedAt) < 1000
              if (immediateFailure) {
                setOverlay({ message: `Process exited immediately with code ${String(event.exitCode)}`, type: 'error' })
              } else if (keepOnExit) {
                engine.write(`\r\n\x1b[2mProcess exited with exit code ${String(event.exitCode)}\x1b[0m\r\n`)
              } else {
                void removeTab(tabId)
              }
            }
            break
          }
          case PtyEventType.Resize: {
            const shouldStayAtBottom = cache.pinnedToBottom || engine.getScrollPosition() === ScrollPosition.Bottom
            const scrollRatio = engine.getScrollRatio()

            engine.resize(event.cols, event.rows)

            // Check if daemon-echoed size matches what we requested
            const req = requestedSize
            if (req && (req.cols !== event.cols || req.rows !== event.rows)) {
              setSizeMismatch({ requested: req, actual: { cols: event.cols, rows: event.rows } })
            } else {
              setSizeMismatch(null)
            }

            if (shouldStayAtBottom) {
              engine.scrollToBottom()
              setScrollPosition(ScrollPosition.Bottom)
            } else {
              engine.scrollToRatio(scrollRatio)
            }
            break
          }
          case PtyEventType.Error: {
            log.error(`[${config.logPrefix} ${tabId}] PTY stream error:`, event.message)
            setOverlay({ message: event.message, type: 'error' })
            break
          }
          case PtyEventType.End: {
            log.debug(`[${config.logPrefix} ${tabId}] PTY stream ended`)
            setOverlay((prev) => prev ?? { message: 'Terminal disconnected', type: 'error' })
            break
          }
        }
      }

      // ResizeObserver — gated on initialResizeDone to prevent resize during mount
      resizeObserver = new ResizeObserver(() => {
        if (!initialResizeDone) return
        applyFit(engine, resize)
      })
      if (containerRef.current) {
        resizeObserver.observe(containerRef.current)
      }

      // Wait for layout to settle, then fit (only sends resize if size actually changed)
      if (resizeTimeout) clearTimeout(resizeTimeout)
      resizeTimeout = setTimeout(() => {
        if (cancelled) return
        applyFit(engine, resize)
        log.debug(`[${config.logPrefix} ${tabId}] resize (initial):`, { cols: engine.cols, rows: engine.rows })
        initialResizeDone = true
      }, INITIAL_FIT_DELAY_MS)

      // Forward terminal input to PTY only when this tab is active.
      // Inactive tabs can't receive keystrokes so any onData during
      // replay is an auto-response (OSC 11, DA, etc.) — drop it.
      //
      // The write is awaited end-to-end — under PTY backpressure the daemon
      // pauses tonic's message loop, HTTP/2 closes the client's stream-level
      // receive window, and the Promise resolves only when the bytes have
      // landed. The engine queues subsequent onData firings while the previous
      // one is in flight, preserving order.
      inputDisposable = engine.onData((data) => {
        const activeTab = workspace.getState().workspace.activeTabId
        if (activeTab !== undefined && activeTab !== tabId) return
        const tty = ttyRef.current
        if (!tty) return
        tty.getState().write(data).catch((error: unknown) => {
          log.error(`[${config.logPrefix} ${tabId}] pty write failed:`, error)
          setOverlay({
            message: error instanceof Error ? error.message : String(error),
            type: 'error',
          })
        })
      })
    }

    if (existingCache) {
      // === REMOUNT PATH: reuse cached terminal ===
      log.debug(`[${config.logPrefix} ${tabId}] remount from cache`)
      const engine = existingCache.engine

      // Apply current settings (font, cursor, theme may have changed)
      engine.applyDisplayOptions(displayOptions)

      // Restore pinned state from cache
      setPinnedToBottom(existingCache.pinnedToBottom)

      setLoading(false)
      ;(containerRef.current as TerminalContainerElement).terminal = engine.raw
      engine.attach(containerRef.current)

      attachMountedState(engine, existingCache.tty, existingCache)
    } else {
      // === FIRST MOUNT PATH: create terminal and cache it ===
      log.debug(`[${config.logPrefix} ${tabId}] first mount`, {
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

        const engine = await config.createEngine({
          ...displayOptions,
          scrollback: SCROLLBACK_LINES,
          openExternal,
          writeClipboardText: clipboard.writeText,
          label: `${config.logPrefix} ${tabId}`,
        })

        // Owns the engine and (once attached) the Tty. Every teardown path below is
        // `owner.dispose()`; nothing is freed piecemeal.
        const owner = new DisposableStore()
        owner.add(engine)

        // `cancelled` is written by the cleanup closure while the awaits in here are in flight.
        // TS narrows it to false from the effect body and cannot see that write, hence the
        // no-unnecessary-condition suppressions below.
        if (cancelled || !containerRef.current) {
          // A StrictMode double-mount (or a fast tab switch) raced ahead of this async init.
          // The engine was never attached and never cached, so nothing else will free it.
          owner.dispose()
          return
        }

        setLoading(false)
        engine.attach(containerRef.current)
        ;(containerRef.current as TerminalContainerElement).terminal = engine.raw

        // Create cache entry — event handler is registered before the stream starts
        const cache: CachedTerminal = {
          engine,
          tty: undefined as unknown as Tty,  // set after openTtyStream resolves
          owner,
          mountedHandler: null,
          connectedAt: Date.now(),
          dataVersion: 0,
          pinnedToBottom: false,
          badgeClickTimer: null,
          onExitUnmounted: (exitCode: number) => {
            log.debug(`[${config.logPrefix} ${tabId}] PTY exited while unmounted, code:`, exitCode)
            const currentTab = workspace.getState().workspace.appStates[tabId]
            if (!currentTab) return // Tab already removed (PTY exit arrived after tab close)
            const keepOnExit = (currentTab.state as BaseTerminalState | undefined)?.keepOnExit
            if (keepOnExit) {
              engine.write(`\r\n\x1b[2mProcess exited with exit code ${String(exitCode)}\x1b[0m\r\n`)
            } else {
              void workspace.getState().removeTab(tabId)
            }
          },
        }

        let tty: Tty
        try {
          // `owner` takes the Tty, so the subscription cannot outlive the engine.
          tty = await thenRegisterOrDispose(
            session.openTtyStream(currentExistingPtyId, (event) => { handleCachedEvent(cache, event) }),
            owner,
          )
          log.debug(`[${config.logPrefix} ${tabId}] reattached to session:`, currentExistingPtyId)
        } catch (error) {
          log.error(`[${config.logPrefix} ${tabId}] failed to attach to PTY:`, currentExistingPtyId, error)
          owner.dispose()
          // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition
          if (cancelled) return
          setOverlay({ message: `Failed to reattach terminal: ${error instanceof Error ? error.message : 'Unknown error'}`, type: 'error' })
          return
        }

        // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition
        if (cancelled) {
          // Unmounted while the daemon was answering. This closure never stored the engine on
          // termAppRef.cachedTerminal, so the effect cleanup couldn't reach it — tear it down
          // here. Otherwise the engine lingers as an orphaned DOM node while the next mount
          // opens its own (duplicated output), and the orphan steals focus with no onData
          // wired (no input).
          owner.dispose()
          return
        }

        cache.tty = tty

        if (termAppRef) termAppRef.cachedTerminal = cache

        // Notify parent that terminal is ready (first mount only — handlers persist on the cached terminal)
        config.onTerminalReady?.(engine)

        attachMountedState(engine, tty, cache)
      }

      void init()
    }

    // Cleanup — detach mounted state but keep terminal + TTY subscription alive
    return () => {
      // NOTE: never dispose `owner` here. Unmount is not close — the cache (and its Tty
      // subscription) must survive a tab switch. Only disposeCachedTerminal ends it.
      log.debug(`[${config.logPrefix} ${tabId}] cleanup (terminal cached, PTY preserved)`)
      cancelled = true
      if (resizeTimeout) clearTimeout(resizeTimeout)

      // Disconnect mounted UI state
      inputDisposable?.dispose()
      scrollDisposable?.dispose()
      wheelDisposable?.dispose()
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

      engineRef.current = null
      ttyRef.current = null
      // Do NOT dispose the engine or unsubscribe the TTY — they stay cached
    }
  }, [tabId, workspaceId, config, settings, removeTab, setTabState, sessionStore, workspace, refreshCounter, openExternal, clipboard.writeText])

  const handleScrollDown = useCallback(() => {
    engineRef.current?.scrollToBottom()
  }, [engineRef])

  const handleScrollToTop = useCallback(() => {
    engineRef.current?.scrollToTop()
  }, [engineRef])

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
    const selection = engineRef.current?.getSelection()
    log.debug(`[${config.logPrefix} ${tabId}] copy:`, { selection: selection ?? '(no selection)', hasTerminal: !!engineRef.current })
    if (selection) {
      clipboard.writeText(selection)
    }
    closeContextMenu()
  }

  const handlePaste = async () => {
    const text = await clipboard.readText()
    log.debug(`[${config.logPrefix} ${tabId}] paste:`, { clipboardText: text || '(empty)', hasTty: !!ttyRef.current })
    closeContextMenu()
    if (text && ttyRef.current) {
      try {
        await ttyRef.current.getState().write(text)
      } catch (error) {
        log.error(`[${config.logPrefix} ${tabId}] paste write failed:`, error)
        setOverlay({
          message: error instanceof Error ? error.message : String(error),
          type: 'error',
        })
      }
    }
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
                engineRef.current?.scrollToBottom()
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
            engineRef.current?.scrollToBottom()
          }}
          title="Scroll to bottom"
        >
          ↓
        </button>
      </div>
    </div>
  )
}
