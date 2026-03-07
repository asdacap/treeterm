interface TerminalToolbarProps {
  onScrollDown: () => void
}

export default function TerminalToolbar({ onScrollDown }: TerminalToolbarProps) {
  return (
    <div className="terminal-toolbar">
      <button
        className="terminal-toolbar-btn"
        onClick={onScrollDown}
        title="Scroll to bottom"
      >
        ↓
      </button>
    </div>
  )
}
