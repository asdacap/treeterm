import { createElement } from 'react'
import type { Application, ApplicationRenderProps } from '../../renderer/types'
import SystemPromptDebugger from '../../renderer/components/SystemPromptDebugger'

export const systemPromptDebuggerApplication: Application = {
  id: 'system-prompt-debugger',
  name: 'System Prompt Debugger',
  icon: '🔬',
  createInitialState: () => ({}),
  onWorkspaceLoad: () => ({ close: () => {}, dispose: () => {} }),
  render: (props: ApplicationRenderProps) => createElement(SystemPromptDebugger, props),
  canClose: true,
  showInNewTabMenu: true,
  displayStyle: 'flex',
  isDefault: false
}
