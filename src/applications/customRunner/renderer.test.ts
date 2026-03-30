import { describe, it, expect, vi, beforeEach } from 'vitest'
import { createCustomRunnerVariant, resolveTemplate } from './renderer'
import type { Tab, Workspace, CustomRunnerInstance } from '../../renderer/types'
import { createStore } from 'zustand/vanilla'
import type { WorkspaceStoreState } from '../../renderer/store/createWorkspaceStore'
import type { GitControllerState } from '../../renderer/store/createGitControllerStore'
import type { ReviewCommentState } from '../../renderer/store/createReviewCommentStore'

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
  workspace: { id: 'ws-1', path: '/test/project' } as Workspace,
  addTab: vi.fn(),
  removeTab: vi.fn(),
  setActiveTab: vi.fn(),
  updateTabTitle: vi.fn(),
  updateTabState: vi.fn(),
  reviewComments: createStore<ReviewCommentState>()(() => ({
    getReviewComments: vi.fn().mockReturnValue([]),
    addReviewComment: vi.fn(),
    deleteReviewComment: vi.fn(),
    toggleReviewCommentAddressed: vi.fn(),
    updateOutdatedReviewComments: vi.fn(),
    clearReviewComments: vi.fn(),
    markAllReviewCommentsAddressed: vi.fn(),
  } as ReviewCommentState)),
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
  getRunActionsApi: vi.fn(),
  initTab: vi.fn(),
  getTabRef: vi.fn().mockReturnValue(null),
  initAnalyzer: vi.fn(),
  createTty: vi.fn().mockResolvedValue('pty-1'),
  connectionId: 'local',
  updateSettings: vi.fn(),
  focusTabId: null,
  requestFocus: vi.fn(),
  clearFocusRequest: vi.fn(),
  gitController: createStore<GitControllerState>()(() => ({
    hasUncommittedChanges: false,
    isDiffCleanFromParent: false,
    hasConflictsWithParent: false,
    behindCount: 0,
    pullLoading: false,
    gitRefreshing: false,
    prInfo: null,
    refreshDiffStatus: vi.fn(),
    refreshRemoteStatus: vi.fn(),
    pullFromRemote: vi.fn(),
    refreshPrStatus: vi.fn(),
    openGitHub: vi.fn(),
    startPolling: vi.fn(),
    dispose: vi.fn(),
  } as GitControllerState)),
} as WorkspaceStoreState

const mockWorkspaceStore = createStore<WorkspaceStoreState>()(() => mockWorkspaceStoreStateData)

const mockInstance: CustomRunnerInstance = {
  id: 'rider',
  name: 'Rider',
  icon: '🚀',
  commandTemplate: 'rider {{workspace_path}}',
  isDefault: false
}

describe('resolveTemplate', () => {
  it('replaces {{workspace_path}} with the given path', () => {
    expect(resolveTemplate('rider {{workspace_path}}', '/home/user/project')).toBe('rider /home/user/project')
  })

  it('replaces multiple occurrences of {{workspace_path}}', () => {
    expect(resolveTemplate('open {{workspace_path}} && cd {{workspace_path}}', '/foo')).toBe('open /foo && cd /foo')
  })

  it('passes through template unchanged when no placeholder present', () => {
    expect(resolveTemplate('echo hello', '/foo')).toBe('echo hello')
  })

  it('handles empty template', () => {
    expect(resolveTemplate('', '/foo')).toBe('')
  })
})

