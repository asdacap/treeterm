import { useState } from 'react'

interface AppErrorFallbackProps {
  error?: Error
  onReload: () => void
}

export default function AppErrorFallback({ error, onReload }: AppErrorFallbackProps) {
  const [showStack, setShowStack] = useState(false)

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
                  onClick={() => { setShowStack(!showStack); }}
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

        <button className="app-error-reload-btn" onClick={onReload}>
          Reload Application
        </button>
      </div>
    </div>
  )
}
