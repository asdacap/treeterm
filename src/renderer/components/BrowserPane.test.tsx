// @vitest-environment jsdom
import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { createStore } from 'zustand/vanilla'
import BrowserPane from './BrowserPane'
import type { PageSsh } from './PageViews'
import { WebAppPhase } from '../../applications/webApp/runtime'
import type { PageRef, WebAppRuntime } from '../../applications/webApp/runtime'
import type { WorkspaceStore } from '../types'
import type { WorkspaceStoreState } from '../store/createWorkspaceStore'

function makeWorkspace(runtime: WebAppRuntime, isRemote: boolean): WorkspaceStore {
  const ref: PageRef = {
    runtime: createStore<WebAppRuntime>(() => runtime),
    retry: vi.fn(),
    restartForward: vi.fn(() => Promise.resolve()),
    pageUrl: vi.fn((home: string) => home),
    rememberPage: vi.fn(),
    close: vi.fn(),
    dispose: vi.fn(),
  }
  return createStore<WorkspaceStoreState>()(() => ({
    isRemote, connectionId: 'c', getTabRef: () => ref,
  }) as unknown as WorkspaceStoreState)
}

const ssh = {
  listPortForwards: vi.fn(() => Promise.resolve([])),
  onPortForwardStatus: vi.fn(() => vi.fn()),
  watchPortForwardOutput: vi.fn(),
} as unknown as PageSsh

describe('BrowserPane', () => {
  it('local: shows just the page, no sub-tabs and no port forward link', () => {
    const { container } = render(
      <BrowserPane workspace={makeWorkspace({ phase: WebAppPhase.Error, message: 'bad url' }, false)} tabId="t" ssh={ssh} openExternal={vi.fn()} />,
    )
    expect(screen.getByText('bad url')).toBeTruthy()
    expect(screen.queryByText('Port Forward')).toBeNull()
    expect(screen.queryByText('View port forward')).toBeNull()
    expect(container.querySelector('.ssh-pane-subtabs')).toBeNull()
  })

  it('remote: offers the Port Forward sub-tab and links to it while forwarding', async () => {
    render(<BrowserPane workspace={makeWorkspace({ phase: WebAppPhase.Forwarding, port: 3000 }, true)} tabId="t" ssh={ssh} openExternal={vi.fn()} />)
    expect(screen.getByText('Starting port forward for port 3000…')).toBeTruthy()
    fireEvent.click(screen.getByText('View port forward'))
    expect(await screen.findByText(/No port forward yet/)).toBeTruthy()
    fireEvent.click(screen.getByText('Browser'))
    fireEvent.click(screen.getByText('Port Forward'))
    expect(ssh.listPortForwards).toHaveBeenCalledWith('c')
  })
})
