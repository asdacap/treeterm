import { useCallback } from 'react'
import type { Terminal as XTerm } from '@xterm/xterm'

interface TerminalScrollWrapperProps {
  terminalRef: React.RefObject<XTerm | null>
  children: React.ReactNode
}

export default function TerminalScrollWrapper({ terminalRef, children }: TerminalScrollWrapperProps) {
  const handleScrollDown = useCallback(() => {
    if (terminalRef.current) {
      terminalRef.current.scrollToBottom()
    }
  }, [terminalRef])

  return (
    <div className="terminal-wrapper">
      {children}
      <button
        className="scroll-down-btn"
        onClick={handleScrollDown}
        title="Scroll to bottom"
      >
        ↓
      </button>
    </div>
  )
}
