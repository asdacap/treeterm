import { useState } from 'react'
import { useStore } from 'zustand'
import Terminal from './Terminal'
import { BrowserView, PortForwardView } from './PageViews'
import type { PageSsh } from './PageViews'
import type { WorkspaceStore } from '../types'
import { forwardIdForTab } from '../../applications/webApp/runtime'
import type { WebAppRef } from '../../applications/webApp/runtime'

interface WebAppPaneProps {
  workspace: WorkspaceStore
  tabId: string
  isVisible: boolean
  ssh: PageSsh
  openExternal: (url: string) => void
}

export enum WebAppSubTab {
  Browser = 'browser',
  Shell = 'shell',
  PortForward = 'port-forward',
}

export default function WebAppPane({ workspace, tabId, isVisible, ssh, openExternal }: WebAppPaneProps) {
  const [subTab, setSubTab] = useState<WebAppSubTab>(WebAppSubTab.Browser)
  const isRemote = useStore(workspace, s => s.isRemote)
  const cwd = useStore(workspace, s => s.workspace.path)
  const connectionId = useStore(workspace, s => s.connectionId)
  // The ref is created in onWorkspaceLoad before the tab renders.
  const ref = useStore(workspace, s => s.getTabRef(tabId)) as WebAppRef
  // Every runtime transition replaces the whole state, so the identity selector is exact.
  const runtime = useStore(ref.runtime, s => s)

  const subTabs: { id: WebAppSubTab; label: string }[] = [
    { id: WebAppSubTab.Browser, label: 'Browser' },
    { id: WebAppSubTab.Shell, label: 'Shell' },
    ...(isRemote ? [{ id: WebAppSubTab.PortForward, label: 'Port Forward' }] : []),
  ]

  // Every sub-tab stays mounted so switching never recreates the terminal or the webview.
  const show = (id: WebAppSubTab): { display: string } => ({ display: subTab === id ? 'flex' : 'none' })

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
      <div className="webapp-pane-body" style={show(WebAppSubTab.Browser)}>
        <BrowserView
          runtime={runtime}
          onRetry={ref.retry}
          detailsLinks={[{ label: 'View shell', onClick: () => { setSubTab(WebAppSubTab.Shell) } }]}
          openExternal={openExternal}
        />
      </div>
      <div className="webapp-pane-body" style={show(WebAppSubTab.Shell)}>
        <Terminal
          cwd={cwd}
          workspace={workspace}
          tabId={tabId}
          isVisible={isVisible && subTab === WebAppSubTab.Shell}
        />
      </div>
      {isRemote && (
        <div className="webapp-pane-body" style={show(WebAppSubTab.PortForward)}>
          <PortForwardView
            ssh={ssh}
            connectionId={connectionId}
            forwardId={forwardIdForTab(tabId)}
            onRestart={() => { void ref.restartForward() }}
          />
        </div>
      )}
    </div>
  )
}
