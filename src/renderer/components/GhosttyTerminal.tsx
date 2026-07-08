import { useEffect, useRef, useState, type ReactNode } from 'react'
import { useStore } from 'zustand'
import {
  init,
  Terminal as GhosttyTerm,
  FitAddon,
  OSC8LinkProvider,
  UrlRegexProvider,
  type IDisposable,
  type ILink,
  type ILinkProvider,
} from 'ghostty-web'
import { TERMINAL_THEME_COLORS } from './terminalTheme'
import { log } from '../utils/logger'
import { useSettingsStore } from '../store/settings'
import { useAppStore } from '../store/app'
import { useSessionApi } from '../contexts/SessionStoreContext'
import { useContextMenuStore } from '../store/contextMenu'
import ContextMenu from './ContextMenu'
import type { Tty } from '../store/createTtyStore'
import type { PtyEvent, TerminalState, WorkspaceStore } from '../types'
import { PtyEventType } from '../../shared/ipc-types'

const THEME_BACKGROUND = '#1e1e1e'
const SCROLLBACK_LINES = 50000
/** Let the container's layout settle before the first fit — matches BaseTerminal. */
const INITIAL_FIT_DELAY_MS = 100

export enum TerminalPhase {
  Loading = 'loading',
  Ready = 'ready',
  Failed = 'failed',
}

type ViewState =
  | { phase: TerminalPhase.Loading }
  | { phase: TerminalPhase.Ready }
  | { phase: TerminalPhase.Failed; message: string }

type PhaseState<P extends TerminalPhase> = Extract<ViewState, { phase: P }>
type OverlayRenderers = { [P in TerminalPhase]: (state: PhaseState<P>) => ReactNode }

const OVERLAYS: OverlayRenderers = {
  [TerminalPhase.Loading]: () => (
    <div className="terminal-overlay terminal-overlay-info">Loading terminal...</div>
  ),
  [TerminalPhase.Ready]: () => null,
  [TerminalPhase.Failed]: (state) => (
    <div className="terminal-overlay terminal-overlay-error">{state.message}</div>
  ),
}

function TerminalOverlay({ view }: { view: ViewState }): ReactNode {
  // OVERLAYS is keyed by phase, so the renderer at view.phase accepts exactly this view.
  const render = OVERLAYS[view.phase] as (state: ViewState) => ReactNode
  return render(view)
}

/**
 * ghostty-web's bundled link providers activate with `window.open`, which would spawn a
 * BrowserWindow inside Electron. Keep their detection, replace the activation.
 */
export function routeLinksExternally(
  provider: ILinkProvider,
  openExternal: (uri: string) => void,
): ILinkProvider {
  return {
    provideLinks(y: number, callback: (links: ILink[] | undefined) => void): void {
      provider.provideLinks(y, (links) => {
        callback(links?.map((link) => ({ ...link, activate: () => { openExternal(link.text) } })))
      })
    },
    dispose(): void {
      provider.dispose?.()
    },
  }
}

interface GhosttyTerminalProps {
  workspace: WorkspaceStore
  tabId: string
}

/**
 * Terminal backed by ghostty-web — Ghostty's VT engine (libghostty-vt, compiled to WASM)
 * drawn by a Canvas2D renderer, in place of xterm.js.
 *
 * Deliberately narrower than BaseTerminal: no cross-unmount terminal cache, no scroll-position
 * badge, no activity-state detector. Unmounting disposes the terminal and drops the TTY
 * subscription; remounting re-attaches, and the daemon replays screen state plus its retained
 * raw buffer (`pty_manager.rs#subscribe_with_initial_state`), so visible content is rebuilt.
 * What is lost across a remount is scrollback older than the daemon's retained buffer.
 */
export default function GhosttyTerminal({ workspace, tabId }: GhosttyTerminalProps): ReactNode {
  const wsData = useStore(workspace, (s) => s.workspace)
  const ptyId = (wsData.appStates[tabId]?.state as TerminalState | undefined)?.ptyId

  if (!ptyId) {
    return <div className="terminal-overlay terminal-overlay-info">Creating terminal...</div>
  }

  // key on ptyId: a new PTY is a new terminal, never a reconfigured one.
  return <GhosttyTerminalView key={ptyId} workspace={workspace} tabId={tabId} ptyId={ptyId} />
}

interface GhosttyTerminalViewProps {
  workspace: WorkspaceStore
  tabId: string
  ptyId: string
}

