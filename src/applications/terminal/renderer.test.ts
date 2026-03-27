import { describe, it, expect, vi, beforeEach } from 'vitest'
import {
  createTerminalApplication,
  createTerminalVariant
} from './renderer'
import type { Tab, Workspace, TerminalInstance, TerminalState } from '../../renderer/types'
import { createStore } from 'zustand/vanilla'
import type { WorkspaceStoreState } from '../../renderer/store/createWorkspaceStore'

// Mock React
vi.mock('react', () => ({
  createElement: vi.fn((component: any, props: any) => ({ component, props }))
}))

// Mock Terminal component
vi.mock('../../renderer/components/Terminal', () => ({
  default: vi.fn(() => null)
}))

// Mock activity state store
const mockRemoveTabState = vi.fn()
vi.mock('../../renderer/store/activityState', () => ({
  useActivityStateStore: {
    getState: vi.fn(() => ({
      removeTabState: mockRemoveTabState
    }))
  }
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
  markAllReviewCommentsAddressed: vi.fn(),
  promptHarness: vi.fn(),
  quickForkWorkspace: vi.fn(),
  updateMetadata: vi.fn(),
  updateStatus: vi.fn(),
  refreshGitInfo: vi.fn(),
  mergeAndRemove: vi.fn(),
  mergeAndKeep: vi.fn(),
  closeAndClean: vi.fn(),
  lookupWorkspace: vi.fn(),
  remove: vi.fn(),
  removeKeepBranch: vi.fn(),
  removeKeepBoth: vi.fn(),
  getGitApi: vi.fn(),
  getFilesystemApi: vi.fn(),
  initTab: vi.fn(),
  getTabRef: vi.fn().mockReturnValue(null),
  initAnalyzer: vi.fn(),
  createTty: vi.fn().mockResolvedValue('pty-1'),
  connectionId: 'local',
  updateSettings: vi.fn(),
  hasUncommittedChanges: false,
  isDiffCleanFromParent: false,
  hasConflictsWithParent: false,
  disposeGitController: vi.fn(),
  focusTabId: null,
  requestFocus: vi.fn(),
  clearFocusRequest: vi.fn(),
  behindCount: 0,
  pullLoading: false,
  refreshRemoteStatus: vi.fn(),
  pullFromRemote: vi.fn(),
  refreshDiffStatus: vi.fn(),
  gitRefreshing: false,
  hasPr: false,
  refreshPrStatus: vi.fn(),
  openGitHub: vi.fn(),
} as WorkspaceStoreState

const mockWorkspaceStore = createStore<WorkspaceStoreState>()(() => mockWorkspaceStoreStateData)

describe('Terminal Renderer', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  describe('createTerminalApplication', () => {
    it('creates terminal application with correct properties', () => {
      const app = createTerminalApplication(mockDeps)

      expect(app.id).toBe('terminal')
      expect(app.name).toBe('Terminal')
      expect(app.icon).toBe('>')
      expect(app.canClose).toBe(true)
      expect(app.showInNewTabMenu).toBe(true)
      expect(app.displayStyle).toBe('flex')
    })

    it('has isDefault set to true', () => {
      const app = createTerminalApplication(mockDeps)
      expect(app.isDefault).toBe(true)
    })

    describe('createInitialState', () => {
      it('returns terminal state with null ptyId', () => {
        const app = createTerminalApplication(mockDeps)
        const state = app.createInitialState()

        expect(state).toEqual({ ptyId: null, ptyHandle: null })
      })
    })

    describe('onWorkspaceLoad and dispose', () => {
      it('dispose kills PTY when tab has ptyId and removes activity state', () => {
        const app = createTerminalApplication(mockDeps)
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'terminal',
          title: 'Terminal',
          state: { ptyId: 'pty-123' }
        }

        const ref = app.onWorkspaceLoad(tab, mockWorkspaceStore)
        ref.dispose()

        expect(mockTerminalKill).toHaveBeenCalledWith('local', 'pty-123')
        expect(mockRemoveTabState).toHaveBeenCalledWith('tab-1')
      })

      it('dispose does not kill PTY when tab has no ptyId but removes activity state', () => {
        const app = createTerminalApplication(mockDeps)
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'terminal',
          title: 'Terminal',
          state: { ptyId: null }
        }

        const ref = app.onWorkspaceLoad(tab, mockWorkspaceStore)
        ref.dispose()

        expect(mockTerminalKill).not.toHaveBeenCalled()
        expect(mockRemoveTabState).toHaveBeenCalledWith('tab-1')
      })

      it('returns an AppRef with dispose method', () => {
        const app = createTerminalApplication(mockDeps)
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'terminal',
          title: 'Terminal',
          state: { ptyId: null }
        }

        const ref = app.onWorkspaceLoad(tab, mockWorkspaceStore)

        expect(typeof ref.dispose).toBe('function')
      })
    })

    describe('render', () => {
      it('renders Terminal component with correct props', () => {
        const app = createTerminalApplication(mockDeps)
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'terminal',
          title: 'Terminal',
          state: { ptyId: null }
        }

        const result = app.render({
          tab,
          workspace: mockWorkspaceStore,
          isVisible: true,
        })

        expect(result).toEqual(expect.objectContaining({
          component: expect.any(Function),
          props: expect.objectContaining({
            key: 'tab-1',
            cwd: '/test',
            workspace: mockWorkspaceStore,
            tabId: 'tab-1',
            isVisible: true
          })
        }))
      })
    })
  })

  describe('terminalApplication (via createTerminalApplication)', () => {
    it('is a terminal application with isDefault set to true', () => {
      const app = createTerminalApplication(mockDeps)
      expect(app.id).toBe('terminal')
      expect(app.name).toBe('Terminal')
      expect(app.isDefault).toBe(true)
    })

    it('can create initial state', () => {
      const app = createTerminalApplication(mockDeps)
      const state = app.createInitialState()
      expect(state).toEqual({ ptyId: null, ptyHandle: null })
    })

    it('can render Terminal component', () => {
      const app = createTerminalApplication(mockDeps)
      const tab: Tab = {
        id: 'tab-1',
        applicationId: 'terminal',
        title: 'Terminal',
        state: { ptyId: null }
      }

      const result = app.render({
        tab,
        workspace: mockWorkspaceStore,
        isVisible: true,
      })

      expect(result).toBeDefined()
      expect(result).toHaveProperty('component')
      expect(result).toHaveProperty('props')
    })

    it('onWorkspaceLoad returns ref that disposes correctly', () => {
      const app = createTerminalApplication(mockDeps)
      const tab: Tab = {
        id: 'tab-1',
        applicationId: 'terminal',
        title: 'Terminal',
        state: { ptyId: 'pty-123' }
      }

      const ref = app.onWorkspaceLoad(tab, mockWorkspaceStore)
      ref.dispose()

      expect(mockTerminalKill).toHaveBeenCalledWith('local', 'pty-123')
      expect(mockRemoveTabState).toHaveBeenCalledWith('tab-1')
    })
  })

  describe('createTerminalVariant', () => {
    const mockInstance: TerminalInstance = {
      id: 'custom-term',
      name: 'Custom Terminal',
      icon: '\u{1F680}',
      startupCommand: 'echo "Hello"',
      isDefault: false
    }

    it('creates variant with custom id and name', () => {
      const variant = createTerminalVariant(mockInstance, mockDeps)

      expect(variant.id).toBe('terminal-custom-term')
      expect(variant.name).toBe('Custom Terminal')
      expect(variant.icon).toBe('\u{1F680}')
    })

    it('preserves other application properties', () => {
      const variant = createTerminalVariant(mockInstance, mockDeps)

      expect(variant.canClose).toBe(true)
      expect(variant.showInNewTabMenu).toBe(true)
      expect(variant.displayStyle).toBe('flex')
    })

    it('sets isDefault from instance', () => {
      const variant = createTerminalVariant(mockInstance, mockDeps)
      expect(variant.isDefault).toBe(false)

      const defaultVariant = createTerminalVariant({
        ...mockInstance,
        isDefault: true
      }, mockDeps)
      expect(defaultVariant.isDefault).toBe(true)
    })

    describe('createInitialState', () => {
      it('returns terminal state with null ptyId', () => {
        const variant = createTerminalVariant(mockInstance, mockDeps)
        const state = variant.createInitialState()

        expect(state).toEqual({ ptyId: null, ptyHandle: null })
      })
    })

    describe('onWorkspaceLoad and dispose', () => {
      it('dispose kills PTY when tab has ptyId', () => {
        const variant = createTerminalVariant(mockInstance, mockDeps)
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'terminal-custom-term',
          title: 'Custom Terminal',
          state: { ptyId: 'pty-456' }
        }

        const ref = variant.onWorkspaceLoad(tab, mockWorkspaceStore)
        ref.dispose()

        expect(mockTerminalKill).toHaveBeenCalledWith('local', 'pty-456')
        expect(mockRemoveTabState).toHaveBeenCalledWith('tab-1')
      })

      it('dispose does not kill PTY when state has no ptyId', () => {
        const variant = createTerminalVariant(mockInstance, mockDeps)
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'terminal-custom-term',
          title: 'Custom Terminal',
          state: { ptyId: null }
        }

        const ref = variant.onWorkspaceLoad(tab, mockWorkspaceStore)
        ref.dispose()

        expect(mockTerminalKill).not.toHaveBeenCalled()
        expect(mockRemoveTabState).toHaveBeenCalledWith('tab-1')
      })
    })

    describe('render', () => {
      it('renders Terminal component with startup command', () => {
        const variant = createTerminalVariant(mockInstance, mockDeps)
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'terminal-custom-term',
          title: 'Custom Terminal',
          state: { ptyId: null }
        }

        const result = variant.render({
          tab,
          workspace: mockWorkspaceStore,
          isVisible: true,
        })

        expect(result).toEqual(expect.objectContaining({
          component: expect.any(Function),
          props: expect.objectContaining({
            key: 'tab-1',
            cwd: '/test',
            workspace: mockWorkspaceStore,
            tabId: 'tab-1',
            isVisible: true,
            startupCommand: 'echo "Hello"'
          })
        }))
      })

      it('passes empty startupCommand when not set', () => {
        const variantWithoutCommand = createTerminalVariant({
          ...mockInstance,
          startupCommand: ''
        }, mockDeps)
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'terminal-custom-term',
          title: 'Custom Terminal',
          state: { ptyId: null }
        }

        const result = variantWithoutCommand.render({
          tab,
          workspace: mockWorkspaceStore,
          isVisible: true,
        }) as { props: { startupCommand: string } }

        expect(result.props.startupCommand).toBe('')
      })
    })
  })
})
