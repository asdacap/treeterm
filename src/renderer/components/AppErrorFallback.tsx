import { useState } from 'react'

interface AppErrorFallbackProps {
  error?: Error
}

export default function AppErrorFallback({ error }: AppErrorFallbackProps) {
  const [showStack, setShowStack] = useState(false)

  const handleReload = () => {
    window.location.reload()
  }

  return (
    <div className="app-error-fallback">
      <div className="app-error-content">
        <h1>Application Error</h1>
        <p>The application encountered an unexpected error and needs to reload.</p>

        {error && (
          <div className="app-error-details">
            <div className="app-error-message">
              <strong>Error:</strong> {error.message}
            </div>

            {error.stack && (
              <div className="app-error-stack-container">
                <button
                  className="app-error-stack-toggle"
                  onClick={() => setShowStack(!showStack)}
                >
                  {showStack ? 'Hide' : 'Show'} Stack Trace
                </button>
                {showStack && (
                  <pre className="app-error-stack">{error.stack}</pre>
                )}
              </div>
            )}
          </div>
        )}

        <button className="app-error-reload-btn" onClick={handleReload}>
          Reload Application
        </button>
      </div>
    </div>
  )
}
