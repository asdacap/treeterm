import { useState, useEffect, useRef } from 'react'
import { Terminal } from '@xterm/xterm'
import { fitTerminal } from '../utils/fitTerminal'
import type { TerminalApi } from '../types'
import { PtyViewerStatus } from '../types'
import { PtyEventType } from '../../shared/ipc-types'

/** Attaches to an existing daemon PTY and renders its live, interactive output
 *  in an xterm instance. Self-contained: registers its event listener before
 *  starting the stream, forwards input, and tears everything down on unmount. */
export default function PtyViewer({ ptyId, connectionId, terminalApi }: { ptyId: string; connectionId: string; terminalApi: TerminalApi }) {
  const containerRef = useRef<HTMLDivElement>(null)
  const termRef = useRef<Terminal | null>(null)
  const [status, setStatus] = useState(PtyViewerStatus.Loading)
  const [errorMessage, setErrorMessage] = useState<string | null>(null)

  useEffect(() => {
    const container = containerRef.current
    if (!container) return

    let cancelled = false
    const cleanups: (() => void)[] = []

    const term = new Terminal({
      fontSize: 13,
      fontFamily: 'monospace',
      theme: {
        background: '#1e1e1e',
        foreground: '#d4d4d4',
      },
      cursorBlink: true,
      scrollback: 5000,
    })

    let handleRef: string | null = null

    const resizePty = (cols: number, rows: number) => {
      if (handleRef) {
        terminalApi.resize(handleRef, cols, rows)
      }
    }

    term.open(container)
    fitTerminal(term, resizePty, getComputedStyle)

    termRef.current = term

    const resizeObserver = new ResizeObserver(() => {
      fitTerminal(term, resizePty, getComputedStyle)
    })
    resizeObserver.observe(container)
    cleanups.push(() => { resizeObserver.disconnect(); })

    // Attach to PTY — register listener before starting stream
    const handle = crypto.randomUUID()
    handleRef = handle

    const unsubEvent = terminalApi.onEvent(handle, (event) => {
      if (event.type === PtyEventType.Data) {
        term.write(event.data)
      } else if (event.type === PtyEventType.Exit) {
        term.write('\r\n\x1b[90m[Process exited]\x1b[0m\r\n')
      } else if (event.type === PtyEventType.Resize) {
        term.resize(event.cols, event.rows)
      }
    })
    cleanups.push(unsubEvent)

    void terminalApi.attach(connectionId, handle, ptyId).then((result) => {
      if (cancelled) return

      if (!result.success) {
        setStatus(PtyViewerStatus.Error)
        setErrorMessage(result.error)
        return
      }

      // Forward input using handle
      const onDataDisposable = term.onData((data) => {
        terminalApi.write(handle, data).catch((err: unknown) => {
          console.error(`[PtyViewer] pty write failed for ${handle}:`, err)
        })
      })
      cleanups.push(() => { onDataDisposable.dispose(); })

      setStatus(PtyViewerStatus.Ready)
    }).catch((err: unknown) => {
      if (cancelled) return
      setStatus(PtyViewerStatus.Error)
      setErrorMessage(err instanceof Error ? err.message : `Failed to attach to PTY session ${ptyId}`)
    })

    return () => {
      cancelled = true
      for (const cleanup of cleanups) cleanup()
      term.dispose()
      termRef.current = null
    }
  }, [ptyId, terminalApi, connectionId])

  if (status === PtyViewerStatus.Error) {
    return (
      <div className="active-processes-pty-viewer" style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', color: '#f44336', padding: '24px', textAlign: 'center' }}>
        <div>
          <div style={{ fontSize: '16px', fontWeight: 'bold', marginBottom: '8px' }}>Failed to attach to process</div>
          <div style={{ fontSize: '13px', opacity: 0.8 }}>{errorMessage}</div>
        </div>
      </div>
    )
  }

  return (
    <div style={{ position: 'relative', width: '100%', height: '100%' }}>
      {status === PtyViewerStatus.Loading && (
        <div style={{ position: 'absolute', inset: 0, display: 'flex', alignItems: 'center', justifyContent: 'center', color: '#888', zIndex: 1 }}>
          Attaching to process...
        </div>
      )}
      <div ref={containerRef} className="active-processes-pty-viewer" />
    </div>
  )
}
