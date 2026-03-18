import { useCallback } from 'react'
import type { Terminal as XTerm } from '@xterm/xterm'
import PushToTalkButton from './PushToTalkButton'
import { ReviewCommentsButton } from './ReviewCommentsButton'

interface TerminalScrollWrapperProps {
  terminalRef: React.RefObject<XTerm | null>
  children: React.ReactNode
  showPushToTalk?: boolean
  onPushToTalkTranscript?: (text: string) => void
  onPushToTalkSubmit?: () => void
  showReviewComments?: boolean
  workspacePath?: string
  ptyId?: string
  reviewId?: string
}

export default function TerminalScrollWrapper({
  terminalRef,
  children,
  showPushToTalk = false,
  onPushToTalkTranscript,
  onPushToTalkSubmit,
  showReviewComments = false,
  workspacePath,
  ptyId,
  reviewId
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
      {showReviewComments && workspacePath && ptyId && (
        <ReviewCommentsButton
          workspacePath={workspacePath}
          ptyId={ptyId}
          reviewId={reviewId}
        />
      )}
      <button className="scroll-down-btn" onClick={handleScrollDown} title="Scroll to bottom">
        ↓
      </button>
    </div>
  )
}
