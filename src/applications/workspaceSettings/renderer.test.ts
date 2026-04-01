import { describe, it, expect, vi, beforeEach } from 'vitest'
import { workspaceSettingsApplication } from './renderer'
import type { Tab, Workspace } from '../../renderer/types'
import { createMockGitApi, createMockFilesystemApi, createMockRunActionsApi } from '../../shared/mockApis'
import { createStore } from 'zustand/vanilla'
import type { WorkspaceStoreState } from '../../renderer/store/createWorkspaceStore'
import type { GitControllerState } from '../../renderer/store/createGitControllerStore'
import type { ReviewCommentState } from '../../renderer/store/createReviewCommentStore'

vi.mock('react', () => ({
  createElement: vi.fn((component: unknown, props: unknown) => ({ component, props }))
}))

vi.mock('../../renderer/components/WorkspaceSettings', () => ({
  default: vi.fn(() => null)
}))

const mockWorkspaceStore = createStore<WorkspaceStoreState>()(() => ({
  workspace: { id: 'ws-1', path: '/test' } as Workspace,
  addTab: vi.fn(), removeTab: vi.fn(), setActiveTab: vi.fn(),
  updateTabTitle: vi.fn(), updateTabState: vi.fn(),
  reviewComments: createStore<ReviewCommentState>()(() => ({
    getReviewComments: vi.fn().mockReturnValue([]),
    addReviewComment: vi.fn(), deleteReviewComment: vi.fn(),
    toggleReviewCommentAddressed: vi.fn(), updateOutdatedReviewComments: vi.fn(),
    clearReviewComments: vi.fn(), markAllReviewCommentsAddressed: vi.fn(),
  } as ReviewCommentState)),
  promptHarness: vi.fn(),
  quickForkWorkspace: vi.fn(), updateMetadata: vi.fn(),
  updateStatus: vi.fn(), refreshGitInfo: vi.fn(),
  mergeAndRemove: vi.fn(), mergeAndKeep: vi.fn(),
  closeAndClean: vi.fn(), lookupWorkspace: vi.fn(),
  remove: vi.fn(), removeKeepBranch: vi.fn(), removeKeepBoth: vi.fn(),
  initTab: vi.fn(), getTabRef: vi.fn().mockReturnValue(null),
  initAnalyzer: vi.fn(), createTty: vi.fn().mockResolvedValue('pty-1'),
  connectionId: 'local', updateSettings: vi.fn(),
  gitApi: createMockGitApi(), filesystemApi: createMockFilesystemApi(), runActionsApi: createMockRunActionsApi(),
  focusTabId: null, requestFocus: vi.fn(), clearFocusRequest: vi.fn(),
  gitController: createStore<GitControllerState>()(() => ({
    hasUncommittedChanges: false, isDiffCleanFromParent: false,
    hasConflictsWithParent: false, behindCount: 0, pullLoading: false,
    gitRefreshing: false, prInfo: null,
    refreshDiffStatus: vi.fn(), refreshRemoteStatus: vi.fn(),
    pullFromRemote: vi.fn(), refreshPrStatus: vi.fn(),
    openGitHub: vi.fn(), startPolling: vi.fn(), dispose: vi.fn(),
  } as GitControllerState)),
} as WorkspaceStoreState))

describe('WorkspaceSettings Renderer', () => {
  beforeEach(() => { vi.clearAllMocks() })

  it('has correct application properties', () => {
    expect(workspaceSettingsApplication.id).toBe('workspace-settings')
    expect(workspaceSettingsApplication.name).toBe('Settings')
    expect(workspaceSettingsApplication.canClose).toBe(true)
    expect(workspaceSettingsApplication.showInNewTabMenu).toBe(false)
    expect(workspaceSettingsApplication.displayStyle).toBe('flex')
    expect(workspaceSettingsApplication.isDefault).toBe(false)
  })

  it('creates empty initial state', () => {
    expect(workspaceSettingsApplication.createInitialState()).toEqual({})
  })

  it('onWorkspaceLoad returns disposable ref', () => {
    const tab = { id: 'tab-1', state: {} } as unknown as Tab
    const ref = workspaceSettingsApplication.onWorkspaceLoad(tab, mockWorkspaceStore)
    expect(typeof ref.dispose).toBe('function')
    ref.dispose()
  })

  it('renders WorkspaceSettings with key and workspace', () => {
    const tab = { id: 'tab-1', state: {} } as unknown as Tab
    const result = workspaceSettingsApplication.render({ tab, workspace: mockWorkspaceStore, isVisible: true })
    expect(result).toEqual({
      component: expect.any(Function),
      props: {
        key: 'tab-1',
        workspace: mockWorkspaceStore,
      },
    })
  })
})