describe('createCustomRunnerVariant', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('creates variant with correct id, name, and icon', () => {
    const variant = createCustomRunnerVariant(mockInstance, mockDeps)

    expect(variant.id).toBe('customrunner-rider')
    expect(variant.name).toBe('Rider')
    expect(variant.icon).toBe('🚀')
  })

  it('sets application properties correctly', () => {
    const variant = createCustomRunnerVariant(mockInstance, mockDeps)

    expect(variant.canClose).toBe(true)
    expect(variant.showInNewTabMenu).toBe(true)
    expect(variant.displayStyle).toBe('flex')
  })

  it('sets isDefault from instance', () => {
    const variant = createCustomRunnerVariant(mockInstance, mockDeps)
    expect(variant.isDefault).toBe(false)

    const defaultVariant = createCustomRunnerVariant({ ...mockInstance, isDefault: true }, mockDeps)
    expect(defaultVariant.isDefault).toBe(true)
  })

  describe('createInitialState', () => {
    it('returns terminal state with null ptyId', () => {
      const variant = createCustomRunnerVariant(mockInstance, mockDeps)
      const state = variant.createInitialState()

      expect(state).toEqual({ ptyId: null, ptyHandle: null, keepOnExit: false })
    })
  })

  describe('onWorkspaceLoad', () => {
    it('calls createTty with resolved command when ptyId is null', async () => {
      const variant = createCustomRunnerVariant(mockInstance, mockDeps)
      const tab: Tab = {
        id: 'tab-1',
        applicationId: 'customrunner-rider',
        title: 'Rider',
        state: { ptyId: null }
      }

      variant.onWorkspaceLoad(tab, mockWorkspaceStore)

      await vi.waitFor(() => {
        expect(mockWorkspaceStoreStateData.createTty).toHaveBeenCalledWith(
          '/test/project',
          undefined,
          'rider /test/project'
        )
      })
    })

    it('does not call createTty when ptyId already exists', () => {
      const variant = createCustomRunnerVariant(mockInstance, mockDeps)
      const tab: Tab = {
        id: 'tab-1',
        applicationId: 'customrunner-rider',
        title: 'Rider',
        state: { ptyId: 'existing-pty' }
      }

      variant.onWorkspaceLoad(tab, mockWorkspaceStore)

      expect(mockWorkspaceStoreStateData.createTty).not.toHaveBeenCalled()
    })

    it('returns AppRef with dispose method', () => {
      const variant = createCustomRunnerVariant(mockInstance, mockDeps)
      const tab: Tab = {
        id: 'tab-1',
        applicationId: 'customrunner-rider',
        title: 'Rider',
        state: { ptyId: null }
      }

      const ref = variant.onWorkspaceLoad(tab, mockWorkspaceStore)

      expect(typeof ref.dispose).toBe('function')
    })

    describe('dispose', () => {
      it('kills PTY when tab has ptyId and removes activity state', () => {
        const variant = createCustomRunnerVariant(mockInstance, mockDeps)
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'customrunner-rider',
          title: 'Rider',
          state: { ptyId: 'pty-123' }
        }

        const ref = variant.onWorkspaceLoad(tab, mockWorkspaceStore)
        ref.dispose()

        expect(mockTerminalKill).toHaveBeenCalledWith('local', 'pty-123')
        expect(mockRemoveTabState).toHaveBeenCalledWith('tab-1')
      })

      it('does not kill PTY when ptyId is null but removes activity state', () => {
        const variant = createCustomRunnerVariant(mockInstance, mockDeps)
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'customrunner-rider',
          title: 'Rider',
          state: { ptyId: null }
        }

        const ref = variant.onWorkspaceLoad(tab, mockWorkspaceStore)
        ref.dispose()

        expect(mockTerminalKill).not.toHaveBeenCalled()
        expect(mockRemoveTabState).toHaveBeenCalledWith('tab-1')
      })
    })
  })

  describe('render', () => {
    it('renders Terminal component with correct props', () => {
      const variant = createCustomRunnerVariant(mockInstance, mockDeps)
      const tab: Tab = {
        id: 'tab-1',
        applicationId: 'customrunner-rider',
        title: 'Rider',
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
          cwd: '/test/project',
          workspace: mockWorkspaceStore,
          tabId: 'tab-1',
          isVisible: true,
        })
      }))
    })
  })
})
