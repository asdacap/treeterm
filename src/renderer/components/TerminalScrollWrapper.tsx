import { useCallback } from 'react'
import type { Terminal as XTerm } from '@xterm/xterm'
import TerminalToolbar from './TerminalToolbar'

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
      <TerminalToolbar onScrollDown={handleScrollDown} />
      {children}
    </div>
  )
}
