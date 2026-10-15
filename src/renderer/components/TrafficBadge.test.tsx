// @vitest-environment jsdom
import { describe, it, expect } from 'vitest'
import { render, screen, act } from '@testing-library/react'
import { createStore } from 'zustand/vanilla'
import type { StoreApi } from 'zustand'
import { WorkspaceTrafficBadge, ConnectionTrafficBadge } from './TrafficBadge'
import { TrafficStatus, type TrafficMeterState } from '../store/createTrafficMeterStore'

function makeMeter(partial: Partial<TrafficMeterState>): StoreApi<TrafficMeterState> {
  return createStore<TrafficMeterState>()(() => ({
    workspaces: {},
    connections: {},
    recordWorkspace: () => {},
    removeWorkspace: () => {},
    dispose: () => {},
    ...partial,
  }))
}

describe('WorkspaceTrafficBadge', () => {
  it('shows zeros before the workspace has downloaded anything', () => {
    render(<WorkspaceTrafficBadge meter={makeMeter({})} workspaceId="ws-1" />)
    expect(screen.getByTestId('workspace-traffic').textContent).toBe('↓ 0 B/s')
    expect(screen.getByTestId('workspace-traffic').getAttribute('title')).toBe('0 B')
  })

  it('shows the rate, the total on hover, and follows updates', () => {
    const meter = makeMeter({ workspaces: { 'ws-1': { totalBytes: 2048, bytesPerSec: 1024 * 1024 } } })
    render(<WorkspaceTrafficBadge meter={meter} workspaceId="ws-1" />)
    expect(screen.getByTestId('workspace-traffic').textContent).toBe('↓ 1.0 MB/s')
    expect(screen.getByTestId('workspace-traffic').getAttribute('title')).toBe('2.0 KB')

    act(() => { meter.setState({ workspaces: { 'ws-1': { totalBytes: 4096, bytesPerSec: 0 } } }) })
    expect(screen.getByTestId('workspace-traffic').textContent).toBe('↓ 0 B/s')
    expect(screen.getByTestId('workspace-traffic').getAttribute('title')).toBe('4.0 KB')
  })
})

describe('ConnectionTrafficBadge', () => {
  it('shows loading before the first sample', () => {
    render(<ConnectionTrafficBadge meter={makeMeter({})} connectionId="conn-1" />)
    expect(screen.getByTestId('connection-traffic').textContent).toBe('↓ …')
  })

  it('shows the rate, with the total on hover, once ready', () => {
    const meter = makeMeter({ connections: { 'conn-1': { status: TrafficStatus.Ready, rate: { totalBytes: 512, bytesPerSec: 100.4 } } } })
    render(<ConnectionTrafficBadge meter={meter} connectionId="conn-1" />)
    expect(screen.getByTestId('connection-traffic').textContent).toBe('↓ 100 B/s')
    expect(screen.getByTestId('connection-traffic').getAttribute('title')).toBe('512 B')
  })

  it('shows the error in the tooltip', () => {
    const meter = makeMeter({ connections: { 'conn-1': { status: TrafficStatus.Error, message: 'Connection not found' } } })
    render(<ConnectionTrafficBadge meter={meter} connectionId="conn-1" />)
    const badge = screen.getByTestId('connection-traffic')
    expect(badge.textContent).toContain('↓ ?')
    expect(badge.getAttribute('title')).toBe('Download meter unavailable: Connection not found')
  })
})
