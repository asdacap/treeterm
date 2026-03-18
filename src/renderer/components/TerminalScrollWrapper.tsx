import { useCallback } from 'react'
import type { Terminal as XTerm } from '@xterm/xterm'
import PushToTalkButton from './PushToTalkButton'
import { ReviewCommentsButton } from './ReviewCommentsButton'
import { PromptDescriptionButton } from './PromptDescriptionButton'

interface TerminalScrollWrapperProps {
  terminalRef: React.RefObject<XTerm | null>
  children: React.ReactNode
  showPushToTalk?: boolean
  onPushToTalkTranscript?: (text: string) => void
  onPushToTalkSubmit?: () => void
  workspacePath?: string
  ptyId?: string
  promptDescription?: string
  onPromptDescriptionDismiss?: () => void
}

export default function TerminalScrollWrapper({
  terminalRef,
  children,
  showPushToTalk = false,
  onPushToTalkTranscript,
  onPushToTalkSubmit,
  workspacePath,
  ptyId,
  promptDescription,
  onPromptDescriptionDismiss
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
      {workspacePath && ptyId && (
        <ReviewCommentsButton
          workspacePath={workspacePath}
          ptyId={ptyId}
        />
      )}
      {promptDescription && onPromptDescriptionDismiss && ptyId && (
        <PromptDescriptionButton
          description={promptDescription}
          ptyId={ptyId}
          onDismiss={onPromptDescriptionDismiss}
        />
      )}
      <button className="scroll-down-btn" onClick={handleScrollDown} title="Scroll to bottom">
        ↓
      </button>
    </div>
  )
}
