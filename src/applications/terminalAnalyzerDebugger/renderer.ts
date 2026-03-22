import { createElement } from 'react'
import type { Application, ApplicationRenderProps } from '../../renderer/types'
import SystemPromptDebugger from '../../renderer/components/SystemPromptDebugger'

export const systemPromptDebuggerApplication: Application = {
  id: 'system-prompt-debugger',
  name: 'System Prompt Debugger',
  icon: '🔬',
  createInitialState: () => ({}),
  render: (props: ApplicationRenderProps) => createElement(SystemPromptDebugger, props),
  canClose: true,
  canHaveMultiple: false,
  showInNewTabMenu: true,
  keepAlive: false,
  displayStyle: 'flex',
  isDefault: false
}
