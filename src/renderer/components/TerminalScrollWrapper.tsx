import { useCallback } from 'react'
import type { Terminal as XTerm } from '@xterm/xterm'

interface TerminalScrollWrapperProps {
  terminalRef: React.RefObject<XTerm | null>
  scrollPosition: 'top' | 'bottom' | 'middle'
  isAlternateScreen?: boolean
  extraButtons?: React.ReactNode
  children: React.ReactNode
}

export default function TerminalScrollWrapper({
  terminalRef,
  scrollPosition,
  isAlternateScreen,
  extraButtons,
  children,
}: TerminalScrollWrapperProps) {
  const handleScrollDown = useCallback(() => {
    terminalRef.current?.scrollToBottom()
  }, [terminalRef])

  const handleScrollToTop = useCallback(() => {
    terminalRef.current?.scrollToTop()
  }, [terminalRef])

  const handleBadgeClick = scrollPosition === 'bottom' ? handleScrollToTop : handleScrollDown

  return (
    <div className="terminal-wrapper">
      {children}
      <button
        className={`scroll-position-badge scroll-position-${scrollPosition}`}
        onClick={handleBadgeClick}
        title={scrollPosition === 'bottom' ? 'Scroll to top' : 'Scroll to bottom'}
      >
        {scrollPosition.toUpperCase()}
      </button>
      {isAlternateScreen && (
        <span className="alt-screen-badge" title="Terminal is in alternate screen mode (no scrollback)">
          ALT SCREEN
        </span>
      )}
      <div className="terminal-floating-buttons">
        {extraButtons}
        <button className="scroll-down-btn terminal-circle-btn" onClick={handleScrollDown} title="Scroll to bottom">
          ↓
        </button>
      </div>
    </div>
  )
}
