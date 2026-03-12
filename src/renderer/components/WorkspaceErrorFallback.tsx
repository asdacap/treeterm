interface WorkspaceErrorFallbackProps {
  error?: Error
  onReset?: () => void
}

export default function WorkspaceErrorFallback({ error, onReset }: WorkspaceErrorFallbackProps) {
  return (
    <div className="workspace-error-fallback">
      <div className="workspace-error-content">
        <h2>Workspace Error</h2>
        <p>The workspace panel encountered an error.</p>

        {error && (
          <div className="workspace-error-message">
            <strong>Error:</strong> {error.message}
          </div>
        )}

        {onReset && (
          <button className="workspace-error-reload-btn" onClick={onReset}>
            Reload Workspace
          </button>
        )}
      </div>
    </div>
  )
}
