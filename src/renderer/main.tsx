import React from 'react'
import ReactDOM from 'react-dom/client'
import './monaco-config' // Configure Monaco before any components use it
import { useAppStore } from './store/app'
import App from './App'
import './styles/index.css'

// Single point of window.electron access — everything else reads from the store
window.electron.app.onReady(() => {
  const e = window.electron

  useAppStore.getState().initialize({
    platform: e.platform,
    terminal: e.terminal,
    git: e.git,
    filesystem: e.filesystem,
    stt: e.stt,
    sandbox: e.sandbox,
    sessionApi: e.session,
    settingsApi: e.settings,
    appApi: e.app,
    daemon: e.daemon,
    selectFolder: e.selectFolder,
    getRecentDirectories: e.getRecentDirectories,
    getWindowUuid: e.getWindowUuid,
    getInitialWorkspace: e.getInitialWorkspace,
  })

  ReactDOM.createRoot(document.getElementById('root')!).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>
  )
})
