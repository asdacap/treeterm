import React from 'react'
import ReactDOM from 'react-dom/client'
import './monaco-config' // Configure Monaco before any components use it
import { initializeApplications } from '../applications'
import App from './App'
import './styles/index.css'

// Wait for main process ready signal, then initialize and render
window.electron.app.onReady(() => {
  initializeApplications({ terminal: window.electron.terminal })

  ReactDOM.createRoot(document.getElementById('root')!).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>
  )
})
