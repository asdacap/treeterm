import { useState } from 'react'
import { useStore } from 'zustand'
import type { WorkspaceStore } from '../types'
import { BrowserView, PortForwardView } from './PageViews'
import type { PageSsh } from './PageViews'
import { forwardIdForTab } from '../../applications/webApp/runtime'
import type { PageRef } from '../../applications/webApp/runtime'

interface BrowserPaneProps {
  workspace: WorkspaceStore
  tabId: string
  ssh: PageSsh
  openExternal: (url: string) => void
}

enum BrowserSubTab {
  Browser = 'browser',
  PortForward = 'port-forward',
}

export default function BrowserPane({ workspace, tabId, ssh, openExternal }: BrowserPaneProps) {
  const [subTab, setSubTab] = useState<BrowserSubTab>(BrowserSubTab.Browser)
  const isRemote = useStore(workspace, s => s.isRemote)
  const connectionId = useStore(workspace, s => s.connectionId)
  // The ref is created in onWorkspaceLoad before the tab renders.
  const ref = useStore(workspace, s => s.getTabRef(tabId)) as PageRef
  // Every runtime transition replaces the whole state, so the identity selector is exact.
  const runtime = useStore(ref.runtime, s => s)

  const browser = (
    <BrowserView
      runtime={runtime}
      onRetry={ref.retry}
      detailsLinks={isRemote ? [{ label: 'View port forward', onClick: () => { setSubTab(BrowserSubTab.PortForward) } }] : []}
      openExternal={openExternal}
    />
  )
  // Local sessions open the page directly: nothing else to show.
  if (!isRemote) {
    return <div className="webapp-pane">{browser}</div>
  }

  const subTabs = [
    { id: BrowserSubTab.Browser, label: 'Browser' },
    { id: BrowserSubTab.PortForward, label: 'Port Forward' },
  ]
  // Both sub-tabs stay mounted so switching never reloads the webview.
  const show = (id: BrowserSubTab): { display: string } => ({ display: subTab === id ? 'flex' : 'none' })

  return (
    <div className="webapp-pane">
      <div className="ssh-pane-subtabs">
        {subTabs.map(tab => (
          <button
            key={tab.id}
            className={`ssh-pane-tab ${subTab === tab.id ? 'active' : ''}`}
            onClick={() => { setSubTab(tab.id) }}
          >
            {tab.label}
          </button>
        ))}
      </div>
      <div className="webapp-pane-body" style={show(BrowserSubTab.Browser)}>{browser}</div>
      <div className="webapp-pane-body" style={show(BrowserSubTab.PortForward)}>
        <PortForwardView
          ssh={ssh}
          connectionId={connectionId}
          forwardId={forwardIdForTab(tabId)}
          onRestart={() => { void ref.restartForward() }}
        />
      </div>
    </div>
  )
}
