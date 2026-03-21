import { describe, it, expect, vi, beforeEach } from 'vitest'
import { createAiHarnessVariant } from './renderer'
import type { Tab, Workspace, AiHarnessInstance, AiHarnessState } from '../../renderer/types'
import { createStore } from 'zustand/vanilla'
import type { WorkspaceStoreState } from '../../renderer/store/createWorkspaceStore'

// Mock React
vi.mock('react', () => ({
  createElement: vi.fn((component: any, props: any) => ({ component, props }))
}))

// Mock AiHarness component
vi.mock('../../renderer/components/AiHarness', () => ({
  default: vi.fn(() => null)
}))

const mockTerminalKill = vi.fn()
const mockDeps = { terminal: { kill: mockTerminalKill } }

const mockWorkspaceStoreStateData = {
  workspace: { id: 'ws-1', path: '/test' } as Workspace,
  addTab: vi.fn(),
  removeTab: vi.fn(),
  setActiveTab: vi.fn(),
  updateTabTitle: vi.fn(),
  updateTabState: vi.fn(),
  getReviewComments: vi.fn(),
  addReviewComment: vi.fn(),
  deleteReviewComment: vi.fn(),
  toggleReviewCommentAddressed: vi.fn(),
  updateOutdatedReviewComments: vi.fn(),
  clearReviewComments: vi.fn(),
  promptHarness: vi.fn(),
  quickForkWorkspace: vi.fn(),
  updateMetadata: vi.fn(),
  updateStatus: vi.fn(),
  refreshGitInfo: vi.fn(),
  mergeAndRemove: vi.fn(),
  closeAndClean: vi.fn(),
  lookupWorkspace: vi.fn(),
  remove: vi.fn(),
  removeKeepBranch: vi.fn(),
  removeKeepWorktree: vi.fn(),
  removeKeepBoth: vi.fn(),
  getGitApi: vi.fn(),
  getFilesystemApi: vi.fn(),
} as WorkspaceStoreState

const mockWorkspaceStore = createStore<WorkspaceStoreState>()(() => mockWorkspaceStoreStateData)

