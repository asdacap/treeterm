import { describe, it, expect, vi, beforeEach } from 'vitest'
import { filesystemApplication } from './renderer'
import type { Tab, Workspace } from '../../renderer/types'
import { createStore } from 'zustand/vanilla'
import type { WorkspaceStoreState, WorkspaceStore } from '../../renderer/store/createWorkspaceStore'
import type { GitControllerState } from '../../renderer/store/createGitControllerStore'
import type { ReviewCommentState } from '../../renderer/store/createReviewCommentStore'

// Mock React
vi.mock('react', () => ({
  createElement: vi.fn((component: unknown, props: unknown) => ({ component, props }))
}))

// Mock FilesystemBrowser component
vi.mock('../../renderer/components/FilesystemBrowser', () => ({
  FilesystemBrowser: vi.fn(() => null)
}))

const mockReviewCommentStore = createStore<ReviewCommentState>()(() => ({
  getReviewComments: vi.fn().mockReturnValue([]),
  addReviewComment: vi.fn(),
  deleteReviewComment: vi.fn(),
  toggleReviewCommentAddressed: vi.fn(),
  updateOutdatedReviewComments: vi.fn(),
  clearReviewComments: vi.fn(),
  markAllReviewCommentsAddressed: vi.fn(),
} as ReviewCommentState))

const mockGitControllerStore = createStore<GitControllerState>()(() => ({
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
} as GitControllerState))

function createMockWorkspaceStoreStateData(overrides?: Partial<WorkspaceStoreState>): WorkspaceStoreState {
  return {
    workspace: { id: 'ws-1', path: '/test' } as Workspace,
    addTab: vi.fn(), openOrFocusTab: vi.fn(),
    removeTab: vi.fn(),
    setActiveTab: vi.fn(),
    updateTabTitle: vi.fn(),
    updateTabState: vi.fn(),
    reviewComments: mockReviewCommentStore,
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
    initTab: vi.fn(),
    getTabRef: vi.fn().mockReturnValue(null),
    disposeTabResources: vi.fn(),
    initAnalyzer: vi.fn(),
    createTty: vi.fn().mockResolvedValue('pty-1'), getTtyWriter: vi.fn().mockResolvedValue({ write: vi.fn<(data: string) => void>(), kill: vi.fn<() => void>() }),
    connectionId: 'local',
    gitController: mockGitControllerStore,
    ...overrides,
  } as WorkspaceStoreState
}

const mockWorkspaceStore = createStore<WorkspaceStoreState>()(() => createMockWorkspaceStoreStateData())

