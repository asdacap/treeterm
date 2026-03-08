import React from 'react'
import ReactDOM from 'react-dom/client'
import './monaco-config' // Configure Monaco before any components use it
import App from './App'
import './styles/index.css'

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
)
