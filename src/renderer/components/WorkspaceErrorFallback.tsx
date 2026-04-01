import type { FallbackProps } from './ErrorBoundary'

export default function WorkspaceErrorFallback({ error, reset }: FallbackProps) {
  return (
    <div className="workspace-error-fallback">
      <div className="workspace-error-content">
        <h2>Workspace Error</h2>
        <p>The workspace panel encountered an error.</p>

        <div className="workspace-error-message">
          <strong>Error:</strong> {error.message}
        </div>

        <button className="workspace-error-reload-btn" onClick={reset}>
          Reload Workspace
        </button>
      </div>
    </div>
  )
}
