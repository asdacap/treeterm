import { useMemo } from 'react'
import BaseTerminal, { type BaseTerminalConfig } from './BaseTerminal'
import type { SandboxConfig } from '../types'

interface ClaudeProps {
  cwd: string
  workspaceId: string
  tabId: string
  sandbox?: SandboxConfig
  isVisible?: boolean
}

// Claude-specific configuration
const CLAUDE_CONFIG: BaseTerminalConfig = {
  themeBackground: '#1a1a24', // Slight purple tint for Claude
  promptPatterns: [/❯\s/], // Claude uses ❯ prompt (no $ anchor - UI draws out of order)
  startupCommand: 'claude',
  logPrefix: 'Claude'
}

export default function Claude({ cwd, workspaceId, tabId, sandbox, isVisible }: ClaudeProps) {
  // Memoize config to prevent unnecessary re-renders
  const config = useMemo(() => CLAUDE_CONFIG, [])

  return (
    <BaseTerminal
      cwd={cwd}
      workspaceId={workspaceId}
      tabId={tabId}
      sandbox={sandbox}
      isVisible={isVisible}
      config={config}
    />
  )
}
