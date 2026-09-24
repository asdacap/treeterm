// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, act, waitFor } from '@testing-library/react'
import { BrowserView, PortForwardView } from './PageViews'
import type { PageSsh } from './PageViews'
import { WebAppPhase } from '../../applications/webApp/runtime'
import type { WebAppRuntime } from '../../applications/webApp/runtime'
import type { PortForwardInfo } from '../types'
import { PortForwardStatus } from '../../shared/types'

const forward = (status: PortForwardStatus): PortForwardInfo => (status === PortForwardStatus.Error
  ? { id: 'webapp-tab-1', connectionId: 'c', localPort: 20001, remoteHost: 'localhost', remotePort: 31000, persist: false, status, error: 'bind failed' }
  : { id: 'webapp-tab-1', connectionId: 'c', localPort: 20001, remoteHost: 'localhost', remotePort: 31000, persist: false, status })

function makeSsh(initial: PortForwardInfo[] = []) {
  let statusCb: (info: PortForwardInfo) => void = () => {}
  let outputCb: (line: string) => void = () => {}
  const unsubscribeOutput = vi.fn()
  const ssh = {
    listPortForwards: vi.fn(() => Promise.resolve(initial)),
    onPortForwardStatus: vi.fn((cb: (info: PortForwardInfo) => void) => { statusCb = cb; return vi.fn() }),
    watchPortForwardOutput: vi.fn((_id: string, cb: (line: string) => void) => {
      outputCb = cb
      return Promise.resolve({ scrollback: ['[portfwd] started'], unsubscribe: unsubscribeOutput })
    }),
  } as unknown as PageSsh
  return { ssh, emitStatus: (i: PortForwardInfo) => { statusCb(i) }, emitOutput: (l: string) => { outputCb(l) }, unsubscribeOutput }
}

beforeEach(() => { vi.clearAllMocks() })

describe('BrowserView', () => {
  const noop = (): void => {}

  it.each([
    [{ phase: WebAppPhase.AllocatingPort } as WebAppRuntime, 'Finding a free port…'],
    [{ phase: WebAppPhase.WaitingForServer, port: 3000 } as WebAppRuntime, 'Waiting for server on port 3000…'],
    [{ phase: WebAppPhase.Forwarding, port: 3000 } as WebAppRuntime, 'Starting port forward for port 3000…'],
  ])('shows a waiting message for %o', (runtime, text) => {
    render(<BrowserView runtime={runtime} onRetry={noop} detailsLinks={[{ label: 'View shell', onClick: noop }]} openExternal={noop} />)
    expect(screen.getByText(text)).toBeTruthy()
  })

  it('shows the error with a retry', () => {
    const onRetry = vi.fn()
    render(<BrowserView runtime={{ phase: WebAppPhase.Error, message: 'boom' }} onRetry={onRetry} detailsLinks={[{ label: 'View shell', onClick: noop }]} openExternal={noop} />)
    expect(screen.getByText('boom')).toBeTruthy()
    fireEvent.click(screen.getByText('Retry'))
    expect(onRetry).toHaveBeenCalled()
  })

  type MockWebview = HTMLElement & Record<'goBack' | 'goForward' | 'reload' | 'stop' | 'canGoBack' | 'canGoForward' | 'loadURL', ReturnType<typeof vi.fn>>

  function renderReady(openExternal = vi.fn()) {
    const { container } = render(
      <BrowserView runtime={{ phase: WebAppPhase.Ready, url: 'http://localhost:3000/' }} onRetry={noop} detailsLinks={[{ label: 'View shell', onClick: noop }]} openExternal={openExternal} />,
    )
    const webview = container.querySelector('webview') as MockWebview
    Object.assign(webview, {
      goBack: vi.fn(), goForward: vi.fn(), reload: vi.fn(), stop: vi.fn(),
      canGoBack: vi.fn(() => true), canGoForward: vi.fn(() => false),
      loadURL: vi.fn(() => Promise.resolve()),
    })
    const address = screen.getByLabelText<HTMLInputElement>('Address')
    const navigate = (url: string): void => {
      act(() => { webview.dispatchEvent(Object.assign(new Event('did-navigate'), { url })) })
    }
    return { webview, address, navigate, openExternal }
  }

  it('embeds the page when ready, tracks navigation and drives the webview', () => {
    const { webview, address, navigate, openExternal } = renderReady()
    expect(webview.getAttribute('src')).toBe('http://localhost:3000/')
    expect(address.value).toBe('http://localhost:3000/')
    expect(screen.getByTitle<HTMLButtonElement>('Back').disabled).toBe(true)

    navigate('http://localhost:3000/about')
    expect(address.value).toBe('http://localhost:3000/about')
    expect(screen.getByTitle<HTMLButtonElement>('Back').disabled).toBe(false)
    expect(screen.getByTitle<HTMLButtonElement>('Forward').disabled).toBe(true)

    fireEvent.click(screen.getByTitle('Back'))
    fireEvent.click(screen.getByTitle('Reload'))
    fireEvent.click(screen.getByTitle('Home'))
    expect(webview.goBack).toHaveBeenCalled()
    expect(webview.reload).toHaveBeenCalled()
    expect(webview.loadURL).toHaveBeenCalledWith('http://localhost:3000/')
    fireEvent.click(screen.getByTitle('Open in external browser'))
    expect(openExternal).toHaveBeenCalledWith('http://localhost:3000/about')
  })

  it('swaps reload for stop while loading', () => {
    const { webview } = renderReady()
    act(() => { webview.dispatchEvent(new Event('did-start-loading')) })
    fireEvent.click(screen.getByTitle('Stop'))
    expect(webview.stop).toHaveBeenCalled()
    act(() => { webview.dispatchEvent(new Event('did-stop-loading')) })
    expect(screen.getByTitle('Reload')).toBeTruthy()
  })

  it('navigates to a typed path on Enter', () => {
    const { webview, address } = renderReady()
    fireEvent.change(address, { target: { value: '/docs?x=1' } })
    fireEvent.keyDown(address, { key: 'Enter' })
    expect(webview.loadURL).toHaveBeenCalledWith('http://localhost:3000/docs?x=1')
  })

  it('keeps the typed text while the page navigates, and Escape reverts it', () => {
    const { address, navigate } = renderReady()
    fireEvent.change(address, { target: { value: '/draft' } })
    navigate('http://localhost:3000/elsewhere')
    expect(address.value).toBe('/draft')
    fireEvent.keyDown(address, { key: 'Escape' })
    expect(address.value).toBe('http://localhost:3000/elsewhere')
  })

  it('refuses another port and flags the input', () => {
    const { webview, address } = renderReady()
    fireEvent.change(address, { target: { value: 'localhost:4000/' } })
    fireEvent.keyDown(address, { key: 'Enter' })
    expect(webview.loadURL).not.toHaveBeenCalled()
    expect(address.classList.contains('invalid')).toBe(true)
    expect(address.title).toContain('Only pages on http://localhost:3000')
    fireEvent.change(address, { target: { value: '/ok' } })
    expect(address.classList.contains('invalid')).toBe(false)
  })
})

