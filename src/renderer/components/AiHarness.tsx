import { useCallback, useMemo } from 'react'
import { useStore } from 'zustand'
import type { Terminal as XTerm } from '@xterm/xterm'
import BaseTerminal, { type BaseTerminalConfig } from './BaseTerminal'
import PushToTalkButton from './PushToTalkButton'
import { PromptCommitButton } from './PromptCommitButton'
import { PromptRebaseButton } from './PromptRebaseButton'
import { ReviewCommentsButton } from './ReviewCommentsButton'
import { PromptGitHubCommentsButton } from './PromptGitHubCommentsButton'
import { useSessionApi } from '../contexts/SessionStoreContext'
import type { ActivityState, SandboxConfig, WorkspaceStore } from '../types'
import { isAiHarnessState } from '../types'
import type { AiHarnessRef } from '../../applications/aiHarness/renderer'
import { useContextMenuStore } from '../store/contextMenu'
import ContextMenu from './ContextMenu'

const STATE_COLORS: Record<ActivityState, string> = {
  idle: '#666',
  working: '#2472c8',
  user_input_required: '#e5e510',
  permission_request: '#cd6600',
  safe_permission_requested: '#0dbc79',
  completed: '#23d18b',
  error: '#f44747'
}

const STATE_LABELS: Record<ActivityState, string> = {
  idle: 'idle',
  working: 'working',
  user_input_required: 'input required',
  permission_request: 'permission request',
  safe_permission_requested: 'safe permission',
  completed: 'completed',
  error: 'error'
}

interface AiHarnessProps {
  cwd: string
  workspace: WorkspaceStore
  tabId: string
  sandbox?: SandboxConfig
  isVisible?: boolean
  command: string
  backgroundColor: string
  disableScrollbar?: boolean
  stripScrollbackClear?: boolean
}

export default function AiHarness({
  cwd: _cwd,
  workspace,
  tabId,
  sandbox: _sandbox,
  isVisible: _isVisible,
  command: _command,
  backgroundColor,
  disableScrollbar,
  stripScrollbackClear,
}: AiHarnessProps) {
  const { workspace: wsData } = useStore(workspace)
  const appState = wsData?.appStates[tabId]

  if (!appState) {
    return <div style={{ padding: 16, color: '#888' }}>Loading AI harness...</div>
  }
  if (!isAiHarnessState(appState.state)) {
    return <div style={{ padding: 16, color: '#f44747' }}>Error: Invalid AI harness state</div>
  }

  const ptyId = appState.state.ptyId
  if (!ptyId) {
    return <div style={{ padding: 16, color: '#888' }}>Starting AI harness...</div>
  }

  const ref = workspace.getState().getTabRef(tabId) as AiHarnessRef | null
  if (!ref?.analyzer) {
    return <div style={{ padding: 16, color: '#888' }}>Starting AI harness...</div>
  }

  return (
    <AiHarnessContent
      workspace={workspace}
      tabId={tabId}
      ptyId={ptyId}
      analyzer={ref.analyzer}
      backgroundColor={backgroundColor}
      disableScrollbar={disableScrollbar}
      stripScrollbackClear={stripScrollbackClear}
    />
  )
}

interface AiHarnessContentProps {
  workspace: WorkspaceStore
  tabId: string
  ptyId: string
  analyzer: AiHarnessRef['analyzer']
  backgroundColor: string
  disableScrollbar?: boolean
  stripScrollbackClear?: boolean
}

function AiHarnessContent({
  workspace,
  tabId,
  ptyId,
  analyzer,
  backgroundColor,
  disableScrollbar,
  stripScrollbackClear,
}: AiHarnessContentProps) {
  const sessionStore = useSessionApi()
  const { aiState, analyzing, reason, autoApprove } = useStore(analyzer)

  const handlePushToTalkTranscript = useCallback(async (text: string) => {
    const writer = await sessionStore.getState().getTtyWriter(ptyId)
    writer.write(text)
  }, [ptyId, sessionStore])

  const handlePushToTalkSubmit = useCallback(async () => {
    const writer = await sessionStore.getState().getTtyWriter(ptyId)
    writer.write('\r')
  }, [ptyId, sessionStore])

  const handleTerminalReady = useCallback((term: XTerm) => {
    term.onData((data) => {
      analyzer.getState().onUserInput(data)
    })
  }, [analyzer])

  const openContextMenu = useContextMenuStore((s) => s.open)
  const closeContextMenu = useContextMenuStore((s) => s.close)
  const badgeMenuId = `ai-badge-${tabId}`

  const handleBadgeContextMenu = (e: React.MouseEvent) => {
    e.preventDefault()
    e.stopPropagation()
    openContextMenu(badgeMenuId, e.clientX, e.clientY)
  }

  const handleDebugAnalyzer = () => {
    closeContextMenu()
    const bufferText = analyzer.getState().getBufferText()
    if (!bufferText) return
    workspace.getState().addTab<{ bufferText: string }>('system-prompt-debugger', { bufferText })
  }

  const handleViewHistory = () => {
    closeContextMenu()
    workspace.getState().addTab<{ sourceTabId: string }>('analyzer-history', { sourceTabId: tabId })
  }

  const config = useMemo<BaseTerminalConfig>(() => ({
    themeBackground: backgroundColor,
    promptPatterns: [/❯\s/],
    logPrefix: 'AiHarness',
    disableScrollbar,
    stripScrollbackClear,
    disableActivityDetector: true,
    onTerminalReady: handleTerminalReady,
  }), [backgroundColor, disableScrollbar, stripScrollbackClear, handleTerminalReady])

  return (
    <div className="ai-harness-wrapper">
      <div className="ai-harness-terminal">
        <BaseTerminal
          workspace={workspace}
          tabId={tabId}
          config={config}
          extraButtons={
            <>
              <PromptCommitButton workspace={workspace} />
              <PromptRebaseButton workspace={workspace} />
              <ReviewCommentsButton workspace={workspace} />
              <PromptGitHubCommentsButton workspace={workspace} />
              <PushToTalkButton
                onTranscript={handlePushToTalkTranscript}
                onSubmit={handlePushToTalkSubmit}
              />
            </>
          }
        />
      </div>
      <div className="ai-harness-status-bar">
        <div
          className="ai-state-badge"
          style={{ background: STATE_COLORS[aiState] }}
          title={reason}
          onContextMenu={handleBadgeContextMenu}
        >
          {analyzing && <span className="ai-state-badge-spinner" />}
          {STATE_LABELS[aiState]}
        </div>
        <label className="ai-harness-toggle">
          <input
            type="checkbox"
            checked={autoApprove}
            onChange={(e) => analyzer.getState().setAutoApprove(e.target.checked)}
          />
          <span className="ai-harness-toggle-slider" />
          <span className="ai-harness-toggle-label">Auto-approve safe</span>
        </label>
      </div>
      <ContextMenu menuId={badgeMenuId}>
        <div className="context-menu-item" onClick={handleDebugAnalyzer}>
          Debug System Prompt
        </div>
        <div className="context-menu-item" onClick={handleViewHistory}>
          History
        </div>
      </ContextMenu>
    </div>
  )
}