describe('AI Harness Renderer', () => {
  const mockInstance: AiHarnessInstance = {
    id: 'claude',
    name: 'Claude',
    icon: '\u2726',
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
      const app = createAiHarnessVariant(mockInstance, mockDeps)

      expect(app.id).toBe('aiharness-claude')
      expect(app.name).toBe('Claude')
      expect(app.icon).toBe('\u2726')
      expect(app.canClose).toBe(true)
      expect(app.canHaveMultiple).toBe(true)
      expect(app.showInNewTabMenu).toBe(true)
      expect(app.keepAlive).toBe(true)
      expect(app.displayStyle).toBe('flex')
    })

    it('sets isDefault from instance', () => {
      const app = createAiHarnessVariant(mockInstance, mockDeps)
      expect(app.isDefault).toBe(false)

      const defaultApp = createAiHarnessVariant({
        ...mockInstance,
        isDefault: true
      }, mockDeps)
      expect(defaultApp.isDefault).toBe(true)
    })

    describe('createInitialState', () => {
      it('returns AI Harness state with sandbox configuration and analyzer defaults', () => {
        const app = createAiHarnessVariant(mockInstance, mockDeps)
        const state = app.createInitialState()

        expect(state).toEqual({
          ptyId: null,
          ptyHandle: null,
          sandbox: {
            enabled: true,
            allowNetwork: false,
            allowedPaths: []
          },
          aiState: 'idle',
          analyzing: false,
          reason: ''
        })
      })

      it('uses instance sandbox settings', () => {
        const appWithoutSandbox = createAiHarnessVariant({
          ...mockInstance,
          enableSandbox: false,
          allowNetwork: true
        }, mockDeps)
        const state = appWithoutSandbox.createInitialState()

        expect(state.sandbox.enabled).toBe(false)
        expect(state.sandbox.allowNetwork).toBe(true)
      })

      it('always initializes allowedPaths as empty array', () => {
        const app = createAiHarnessVariant(mockInstance, mockDeps)
        const state = app.createInitialState()

        expect(state.sandbox.allowedPaths).toEqual([])
      })
    })

    describe('cleanup', () => {
      it('kills PTY when tab has ptyId', async () => {
        const app = createAiHarnessVariant(mockInstance, mockDeps)
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
          appStates: {},
          activeTabId: null,
          metadata: {},
          createdAt: Date.now(),
          lastActivity: Date.now()
        }

        await app.cleanup?.(tab, workspace)

        expect(mockTerminalKill).toHaveBeenCalledWith('pty-789')
      })

      it('does not kill PTY when tab has no ptyId', async () => {
        const app = createAiHarnessVariant(mockInstance, mockDeps)
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
          appStates: {},
          activeTabId: null,
          metadata: {},
          createdAt: Date.now(),
          lastActivity: Date.now()
        }

        await app.cleanup?.(tab, workspace)

        expect(mockTerminalKill).not.toHaveBeenCalled()
      })

      it('does not kill PTY when state is not AI Harness state', async () => {
        const app = createAiHarnessVariant(mockInstance, mockDeps)
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
          appStates: {},
          activeTabId: null,
          metadata: {},
          createdAt: Date.now(),
          lastActivity: Date.now()
        }

        await app.cleanup?.(tab, workspace)

        expect(mockTerminalKill).not.toHaveBeenCalled()
      })
    })

    describe('render', () => {
      it('renders AiHarness component with correct props', () => {
        const app = createAiHarnessVariant(mockInstance, mockDeps)
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
          workspace: mockWorkspaceStore,
          isVisible: true,
        })

        expect(result).toEqual({
          component: expect.any(Function),
          props: expect.objectContaining({
            key: 'tab-1',
            cwd: '/test',
            workspace: mockWorkspaceStore,
            tabId: 'tab-1',
            sandbox: { enabled: true, allowNetwork: false, allowedPaths: [] },
            isVisible: true,
            command: 'claude',
            backgroundColor: '#1a1a24',
            disableScrollbar: true
          })
        })
      })

      it('returns null for invalid state', () => {
        const app = createAiHarnessVariant(mockInstance, mockDeps)
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'aiharness-claude',
          title: 'Claude',
          state: { invalid: true } // Not a valid AiHarnessState
        }

        const result = app.render({
          tab,
          workspace: mockWorkspaceStore,
          isVisible: true,
        })

        expect(result).toBeNull()
      })

      it('passes instance properties to component', () => {
        const app = createAiHarnessVariant(mockInstance, mockDeps)
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
          workspace: mockWorkspaceStore,
          isVisible: false,
        }) as { props: Record<string, unknown> }

        expect(result.props.command).toBe('claude')
        expect(result.props.backgroundColor).toBe('#1a1a24')
        expect(result.props.disableScrollbar).toBe(true)
      })

      it('passes false for disableScrollbar when undefined', () => {
        const appWithoutScrollbarOption = createAiHarnessVariant({
          ...mockInstance,
          disableScrollbar: undefined
        }, mockDeps)
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
          workspace: mockWorkspaceStore,
          isVisible: true,
        }) as { props: Record<string, unknown> }

        expect(result.props.disableScrollbar).toBeUndefined()
      })
    })

    describe('getActivityState', () => {
      it('returns aiState directly as ActivityState', () => {
        const app = createAiHarnessVariant(mockInstance, mockDeps)
        const states = ['idle', 'working', 'user_input_required', 'permission_request', 'safe_permission_requested', 'completed', 'error'] as const

        for (const aiState of states) {
          const tab: Tab = {
            id: 'tab-1',
            applicationId: 'aiharness-claude',
            title: 'Claude',
            state: {
              ptyId: null,
              sandbox: { enabled: true, allowNetwork: false, allowedPaths: [] },
              aiState,
              analyzing: false,
              reason: ''
            }
          }

          expect(app.getActivityState?.(tab)).toBe(aiState)
        }
      })

      it('returns idle when state is not AiHarnessState', () => {
        const app = createAiHarnessVariant(mockInstance, mockDeps)
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'aiharness-claude',
          title: 'Claude',
          state: { ptyId: 'pty-789' } // Not a valid AiHarnessState
        }

        const activityState = app.getActivityState?.(tab)

        expect(activityState).toBe('idle')
      })
    })
  })
})