function GhosttyTerminalView({ workspace, tabId, ptyId }: GhosttyTerminalViewProps): ReactNode {
  const containerRef = useRef<HTMLDivElement>(null)
  const terminalRef = useRef<GhosttyTerm | null>(null)
  const ttyRef = useRef<Tty | null>(null)
  const [view, setView] = useState<ViewState>({ phase: TerminalPhase.Loading })

  const workspaceId = useStore(workspace, (s) => s.workspace.id)
  const sessionStore = useSessionApi()
  const settings = useSettingsStore((state) => state.settings)
  const clipboard = useAppStore((state) => state.clipboard)
  const openExternal = useAppStore((state) => state.openExternal)

  useEffect(() => {
    const container = containerRef.current
    if (!container) return

    let cancelled = false
    let initialFitDone = false
    let terminal: GhosttyTerm | null = null
    let tty: Tty | null = null
    let unsubscribeEvents: (() => void) | null = null
    let unsubscribeFocus: (() => void) | null = null
    let inputDisposable: IDisposable | null = null
    let resizeObserver: ResizeObserver | null = null
    let fitTimeout: ReturnType<typeof setTimeout> | null = null

    const fail = (message: string): void => {
      if (!cancelled) setView({ phase: TerminalPhase.Failed, message })
    }

    // Registered with openTtyStream before attach, so events can land before it resolves.
    // Only `terminal` is needed here, and it exists by then.
    const handleEvent = (event: PtyEvent): void => {
      if (cancelled || !terminal) return
      switch (event.type) {
        case PtyEventType.Data:
          terminal.write(event.data)
          break
        case PtyEventType.Resize:
          // The daemon owns the size. This is the only place the terminal is resized.
          terminal.resize(event.cols, event.rows)
          break
        case PtyEventType.Exit: {
          log.debug(`[Ghostty ${tabId}] PTY exited with code:`, event.exitCode)
          // A PTY exit can arrive after the tab is gone — there is nothing left to update.
          const tab = workspace.getState().workspace.appStates[tabId]
          if (!tab) return
          const keepOnExit = (tab.state as TerminalState | undefined)?.keepOnExit ?? false
          if (keepOnExit) {
            terminal.write(`\r\n\x1b[2mProcess exited with exit code ${String(event.exitCode)}\x1b[0m\r\n`)
          } else {
            void workspace.getState().removeTab(tabId)
          }
          break
        }
        case PtyEventType.Error:
          log.error(`[Ghostty ${tabId}] PTY stream error:`, event.message)
          fail(event.message)
          break
        case PtyEventType.End:
          log.debug(`[Ghostty ${tabId}] PTY stream ended`)
          fail('Terminal disconnected')
          break
      }
    }

    const start = async (): Promise<void> => {
      // Instantiates the ghostty-vt WASM module, which the bundle carries as an inline
      // data: URI. Idempotent — later calls resolve against the same instance.
      await init()
      if (cancelled) return

      terminal = new GhosttyTerm({
        cursorBlink: settings.terminal.cursorBlink,
        cursorStyle: settings.terminal.cursorStyle,
        fontSize: settings.terminal.fontSize,
        fontFamily: settings.terminal.fontFamily,
        scrollback: SCROLLBACK_LINES,
        theme: {
          ...TERMINAL_THEME_COLORS,
          background: THEME_BACKGROUND,
          cursorAccent: THEME_BACKGROUND,
        },
      })
      terminal.open(container)

      // registerLinkProvider throws unless the terminal is already open.
      const routeExternal = (uri: string): void => { openExternal(uri) }
      terminal.registerLinkProvider(routeLinksExternally(new UrlRegexProvider(terminal), routeExternal))
      terminal.registerLinkProvider(routeLinksExternally(new OSC8LinkProvider(terminal), routeExternal))

      const fit = new FitAddon()
      terminal.loadAddon(fit)

      let attached: { tty: Tty; unsubscribe: () => void }
      try {
        attached = await sessionStore.getState().openTtyStream(ptyId, handleEvent)
      } catch (error) {
        log.error(`[Ghostty ${tabId}] failed to attach to PTY:`, ptyId, error)
        // The terminal is open but has no stream behind it — drop the canvas and show why.
        terminal.dispose()
        fail(`Failed to attach terminal: ${error instanceof Error ? error.message : 'Unknown error'}`)
        return
      }

      // `cancelled` is written by the cleanup closure while openTtyStream is awaited. TS narrows
      // it to false from the `await init()` guard above and cannot see that write, so the rule
      // below wrongly reads this as dead code.
      // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition
      if (cancelled) {
        // A StrictMode double-mount (or a fast tab switch) raced ahead of this async start.
        // The cleanup below never saw these, so tear them down here — otherwise the canvas
        // lingers in a detached container and the TTY subscription leaks.
        attached.unsubscribe()
        terminal.dispose()
        return
      }

      tty = attached.tty
      unsubscribeEvents = attached.unsubscribe
      terminalRef.current = terminal
      ttyRef.current = tty
      log.debug(`[Ghostty ${tabId}] attached to pty:`, ptyId)

      // Propose only. The daemon applies the size and echoes a Resize event back, which is
      // what actually resizes the terminal. Never resize locally — that is why FitAddon.fit()
      // and observeResize() are not used.
      // Reads the refs rather than the closure vars: cleanup nulls them, so a fit that races
      // an unmount is inert even before the observer disconnects.
      const applyFit = (): void => {
        const currentTerminal = terminalRef.current
        const currentTty = ttyRef.current
        if (!currentTerminal || !currentTty) return
        const dims = fit.proposeDimensions()
        if (!dims) return
        if (dims.cols === currentTerminal.cols && dims.rows === currentTerminal.rows) return
        currentTty.getState().resize(dims.cols, dims.rows)
      }

      resizeObserver = new ResizeObserver(() => {
        if (initialFitDone) applyFit()
      })
      resizeObserver.observe(container)

      fitTimeout = setTimeout(() => {
        if (cancelled) return
        applyFit()
        initialFitDone = true
      }, INITIAL_FIT_DELAY_MS)

      inputDisposable = terminal.onData((data: string) => {
        // Inactive tabs cannot receive keystrokes, so any onData here is an auto-response
        // (DA, OSC 11, …) provoked by the daemon's replay. Drop it.
        const activeTabId = workspace.getState().workspace.activeTabId
        if (activeTabId !== undefined && activeTabId !== tabId) return
        const current = ttyRef.current
        if (!current) return
        current.getState().write(data).catch((error: unknown) => {
          log.error(`[Ghostty ${tabId}] pty write failed:`, error)
          fail(error instanceof Error ? error.message : String(error))
        })
      })

      unsubscribeFocus = workspace.subscribe((state) => {
        if (state.workspace.activeTabId === tabId) terminalRef.current?.focus()
      })
      if (workspace.getState().workspace.activeTabId === tabId) terminal.focus()

      setView({ phase: TerminalPhase.Ready })
    }

    start().catch((error: unknown) => {
      log.error(`[Ghostty ${tabId}] failed to start terminal:`, error)
      fail(error instanceof Error ? error.message : String(error))
    })

    return () => {
      log.debug(`[Ghostty ${tabId}] cleanup (terminal disposed, PTY preserved)`)
      cancelled = true
      if (fitTimeout) clearTimeout(fitTimeout)
      resizeObserver?.disconnect()
      inputDisposable?.dispose()
      unsubscribeFocus?.()
      // Unsubscribe before dispose: writing PTY data into a disposed terminal throws.
      unsubscribeEvents?.()
      terminal?.dispose()
      terminalRef.current = null
      ttyRef.current = null
    }
  }, [tabId, workspaceId, ptyId, settings, sessionStore, workspace, openExternal])

  const openContextMenu = useContextMenuStore((s) => s.open)
  const closeContextMenu = useContextMenuStore((s) => s.close)
  const activeMenuId = useContextMenuStore((s) => s.activeMenuId)
  const menuPosition = useContextMenuStore((s) => s.position)
  const contextMenuId = `ghostty-terminal-${tabId}`

  const handleContextMenu = (e: React.MouseEvent): void => {
    e.preventDefault()
    openContextMenu(contextMenuId, e.clientX, e.clientY)
  }

  const handleCopy = (): void => {
    const selection = terminalRef.current?.getSelection()
    if (selection) clipboard.writeText(selection)
    closeContextMenu()
  }

  const handlePaste = async (): Promise<void> => {
    const text = await clipboard.readText()
    closeContextMenu()
    const tty = ttyRef.current
    if (!text || !tty) return
    try {
      await tty.getState().write(text)
    } catch (error) {
      log.error(`[Ghostty ${tabId}] paste write failed:`, error)
      setView({
        phase: TerminalPhase.Failed,
        message: error instanceof Error ? error.message : String(error),
      })
    }
  }

  return (
    <div className="terminal-wrapper">
      <div className="terminal-padding-wrapper">
        <div
          ref={containerRef}
          className="terminal-container"
          onContextMenu={handleContextMenu}
        />
      </div>

      <TerminalOverlay view={view} />

      <ContextMenu menuId={contextMenuId} activeMenuId={activeMenuId} position={menuPosition}>
        <div className="context-menu-item" onClick={handleCopy}>
          Copy
        </div>
        <div className="context-menu-item" onClick={() => { void handlePaste(); }}>
          Paste
        </div>
      </ContextMenu>
    </div>
  )
}
