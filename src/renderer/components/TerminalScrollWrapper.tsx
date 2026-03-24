import { useCallback } from 'react'
import type { Terminal as XTerm } from '@xterm/xterm'

interface TerminalScrollWrapperProps {
  terminalRef: React.RefObject<XTerm | null>
  extraButtons?: React.ReactNode
  children: React.ReactNode
}

export default function TerminalScrollWrapper({
  terminalRef,
  extraButtons,
  children,
}: TerminalScrollWrapperProps) {
  const handleScrollDown = useCallback(() => {
    if (terminalRef.current) {
      terminalRef.current.scrollToBottom()
    }
  }, [terminalRef])

  return (
    <div className="terminal-wrapper">
      {children}
      <div className="terminal-floating-buttons">
        {extraButtons}
        <button className="scroll-down-btn" onClick={handleScrollDown} title="Scroll to bottom">
          ↓
        </button>
      </div>
    </div>
  )
}
