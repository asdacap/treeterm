import React from 'react'
import ReactDOM from 'react-dom/client'
import './monaco-config' // Configure Monaco before any components use it
import { useAppStore } from './store/app'
import { useSessionNamesStore } from './store/sessionNames'
import App from './App'
import { startWorkspaceNotifications } from './store/startWorkspaceNotifications'
import { createDingPlayer } from './audio/ding'
import { useActivityStateStore } from './store/activityState'
import { useNavigationStore } from './store/navigation'
import { useSettingsStore } from './store/settings'
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

  const reportError = (error: unknown): void => {
    useAppStore.setState({ notificationError: error instanceof Error ? error.message : String(error) })
  }
  const stopNotifications = startWorkspaceNotifications({
    getSessions: () => Array.from(useAppStore.getState().sessionStores.values(), entry => entry.store),
    subscribeSessions: listener => useAppStore.subscribe(listener),
    activityStore: useActivityStateStore,
    navigationStore: useNavigationStore,
    soundEnabled: () => useSettingsStore.getState().settings.notifications.soundEnabled,
    reportError,
  }, createDingPlayer())
  window.addEventListener('pagehide', stopNotifications, { once: true })

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
    selectFile: e.selectFile,
    getWindowUuid: e.getWindowUuid,
    getInitialWorkspace: e.getInitialWorkspace,
    openExternal: (url: string) => { window.open(url, '_blank') },
    getViewportSize: () => ({ width: window.innerWidth, height: window.innerHeight }),
    keyEventTarget: window,
    isKeyDiagEnabled: () => !!window.__enableKeyDiag,
    sessionNamesStore: useSessionNamesStore,
  })

  const rootEl = document.getElementById('root')
  if (!rootEl) throw new Error('Root element not found')
  ReactDOM.createRoot(rootEl).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>
  )
})