describe('Filesystem Renderer', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  describe('filesystemApplication', () => {
    it('has correct application properties', () => {
      expect(filesystemApplication.id).toBe('filesystem')
      expect(filesystemApplication.name).toBe('Files')
      expect(filesystemApplication.icon).toBe('\uD83D\uDCC2')
      expect(filesystemApplication.canClose).toBe(true)
      expect(filesystemApplication.showInNewTabMenu).toBe(true)
      expect(filesystemApplication.displayStyle).toBe('flex')
      expect(filesystemApplication.isDefault).toBe(true)
    })

    describe('createInitialState', () => {
      it('returns filesystem state with null selectedPath and empty expandedDirs', () => {
        const state = filesystemApplication.createInitialState()

        expect(state).toEqual({
          selectedPath: null,
          expandedDirs: []
        })
      })

      it('returns a fresh state object on each call', () => {
        const state1 = filesystemApplication.createInitialState()
        const state2 = filesystemApplication.createInitialState()

        expect(state1).toEqual(state2)
        expect(state1).not.toBe(state2)
        expect(state1.expandedDirs).not.toBe(state2.expandedDirs)
      })

      it('returns mutable array for expandedDirs', () => {
        const state = filesystemApplication.createInitialState()

        // Should be able to modify the array
        state.expandedDirs.push('/test')
        expect(state.expandedDirs).toEqual(['/test'])
      })
    })

    describe('render', () => {
      it('renders FilesystemBrowser component with correct props', () => {
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'filesystem',
          title: 'Files',
          state: {
            selectedPath: null,
            expandedDirs: []
          }
        }

        const result = filesystemApplication.render({
          tab,
          workspace: mockWorkspaceStore,
          isVisible: true,
        })

        expect(result).toEqual({
          // eslint-disable-next-line @typescript-eslint/no-unsafe-assignment
          component: expect.any(Function),
          // eslint-disable-next-line @typescript-eslint/no-unsafe-assignment
          props: expect.objectContaining({
            key: 'tab-1',
            workspace: mockWorkspaceStore,
            tabId: 'tab-1'
          })
        })
      })

      it('passes workspace correctly', () => {
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'filesystem',
          title: 'Files',
          state: {
            selectedPath: '/project/src',
            expandedDirs: ['/project', '/project/src']
          }
        }

        const wsHandle = createStore<WorkspaceStoreState>()(() => createMockWorkspaceStoreStateData({
          workspace: { id: 'project-ws', path: '/project' } as Workspace,
        }))

        const result = filesystemApplication.render({
          tab,
          workspace: wsHandle,
          isVisible: true,
        }) as { props: { workspace: WorkspaceStore } }

        expect(result.props.workspace.getState().workspace.id).toBe('project-ws')
        expect(result.props.workspace.getState().workspace.path).toBe('/project')
      })

      it('passes correct tabId', () => {
        const tab: Tab = {
          id: 'unique-tab-id',
          applicationId: 'filesystem',
          title: 'Files',
          state: {
            selectedPath: '/home/user',
            expandedDirs: []
          }
        }

        const result = filesystemApplication.render({
          tab,
          workspace: mockWorkspaceStore,
          isVisible: true,
        }) as { props: { tabId: string } }

        expect(result.props.tabId).toBe('unique-tab-id')
      })

      it('renders with state containing selected file', () => {
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'filesystem',
          title: 'Files',
          state: {
            selectedPath: '/test/document.txt',
            expandedDirs: ['/test', '/test/subdir']
          }
        }

        const result = filesystemApplication.render({
          tab,
          workspace: mockWorkspaceStore,
          isVisible: true,
        })

        expect(result).toBeDefined()
        expect(result).toHaveProperty('component')
        expect(result).toHaveProperty('props')
      })

      it('renders with empty expandedDirs', () => {
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'filesystem',
          title: 'Files',
          state: {
            selectedPath: null,
            expandedDirs: []
          }
        }

        const result = filesystemApplication.render({
          tab,
          workspace: mockWorkspaceStore,
          isVisible: true,
        })

        expect(result).toBeDefined()
      })

      it('renders with multiple expanded directories', () => {
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'filesystem',
          title: 'Files',
          state: {
            selectedPath: '/root/deep/file.txt',
            expandedDirs: ['/root', '/root/deep', '/root/deep/nested']
          }
        }

        const wsHandle = createStore<WorkspaceStoreState>()(() => createMockWorkspaceStoreStateData({
          workspace: { id: 'ws-root', path: '/root' } as Workspace,
        }))

        const result = filesystemApplication.render({
          tab,
          workspace: wsHandle,
          isVisible: true,
        })

        expect(result).toBeDefined()
      })

      it('renders regardless of isVisible flag', () => {
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'filesystem',
          title: 'Files',
          state: {
            selectedPath: null,
            expandedDirs: []
          }
        }

        const visibleResult = filesystemApplication.render({
          tab,
          workspace: mockWorkspaceStore,
          isVisible: true,
        })

        const hiddenResult = filesystemApplication.render({
          tab,
          workspace: mockWorkspaceStore,
          isVisible: false,
        })

        expect(visibleResult).toBeDefined()
        expect(hiddenResult).toBeDefined()
      })

      it('renders with null state gracefully', () => {
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'filesystem',
          title: 'Files',
          state: null
        }

        const result = filesystemApplication.render({
          tab,
          workspace: mockWorkspaceStore,
          isVisible: true,
        })

        // Filesystem renderer doesn't validate state, so it should still render
        expect(result).toBeDefined()
        expect(result).toHaveProperty('component')
        expect(result).toHaveProperty('props')
      })

      it('renders with invalid state gracefully', () => {
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'filesystem',
          title: 'Files',
          state: { invalid: true }
        }

        const result = filesystemApplication.render({
          tab,
          workspace: mockWorkspaceStore,
          isVisible: true,
        })

        // Filesystem renderer doesn't validate state, so it should still render
        expect(result).toBeDefined()
        expect(result).toHaveProperty('component')
        expect(result).toHaveProperty('props')
      })
    })

    describe('onWorkspaceLoad', () => {
      it('returns an AppRef with no-op dispose', () => {
        const tab: Tab = { id: 'tab-1', applicationId: 'filesystem', title: 'Files', state: {} }
        const ref = filesystemApplication.onWorkspaceLoad(tab, mockWorkspaceStore)
        expect(typeof ref.dispose).toBe('function')
        ref.dispose() // should not throw
      })
    })

    describe('isDefault flag', () => {
      it('is marked as default application', () => {
        expect(filesystemApplication.isDefault).toBe(true)
      })

      it('appears in new tab menu', () => {
        expect(filesystemApplication.showInNewTabMenu).toBe(true)
      })
    })
  })
})