describe('PortForwardView', () => {
  it('shows status, streams output and offers restart on error', async () => {
    const { ssh, emitStatus, emitOutput } = makeSsh([forward(PortForwardStatus.Active)])
    const onRestart = vi.fn()
    render(<PortForwardView ssh={ssh} connectionId="c" forwardId="webapp-tab-1" onRestart={onRestart} />)
    expect(await screen.findByText(/localhost:20001 → localhost:31000 \(active\)/)).toBeTruthy()
    expect(await screen.findByText('[portfwd] started')).toBeTruthy()
    act(() => { emitOutput('[portfwd] line 2') })
    expect(screen.getByText('[portfwd] line 2')).toBeTruthy()
    expect(screen.queryByText('Restart')).toBeNull()

    act(() => { emitStatus({ ...forward(PortForwardStatus.Active), id: 'someone-else' }) })
    act(() => { emitStatus(forward(PortForwardStatus.Error)) })
    expect(screen.getByText('bind failed')).toBeTruthy()
    fireEvent.click(screen.getByText('Restart'))
    expect(onRestart).toHaveBeenCalled()
  })

  it('re-watches output when a new forward process starts', async () => {
    const { ssh, emitStatus } = makeSsh([forward(PortForwardStatus.Active)])
    render(<PortForwardView ssh={ssh} connectionId="c" forwardId="webapp-tab-1" onRestart={vi.fn()} />)
    await waitFor(() => { expect(ssh.watchPortForwardOutput).toHaveBeenCalledTimes(1) })
    act(() => { emitStatus(forward(PortForwardStatus.Connecting)) })
    await waitFor(() => { expect(ssh.watchPortForwardOutput).toHaveBeenCalledTimes(2) })
  })

  it('unsubscribes a watch that resolves after unmount', async () => {
    const { ssh, unsubscribeOutput } = makeSsh([forward(PortForwardStatus.Active)])
    const { unmount } = render(<PortForwardView ssh={ssh} connectionId="c" forwardId="webapp-tab-1" onRestart={vi.fn()} />)
    await waitFor(() => { expect(ssh.watchPortForwardOutput).toHaveBeenCalled() })
    unmount()
    await waitFor(() => { expect(unsubscribeOutput).toHaveBeenCalled() })
  })

  it('shows list and watch failures', async () => {
    const { ssh } = makeSsh([forward(PortForwardStatus.Active)])
    vi.mocked(ssh.watchPortForwardOutput).mockRejectedValue(new Error('gone'))
    const view = render(<PortForwardView ssh={ssh} connectionId="c" forwardId="webapp-tab-1" onRestart={vi.fn()} />)
    expect(await screen.findByText('Failed to watch output: gone')).toBeTruthy()
    view.unmount()

    vi.mocked(ssh.listPortForwards).mockRejectedValue('offline')
    render(<PortForwardView ssh={ssh} connectionId="c" forwardId="webapp-tab-1" onRestart={vi.fn()} />)
    expect(await screen.findByText('offline')).toBeTruthy()
  })
})
