import { createElement } from 'react'
import type { Application, ApplicationRenderProps, ChatState } from '../../renderer/types'
import Chat from '../../renderer/components/Chat'

export const chatApplication: Application<ChatState> = {
  id: 'chat',
  name: 'Chat',
  icon: 'M',
  createInitialState: (): ChatState => ({
    messages: []
  }),
  onWorkspaceLoad: () => ({ dispose: () => {} }),
  render: (props: ApplicationRenderProps) => createElement(Chat, props),
  canClose: true,
  showInNewTabMenu: true,
  displayStyle: 'flex',
  isDefault: false
}
