import { createElement } from 'react'
import type { Application, ApplicationRenderProps } from '../../renderer/types'
import TerminalAnalyzerDebugger from '../../renderer/components/TerminalAnalyzerDebugger'

export const terminalAnalyzerDebuggerApplication: Application = {
  id: 'analyzer-debugger',
  name: 'Analyzer Debugger',
  icon: '🔬',
  createInitialState: () => ({}),
  render: (_props: ApplicationRenderProps) => createElement(TerminalAnalyzerDebugger),
  canClose: true,
  canHaveMultiple: false,
  showInNewTabMenu: true,
  keepAlive: false,
  displayStyle: 'flex',
  isDefault: false
}
