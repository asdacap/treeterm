import { useCallback } from 'react'
import type { Terminal as XTerm } from '@xterm/xterm'
import type { StoreApi } from 'zustand'
import type { WorkspaceState } from '../store/createWorkspaceStore'
import PushToTalkButton from './PushToTalkButton'
import { ReviewCommentsButton } from './ReviewCommentsButton'

interface TerminalScrollWrapperProps {
  terminalRef: React.RefObject<XTerm | null>
  children: React.ReactNode
  showPushToTalk?: boolean
  onPushToTalkTranscript?: (text: string) => void
  onPushToTalkSubmit?: () => void
  showReviewComments?: boolean
  workspaceStore?: StoreApi<WorkspaceState>
  workspaceId?: string
  ptyId?: string
}

export default function TerminalScrollWrapper({
  terminalRef,
  children,
  showPushToTalk = false,
  onPushToTalkTranscript,
  onPushToTalkSubmit,
  showReviewComments = false,
  workspaceStore,
  workspaceId,
  ptyId
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
      {showReviewComments && workspaceStore && workspaceId && ptyId && (
        <ReviewCommentsButton
          workspaceStore={workspaceStore}
          workspaceId={workspaceId}
          ptyId={ptyId}
        />
      )}
      <button className="scroll-down-btn" onClick={handleScrollDown} title="Scroll to bottom">
        ↓
      </button>
    </div>
  )
}
