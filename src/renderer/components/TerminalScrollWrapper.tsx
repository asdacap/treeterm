import { useCallback } from 'react'
import type { Terminal as XTerm } from '@xterm/xterm'
import PushToTalkButton from './PushToTalkButton'

interface TerminalScrollWrapperProps {
  terminalRef: React.RefObject<XTerm | null>
  children: React.ReactNode
  showPushToTalk?: boolean
  onPushToTalkTranscript?: (text: string) => void
  onPushToTalkSubmit?: () => void
}

export default function TerminalScrollWrapper({
  terminalRef,
  children,
  showPushToTalk = false,
  onPushToTalkTranscript,
  onPushToTalkSubmit
}: TerminalScrollWrapperProps) {
  const handleScrollDown = useCallback(() => {
    if (terminalRef.current) {
      terminalRef.current.scrollToBottom()
    }
  }, [terminalRef])

  return (
    <div className="terminal-wrapper">
      {children}
      {showPushToTalk && onPushToTalkTranscript && onPushToTalkSubmit && (
        <PushToTalkButton
          onTranscript={onPushToTalkTranscript}
          onSubmit={onPushToTalkSubmit}
        />
      )}
      <button className="scroll-down-btn" onClick={handleScrollDown} title="Scroll to bottom">
        ↓
      </button>
    </div>
  )
}
