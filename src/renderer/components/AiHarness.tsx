import { useCallback, useState } from 'react'
import { useStore } from 'zustand'
import type { StoreApi } from 'zustand'
import BaseTerminal, { type BaseTerminalConfig } from './BaseTerminal'
import { createXtermEngine } from '../terminal/xtermEngine'
import type { TerminalEngine } from '../terminal/engine'
import { PromptCommitButton } from './PromptCommitButton'
import { PromptRebaseButton } from './PromptRebaseButton'
import { ReviewCommentsButton } from './ReviewCommentsButton'
import { PromptGitHubCommentsButton } from './PromptGitHubCommentsButton'
import type { SandboxConfig, WorkspaceStore } from '../types'
import { isAiHarnessState } from '../types'
import type { AiHarnessRef } from '../../applications/aiHarness/renderer'
import type { AnalyzerState } from '../store/createAnalyzerStore'
import { ToggleSwitch } from './ToggleSwitch'
import { IdleDetectorToggle } from './IdleDetectorToggle'
import { ActivityStateBadge } from './ActivityStateBadge'

interface AiHarnessProps {
  cwd: string
  workspace: WorkspaceStore
  tabId: string
  sandbox?: SandboxConfig
  isVisible?: boolean
  command: string
  backgroundColor: string
  disableScrollbar?: boolean
}

export default function AiHarness({
  workspace,
  tabId,
  backgroundColor,
  disableScrollbar,
}: AiHarnessProps) {
  const wsData = useStore(workspace, s => s.workspace)
  const getTabRef = useStore(workspace, s => s.getTabRef)
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

  const ref = getTabRef(tabId) as AiHarnessRef | null
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
    />
  )
}

interface AiHarnessContentProps {
  workspace: WorkspaceStore
  tabId: string
  analyzer: AiHarnessRef['analyzer']
  backgroundColor: string
  disableScrollbar?: boolean
}

function AiHarnessContent({
  workspace,
  tabId,
  analyzer,
  backgroundColor,
  disableScrollbar,
}: AiHarnessContentProps) {
  const handleTerminalReady = useCallback((engine: TerminalEngine) => {
    // onTerminalReady fires once per engine, and engine.dispose() tears its own listeners
    // down, so this subscription's lifetime is already the engine's.
    // eslint-disable-next-line custom/no-discarded-disposable -- owned by the engine
    engine.onData((data) => {
      analyzer.getState().onUserInput(data)
    })
  }, [analyzer])

  // Stable config — useState initializer runs once, so BaseTerminal never re-renders from config changes
  const [config] = useState<BaseTerminalConfig>(() => ({
    createEngine: createXtermEngine,
    themeBackground: backgroundColor,
    logPrefix: 'AiHarness',
    disableScrollbar,
    disableActivityDetector: true,
    onTerminalReady: handleTerminalReady,
  }))

  return (
    <div className="terminal-app-wrapper">
      <div className="terminal-app-body">
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

  return (
    <div className="terminal-status-bar">
      <ActivityStateBadge
        workspace={workspace}
        tabId={tabId}
        state={aiState}
        analyzing={analyzing}
        title={reason}
        getBufferText={() => analyzer.getState().getBufferText() ?? ''}
      />
      <ToggleSwitch
        checked={autoApprove}
        label="Auto-approve safe"
        onChange={(checked) => { analyzer.getState().setAutoApprove(checked) }}
      />
      <IdleDetectorToggle workspace={workspace} tabId={tabId} />
    </div>
  )
}
