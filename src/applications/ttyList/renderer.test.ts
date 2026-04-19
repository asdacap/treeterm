import { describe, it, expect, vi, beforeEach } from 'vitest'
import { ttyListApplication } from './renderer'
import type { Tab, Workspace } from '../../renderer/types'
import { createStore } from 'zustand/vanilla'
import type { WorkspaceStoreState } from '../../renderer/store/createWorkspaceStore'

vi.mock('react', async (importOriginal) => {
  const actual = await importOriginal<typeof import('react')>()
  return {
    ...actual,
    createElement: vi.fn((component: unknown, props: unknown) => ({ component, props }))
  }
})

vi.mock('../../renderer/components/TtyListBrowser', () => ({
  default: vi.fn(() => null)
}))

vi.mock('../../renderer/store/app', () => ({
  useAppStore: vi.fn(() => ({})),
}))

const mockWorkspaceStore = createStore<WorkspaceStoreState>()(() => ({
  workspace: { id: 'ws-1', path: '/test' } as Workspace,
  connectionId: 'local',
  appStates: {},
  metadata: {},
} as unknown as WorkspaceStoreState))

describe('TtyList Renderer', () => {
  beforeEach(() => { vi.clearAllMocks() })

  it('has correct application properties', () => {
    expect(ttyListApplication.id).toBe('tty-list')
    expect(ttyListApplication.name).toBe('TTYs')
    expect(ttyListApplication.canClose).toBe(true)
    expect(ttyListApplication.showInNewTabMenu).toBe(true)
    expect(ttyListApplication.displayStyle).toBe('flex')
    expect(ttyListApplication.isDefault).toBe(false)
  })

  it('creates empty initial state', () => {
    expect(ttyListApplication.createInitialState()).toEqual({})
  })

  it('returns a fresh state object on each call', () => {
    const a = ttyListApplication.createInitialState()
    const b = ttyListApplication.createInitialState()
    expect(a).toEqual(b)
    expect(a).not.toBe(b)
  })

  it('onWorkspaceLoad returns a disposable ref with no-op close+dispose', () => {
    const tab = { id: 'tab-1', state: {} } as unknown as Tab
    const ref = ttyListApplication.onWorkspaceLoad(tab, mockWorkspaceStore)
    expect(typeof ref.dispose).toBe('function')
    expect(typeof ref.close).toBe('function')
    ref.close()
    ref.dispose()
  })

  it('renders TtyListBrowser via createElement', () => {
    const tab = { id: 'tab-1', state: {} } as unknown as Tab
    const result = ttyListApplication.render({ tab, workspace: mockWorkspaceStore, isVisible: true })
    expect(result).toEqual({
      // eslint-disable-next-line @typescript-eslint/no-unsafe-assignment
      component: expect.any(Function),
      // eslint-disable-next-line @typescript-eslint/no-unsafe-assignment
      props: expect.objectContaining({ tab, workspace: mockWorkspaceStore, isVisible: true }),
    })
  })
})
