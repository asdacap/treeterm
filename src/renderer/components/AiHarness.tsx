import { useCallback, useState } from 'react'
import { useStore } from 'zustand'
import type { StoreApi } from 'zustand'
import type { Terminal as XTerm } from '@xterm/xterm'
import BaseTerminal, { type BaseTerminalConfig } from './BaseTerminal'
import { PromptCommitButton } from './PromptCommitButton'
import { PromptRebaseButton } from './PromptRebaseButton'
import { ReviewCommentsButton } from './ReviewCommentsButton'
import { PromptGitHubCommentsButton } from './PromptGitHubCommentsButton'
import type { ActivityState, SandboxConfig, WorkspaceStore } from '../types'
import { isAiHarnessState } from '../types'
import type { AiHarnessRef } from '../../applications/aiHarness/renderer'
import type { AnalyzerState } from '../store/createAnalyzerStore'
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
  workspace,
  tabId,
  backgroundColor,
  disableScrollbar,
  stripScrollbackClear,
}: AiHarnessProps) {
  const { workspace: wsData } = useStore(workspace)
  const appState = wsData.appStates[tabId]

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
  analyzer: AiHarnessRef['analyzer']
  backgroundColor: string
  disableScrollbar?: boolean
  stripScrollbackClear?: boolean
}

function AiHarnessContent({
  workspace,
  tabId,
  analyzer,
  backgroundColor,
  disableScrollbar,
  stripScrollbackClear,
}: AiHarnessContentProps) {
  const handleTerminalReady = useCallback((term: XTerm) => {
    term.onData((data) => {
      analyzer.getState().onUserInput(data)
    })
  }, [analyzer])

  // Stable config — useState initializer runs once, so BaseTerminal never re-renders from config changes
  const [config] = useState<BaseTerminalConfig>(() => ({
    themeBackground: backgroundColor,
    promptPatterns: [/❯\s/],
    logPrefix: 'AiHarness',
    disableScrollbar,
    stripScrollbackClear,
    disableActivityDetector: true,
    onTerminalReady: handleTerminalReady,
  }))

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
            </>
          }
        />
      </div>
      <AiHarnessStatusBar analyzer={analyzer} workspace={workspace} tabId={tabId} />
    </div>
  )
}

interface AiHarnessStatusBarProps {
  analyzer: StoreApi<AnalyzerState>
  workspace: WorkspaceStore
  tabId: string
}

function AiHarnessStatusBar({ analyzer, workspace, tabId }: AiHarnessStatusBarProps) {
  const aiState = useStore(analyzer, s => s.aiState)
  const analyzing = useStore(analyzer, s => s.analyzing)
  const reason = useStore(analyzer, s => s.reason)
  const autoApprove = useStore(analyzer, s => s.autoApprove)

  const openContextMenu = useContextMenuStore((s) => s.open)
  const closeContextMenu = useContextMenuStore((s) => s.close)
  const activeMenuId = useContextMenuStore((s) => s.activeMenuId)
  const menuPosition = useContextMenuStore((s) => s.position)
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
    workspace.getState().addTab('system-prompt-debugger', { bufferText })
  }

  const handleViewHistory = () => {
    closeContextMenu()
    workspace.getState().addTab('analyzer-history', { sourceTabId: tabId })
  }

  return (
    <>
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
            onChange={(e) => { analyzer.getState().setAutoApprove(e.target.checked); }}
          />
          <span className="ai-harness-toggle-slider" />
          <span className="ai-harness-toggle-label">Auto-approve safe</span>
        </label>
      </div>
      <ContextMenu menuId={badgeMenuId} activeMenuId={activeMenuId} position={menuPosition}>
        <div className="context-menu-item" onClick={handleDebugAnalyzer}>
          Debug System Prompt
        </div>
        <div className="context-menu-item" onClick={handleViewHistory}>
          History
        </div>
      </ContextMenu>
    </>
  )
}
