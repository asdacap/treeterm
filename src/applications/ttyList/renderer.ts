import { createElement } from 'react'
import type { Application, ApplicationRenderProps } from '../../renderer/types'
import TtyListBrowser from '../../renderer/components/TtyListBrowser'

// eslint-disable-next-line @typescript-eslint/no-empty-object-type -- intentional marker interface for tab state
export interface TtyListState {}

export const ttyListApplication: Application<TtyListState> = {
  id: 'tty-list',
  name: 'TTYs',
  icon: '\u2630',
  createInitialState: () => ({}),
  onWorkspaceLoad: () => ({ close: () => {}, dispose: () => {} }),
  render: (props: ApplicationRenderProps) => createElement(TtyListBrowser, props),
  canClose: true,
  showInNewTabMenu: true,
  displayStyle: 'flex',
  isDefault: false,
}
