import { createElement } from 'react'
import type { Application, ApplicationRenderProps } from '../../renderer/types'
import AnalyzerHistory from '../../renderer/components/AnalyzerHistory'

export const analyzerHistoryApplication: Application = {
  id: 'analyzer-history',
  name: 'Analyzer History',
  icon: '📊',
  createInitialState: () => ({}),
  render: (props: ApplicationRenderProps) => createElement(AnalyzerHistory, props),
  canClose: true,
  canHaveMultiple: true,
  showInNewTabMenu: false,
  displayStyle: 'flex',
  isDefault: false
}
