import { createElement } from 'react'
import type { Application, ApplicationRenderProps } from '../../renderer/types'
import TerminalAnalyzerDebugger from '../../renderer/components/TerminalAnalyzerDebugger'

export const terminalAnalyzerDebuggerApplication: Application = {
  id: 'analyzer-debugger',
  name: 'Analyzer Debugger',
  icon: '🔬',
  createInitialState: () => ({}),
  render: (props: ApplicationRenderProps) => createElement(TerminalAnalyzerDebugger, props),
  canClose: true,
  canHaveMultiple: false,
  showInNewTabMenu: true,
  keepAlive: false,
  displayStyle: 'flex',
  isDefault: false
}
