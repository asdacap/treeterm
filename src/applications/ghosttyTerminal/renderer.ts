import { createElement } from 'react'
import type { Application, TerminalState } from '../../renderer/types'
import GhosttyTerminal from '../../renderer/components/GhosttyTerminal'
import { makeTerminalOnWorkspaceLoad, type TerminalDeps } from '../terminal/renderer'

/**
 * A second terminal application rendered by ghostty-web instead of xterm.js.
 *
 * PTY lifecycle is shared with the xterm terminal via makeTerminalOnWorkspaceLoad — only the
 * frontend differs, so both apps create, reattach to and kill PTYs identically. The returned
 * ref's `cachedTerminal` stays null: GhosttyTerminal disposes its terminal on unmount and
 * rebuilds from the daemon's replay on remount.
 */
export function createGhosttyTerminalApplication(deps: TerminalDeps): Application<TerminalState> {
  return {
    id: 'ghostty-terminal',
    name: 'Terminal (Ghostty)',
    icon: '👻',

    createInitialState: () => ({
      ptyId: null,
      ptyHandle: crypto.randomUUID(),
      keepOnExit: false
    }),

    onWorkspaceLoad: makeTerminalOnWorkspaceLoad(deps),

    render: ({ tab, workspace }) => createElement(GhosttyTerminal, {
      key: tab.id,
      workspace,
      tabId: tab.id,
    }),

    canClose: true,

    showInNewTabMenu: true,
    displayStyle: 'flex',
    isDefault: false
  }
}
