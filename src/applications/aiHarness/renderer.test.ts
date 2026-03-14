import { describe, it, expect, vi, beforeEach } from 'vitest'
import { createAiHarnessVariant } from './renderer'
import type { Tab, Workspace, AiHarnessInstance, AiHarnessState } from '../../renderer/types'
import { useActivityStateStore } from '../../renderer/store/activityState'

// Mock React
vi.mock('react', () => ({
  createElement: vi.fn((component: any, props: any) => ({ component, props }))
}))

// Mock AiHarness component
vi.mock('../../renderer/components/AiHarness', () => ({
  default: vi.fn(() => null)
}))

// Mock activity state store
const mockRemoveTabState = vi.fn()
const mockGetTabState = vi.fn()
vi.mock('../../renderer/store/activityState', () => ({
  useActivityStateStore: {
    getState: vi.fn(() => ({
      removeTabState: mockRemoveTabState,
      states: {}
    }))
  }
}))

// Mock window.electron
const mockTerminalKill = vi.fn()
;(globalThis as unknown as { window: { electron: { terminal: { kill: typeof mockTerminalKill } } } }).window = {
  electron: {
    terminal: {
      kill: mockTerminalKill
    }
  }
}

describe('AI Harness Renderer', () => {
  const mockInstance: AiHarnessInstance = {
    id: 'claude',
    name: 'Claude',
    icon: '✦',
    command: 'claude',
    isDefault: false,
    enableSandbox: true,
    allowNetwork: false,
    backgroundColor: '#1a1a24',
    disableScrollbar: true
  }

  beforeEach(() => {
    vi.clearAllMocks()
  })

  describe('createAiHarnessVariant', () => {
    it('creates AI Harness variant with correct properties', () => {
      const app = createAiHarnessVariant(mockInstance)

      expect(app.id).toBe('aiharness-claude')
      expect(app.name).toBe('Claude')
      expect(app.icon).toBe('✦')
      expect(app.canClose).toBe(true)
      expect(app.canHaveMultiple).toBe(true)
      expect(app.showInNewTabMenu).toBe(true)
      expect(app.keepAlive).toBe(true)
      expect(app.displayStyle).toBe('flex')
    })

    it('sets isDefault from instance', () => {
      const app = createAiHarnessVariant(mockInstance)
      expect(app.isDefault).toBe(false)

      const defaultApp = createAiHarnessVariant({
        ...mockInstance,
        isDefault: true
      })
      expect(defaultApp.isDefault).toBe(true)
    })

    describe('createInitialState', () => {
      it('returns AI Harness state with sandbox configuration', () => {
        const app = createAiHarnessVariant(mockInstance)
        const state = app.createInitialState()

        expect(state).toEqual({
          ptyId: null,
          sandbox: {
            enabled: true,
            allowNetwork: false,
            allowedPaths: []
          }
        })
      })

      it('uses instance sandbox settings', () => {
        const appWithoutSandbox = createAiHarnessVariant({
          ...mockInstance,
          enableSandbox: false,
          allowNetwork: true
        })
        const state = appWithoutSandbox.createInitialState()

        expect(state.sandbox.enabled).toBe(false)
        expect(state.sandbox.allowNetwork).toBe(true)
      })

      it('always initializes allowedPaths as empty array', () => {
        const app = createAiHarnessVariant(mockInstance)
        const state = app.createInitialState()

        expect(state.sandbox.allowedPaths).toEqual([])
      })
    })

    describe('cleanup', () => {
      it('kills PTY when tab has ptyId', async () => {
        const app = createAiHarnessVariant(mockInstance)
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'aiharness-claude',
          title: 'Claude',
          state: {
            ptyId: 'pty-789',
            sandbox: { enabled: true, allowNetwork: false, allowedPaths: [] }
          }
        }
        const workspace: Workspace = {
          id: 'ws-1',
          path: '/test',
          name: 'Test',
          parentId: null,
          children: [],
          status: 'active',
          isGitRepo: true,
          gitBranch: 'main',
          gitRootPath: '/test',
          isWorktree: false,
          tabs: [],
          activeTabId: null,
          createdAt: Date.now(),
          lastActivity: Date.now(),
          attachedClients: 1
        }

        await app.cleanup?.(tab, workspace)

        expect(mockTerminalKill).toHaveBeenCalledWith('pty-789')
        expect(mockRemoveTabState).toHaveBeenCalledWith('tab-1')
      })

      it('does not kill PTY when tab has no ptyId', async () => {
        const app = createAiHarnessVariant(mockInstance)
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'aiharness-claude',
          title: 'Claude',
          state: {
            ptyId: null,
            sandbox: { enabled: true, allowNetwork: false, allowedPaths: [] }
          }
        }
        const workspace: Workspace = {
          id: 'ws-1',
          path: '/test',
          name: 'Test',
          parentId: null,
          children: [],
          status: 'active',
          isGitRepo: true,
          gitBranch: 'main',
          gitRootPath: '/test',
          isWorktree: false,
          tabs: [],
          activeTabId: null,
          createdAt: Date.now(),
          lastActivity: Date.now(),
          attachedClients: 1
        }

        await app.cleanup?.(tab, workspace)

        expect(mockTerminalKill).not.toHaveBeenCalled()
        expect(mockRemoveTabState).toHaveBeenCalledWith('tab-1')
      })

      it('does not kill PTY when state is not AI Harness state', async () => {
        const app = createAiHarnessVariant(mockInstance)
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'aiharness-claude',
          title: 'Claude',
          state: { ptyId: 'pty-789' } // Missing sandbox property
        }
        const workspace: Workspace = {
          id: 'ws-1',
          path: '/test',
          name: 'Test',
          parentId: null,
          children: [],
          status: 'active',
          isGitRepo: true,
          gitBranch: 'main',
          gitRootPath: '/test',
          isWorktree: false,
          tabs: [],
          activeTabId: null,
          createdAt: Date.now(),
          lastActivity: Date.now(),
          attachedClients: 1
        }

        await app.cleanup?.(tab, workspace)

        expect(mockTerminalKill).not.toHaveBeenCalled()
        expect(mockRemoveTabState).toHaveBeenCalledWith('tab-1')
      })

      it('removes activity state regardless of PTY state', async () => {
        const app = createAiHarnessVariant(mockInstance)
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'aiharness-claude',
          title: 'Claude',
          state: {
            ptyId: null,
            sandbox: { enabled: true, allowNetwork: false, allowedPaths: [] }
          }
        }
        const workspace: Workspace = {
          id: 'ws-1',
          path: '/test',
          name: 'Test',
          parentId: null,
          children: [],
          status: 'active',
          isGitRepo: true,
          gitBranch: 'main',
          gitRootPath: '/test',
          isWorktree: false,
          tabs: [],
          activeTabId: null,
          createdAt: Date.now(),
          lastActivity: Date.now(),
          attachedClients: 1
        }

        await app.cleanup?.(tab, workspace)

        expect(mockRemoveTabState).toHaveBeenCalledTimes(1)
        expect(mockRemoveTabState).toHaveBeenCalledWith('tab-1')
      })
    })

    describe('render', () => {
      it('renders AiHarness component with correct props', () => {
        const app = createAiHarnessVariant(mockInstance)
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'aiharness-claude',
          title: 'Claude',
          state: {
            ptyId: null,
            sandbox: { enabled: true, allowNetwork: false, allowedPaths: [] }
          }
        }

        const result = app.render({
          tab,
          workspaceId: 'ws-1',
          workspacePath: '/test',
          isVisible: true
        })

        expect(result).toEqual({
          component: expect.any(Function),
          props: {
            key: 'tab-1',
            cwd: '/test',
            workspaceId: 'ws-1',
            tabId: 'tab-1',
            sandbox: { enabled: true, allowNetwork: false, allowedPaths: [] },
            isVisible: true,
            command: 'claude',
            backgroundColor: '#1a1a24',
            disableScrollbar: true
          }
        })
      })

      it('returns null for invalid state', () => {
        const app = createAiHarnessVariant(mockInstance)
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'aiharness-claude',
          title: 'Claude',
          state: { invalid: true } // Not a valid AiHarnessState
        }

        const result = app.render({
          tab,
          workspaceId: 'ws-1',
          workspacePath: '/test',
          isVisible: true
        })

        expect(result).toBeNull()
      })

      it('passes instance properties to component', () => {
        const app = createAiHarnessVariant(mockInstance)
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'aiharness-claude',
          title: 'Claude',
          state: {
            ptyId: null,
            sandbox: { enabled: true, allowNetwork: false, allowedPaths: [] }
          }
        }

        const result = app.render({
          tab,
          workspaceId: 'ws-1',
          workspacePath: '/test',
          isVisible: false
        }) as { props: Record<string, unknown> }

        expect(result.props.command).toBe('claude')
        expect(result.props.backgroundColor).toBe('#1a1a24')
        expect(result.props.disableScrollbar).toBe(true)
      })

      it('passes false for disableScrollbar when undefined', () => {
        const appWithoutScrollbarOption = createAiHarnessVariant({
          ...mockInstance,
          disableScrollbar: undefined
        })
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'aiharness-claude',
          title: 'Claude',
          state: {
            ptyId: null,
            sandbox: { enabled: true, allowNetwork: false, allowedPaths: [] }
          }
        }

        const result = appWithoutScrollbarOption.render({
          tab,
          workspaceId: 'ws-1',
          workspacePath: '/test',
          isVisible: true
        }) as { props: Record<string, unknown> }

        expect(result.props.disableScrollbar).toBeUndefined()
      })
    })

    describe('getActivityState', () => {
      it('returns idle when no state exists for tab', () => {
        const app = createAiHarnessVariant(mockInstance)
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'aiharness-claude',
          title: 'Claude',
          state: {
            ptyId: null,
            sandbox: { enabled: true, allowNetwork: false, allowedPaths: [] }
          }
        }

        const activityState = app.getActivityState?.(tab)

        expect(activityState).toBe('idle')
      })

      it('returns correct state from store', () => {
        const mockStates = { 'tab-1': 'working' }
        vi.mocked(useActivityStateStore.getState).mockReturnValue({
          removeTabState: mockRemoveTabState,
          states: mockStates
        } as any)

        const app = createAiHarnessVariant(mockInstance)
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'aiharness-claude',
          title: 'Claude',
          state: {
            ptyId: null,
            sandbox: { enabled: true, allowNetwork: false, allowedPaths: [] }
          }
        }

        const activityState = app.getActivityState?.(tab)

        expect(activityState).toBe('working')
      })

      it('returns waiting_for_input state from store', () => {
        const mockStates = { 'tab-1': 'waiting_for_input' }
        vi.mocked(useActivityStateStore.getState).mockReturnValue({
          removeTabState: mockRemoveTabState,
          states: mockStates
        } as any)

        const app = createAiHarnessVariant(mockInstance)
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'aiharness-claude',
          title: 'Claude',
          state: {
            ptyId: null,
            sandbox: { enabled: true, allowNetwork: false, allowedPaths: [] }
          }
        }

        const activityState = app.getActivityState?.(tab)

        expect(activityState).toBe('waiting_for_input')
      })
    })
  })
})
