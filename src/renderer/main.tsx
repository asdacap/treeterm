import React from 'react'
import ReactDOM from 'react-dom/client'
import './monaco-config' // Configure Monaco before any components use it
import { useAppStore } from './store/app'
import App from './App'
import '@aptre/flex-layout/style/dark.css'
import './styles/index.css'
import './styles/flexlayout-overrides.css'

declare global {
  interface Window {
    __enableKeyDiag?: boolean
  }
}

// Single point of window/electron access — everything else reads from the store
window.electron.app.onReady(() => {
  const e = window.electron

  void useAppStore.getState().initialize({
    platform: e.platform,
    terminal: e.terminal,
    filesystem: e.filesystem,
    exec: e.exec,
    sandbox: e.sandbox,
    ssh: e.ssh,
    clipboard: e.clipboard,
    sessionApi: e.session,
    settingsApi: e.settings,
    appApi: e.app,
    daemon: e.daemon,
    selectFolder: e.selectFolder,
    getWindowUuid: e.getWindowUuid,
    getInitialWorkspace: e.getInitialWorkspace,
    openExternal: (url: string) => { window.open(url, '_blank') },
    getViewportSize: () => ({ width: window.innerWidth, height: window.innerHeight }),
    keyEventTarget: window,
    isKeyDiagEnabled: () => !!window.__enableKeyDiag,
  })

  const rootEl = document.getElementById('root')
  if (!rootEl) throw new Error('Root element not found')
  ReactDOM.createRoot(rootEl).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>
  )
})
