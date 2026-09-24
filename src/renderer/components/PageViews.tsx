import { useEffect, useState } from 'react'
import type { ReactNode } from 'react'
import type { WebviewTag } from 'electron'
import type { PortForwardInfo, SSHApi } from '../types'
import { PortForwardStatus } from '../../shared/types'
import { WebAppPhase } from '../../applications/webApp/runtime'
import type { WebAppRuntime } from '../../applications/webApp/runtime'

// Views shared by the WebApp and Browser panes: the embedded page and the ssh forward.

export type PageSsh = Pick<SSHApi, 'listPortForwards' | 'onPortForwardStatus' | 'watchPortForwardOutput'>

export interface DetailsLink {
  label: string
  onClick: () => void
}

interface BrowserViewProps {
  runtime: WebAppRuntime
  onRetry: () => void
  // Where to look while waiting or after a failure (the shell, the port forward log).
  detailsLinks: DetailsLink[]
  openExternal: (url: string) => void
}

export function BrowserView({ runtime, onRetry, detailsLinks, openExternal }: BrowserViewProps) {
  const links = detailsLinks.map(link => (
    <button key={link.label} className="webapp-link" onClick={link.onClick}>{link.label}</button>
  ))
  const waiting = (message: string): ReactNode => (
    <div className="webapp-waiting">
      <div className="webapp-spinner" />
      <div>{message}</div>
      {links}
    </div>
  )
  // Render callbacks are invoked directly, not mounted as component types.
  /* eslint-disable react/no-unstable-nested-components */
  const views: { [P in WebAppPhase]: (r: Extract<WebAppRuntime, { phase: P }>) => ReactNode } = {
    [WebAppPhase.AllocatingPort]: () => waiting('Finding a free port…'),
    [WebAppPhase.WaitingForServer]: (r) => waiting(`Waiting for server on port ${String(r.port)}…`),
    [WebAppPhase.Forwarding]: (r) => waiting(`Starting port forward for port ${String(r.port)}…`),
    [WebAppPhase.Ready]: (r) => <WebView url={r.url} openExternal={openExternal} />,
    [WebAppPhase.Error]: (r) => (
      <div className="webapp-waiting">
        <div className="webapp-error">{r.message}</div>
        <button className="webapp-link" onClick={onRetry}>Retry</button>
        {links}
      </div>
    ),
  }
  /* eslint-enable react/no-unstable-nested-components */
  // TS cannot correlate the mapped key with the union member; the record above is exhaustive.
  const view = views[runtime.phase] as (r: WebAppRuntime) => ReactNode
  return <>{view(runtime)}</>
}

function WebView({ url, openExternal }: { url: string; openExternal: (url: string) => void }) {
  const [view, setView] = useState<WebviewTag | null>(null)
  const [currentUrl, setCurrentUrl] = useState(url)

  useEffect(() => {
    if (!view) return
    const onNavigate = (e: Event): void => { setCurrentUrl((e as Event & { url: string }).url) }
    view.addEventListener('did-navigate', onNavigate)
    view.addEventListener('did-navigate-in-page', onNavigate)
    return () => {
      view.removeEventListener('did-navigate', onNavigate)
      view.removeEventListener('did-navigate-in-page', onNavigate)
    }
  }, [view])

  return (
    <div className="webapp-browser">
      <div className="webapp-toolbar">
        <button title="Back" onClick={() => { view?.goBack() }}>←</button>
        <button title="Forward" onClick={() => { view?.goForward() }}>→</button>
        <button title="Reload" onClick={() => { view?.reload() }}>⟳</button>
        <span className="webapp-url">{currentUrl}</span>
        <button title="Open in external browser" onClick={() => { openExternal(currentUrl) }}>↗</button>
      </div>
      <webview
        ref={(el: HTMLElement | null) => { setView(el as WebviewTag | null) }}
        src={url}
        partition="persist:webapp"
        className="webapp-webview"
      />
    </div>
  )
}

interface PortForwardViewProps {
  ssh: PageSsh
  connectionId: string
  forwardId: string
  onRestart: () => void
}

type LoadedForward = { id: number; info: PortForwardInfo | undefined }

export function PortForwardView({ ssh, connectionId, forwardId, onRestart }: PortForwardViewProps) {
  const [forward, setForward] = useState<LoadedForward>({ id: 0, info: undefined })
  const [listError, setListError] = useState('')

  useEffect(() => {
    // Subscribe before listing so no status change is missed in between.
    const unsubscribe = ssh.onPortForwardStatus((info) => {
      if (info.id !== forwardId) return
      // A new Connecting forward is a fresh ssh process: re-watch its output.
      setForward(prev => ({ id: info.status === PortForwardStatus.Connecting ? prev.id + 1 : prev.id, info }))
    })
    ssh.listPortForwards(connectionId).then((list) => {
      const info = list.find(pf => pf.id === forwardId)
      setForward(prev => ({ id: prev.id + 1, info }))
    }).catch((err: unknown) => {
      setListError(err instanceof Error ? err.message : String(err))
    })
    return unsubscribe
  }, [ssh, connectionId, forwardId])

  if (listError) {
    return <div className="webapp-waiting"><div className="webapp-error">{listError}</div></div>
  }
  if (!forward.info) {
    return <div className="webapp-waiting">No port forward yet. It starts once the server is ready.</div>
  }
  const info = forward.info
  const canRestart = info.status === PortForwardStatus.Error || info.status === PortForwardStatus.Stopped
  return (
    <div className="webapp-forward">
      <div className="webapp-toolbar">
        <span className="webapp-url">
          localhost:{info.localPort} → {info.remoteHost}:{info.remotePort} ({info.status})
        </span>
        {canRestart && <button onClick={onRestart}>Restart</button>}
      </div>
      {info.status === PortForwardStatus.Error && <div className="webapp-error">{info.error}</div>}
      <ForwardLog key={forward.id} ssh={ssh} forwardId={forwardId} />
    </div>
  )
}

function ForwardLog({ ssh, forwardId }: { ssh: PageSsh; forwardId: string }) {
  // Lines are append-only, so their position is a stable identity.
  const [lines, setLines] = useState<string[]>([])
  useEffect(() => {
    let unsubscribe = (): void => {}
    let cancelled = false
    ssh.watchPortForwardOutput(forwardId, (line) => { setLines(prev => [...prev, line]) })
      .then((watch) => {
        unsubscribe = watch.unsubscribe
        if (cancelled) { unsubscribe(); return }
        setLines(prev => [...watch.scrollback, ...prev])
      })
      .catch((err: unknown) => {
        setLines(prev => [...prev, `Failed to watch output: ${err instanceof Error ? err.message : String(err)}`])
      })
    return () => { cancelled = true; unsubscribe() }
  }, [ssh, forwardId])
  return (
    <div className="ssh-pane-output">
      {/* eslint-disable-next-line react/no-array-index-key -- append-only log */}
      {lines.map((line, i) => <div key={i}>{line}</div>)}
    </div>
  )
}
