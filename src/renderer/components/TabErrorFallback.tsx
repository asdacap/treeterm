interface TabErrorFallbackProps {
  error: Error
  tabTitle: string
  onReset: () => void
  onClose: () => void
}

export default function TabErrorFallback({ error, tabTitle, onReset, onClose }: TabErrorFallbackProps) {
  return (
    <div className="tab-error-fallback">
      <div className="tab-error-content">
        <h3>Tab Error</h3>
        <p>The tab "{tabTitle}" encountered an error and stopped working.</p>

        <div className="tab-error-message">
          <strong>Error:</strong> {error.message}
        </div>

        <div className="tab-error-actions">
          <button className="tab-error-reload-btn" onClick={onReset}>
            Reload Tab
          </button>
          <button className="tab-error-close-btn" onClick={onClose}>
            Close Tab
          </button>
        </div>
      </div>
    </div>
  )
}
