import { describe, it, expect, vi, beforeEach } from 'vitest'
import { chatApplication } from './renderer'
import type { Tab, Workspace } from '../../renderer/types'
import { createStore } from 'zustand/vanilla'
import type { WorkspaceStoreState } from '../../renderer/store/createWorkspaceStore'

vi.mock('react', () => ({
  createElement: vi.fn((component: unknown, props: unknown) => ({ component, props }))
}))

vi.mock('../../renderer/components/Chat', () => ({
  default: vi.fn(() => null)
}))

const mockWorkspaceStore = createStore<WorkspaceStoreState>()(() => ({
  workspace: { id: 'ws-1', path: '/test' } as Workspace,
  addTab: vi.fn(), removeTab: vi.fn(), setActiveTab: vi.fn(),
  updateTabTitle: vi.fn(), updateTabState: vi.fn(),
  getReviewComments: vi.fn(), addReviewComment: vi.fn(),
  deleteReviewComment: vi.fn(), toggleReviewCommentAddressed: vi.fn(),
  updateOutdatedReviewComments: vi.fn(), clearReviewComments: vi.fn(),
  markAllReviewCommentsAddressed: vi.fn(), promptHarness: vi.fn(),
  quickForkWorkspace: vi.fn(), updateMetadata: vi.fn(),
  updateStatus: vi.fn(), refreshGitInfo: vi.fn(),
  mergeAndRemove: vi.fn(), mergeAndKeep: vi.fn(),
  closeAndClean: vi.fn(), lookupWorkspace: vi.fn(),
  remove: vi.fn(), removeKeepBranch: vi.fn(), removeKeepBoth: vi.fn(),
  initTab: vi.fn(), getTabRef: vi.fn().mockReturnValue(null),
  initAnalyzer: vi.fn(), createTty: vi.fn().mockResolvedValue('pty-1'),
  connectionId: 'local', updateSettings: vi.fn(),
  getGitApi: vi.fn(), getFilesystemApi: vi.fn(),
  hasUncommittedChanges: false, isDiffCleanFromParent: false,
  hasConflictsWithParent: false, disposeGitController: vi.fn(),
  focusTabId: null, requestFocus: vi.fn(), clearFocusRequest: vi.fn(),
  behindCount: 0, pullLoading: false, refreshRemoteStatus: vi.fn(),
  pullFromRemote: vi.fn(), refreshDiffStatus: vi.fn(),
  gitRefreshing: false, prInfo: null, refreshPrStatus: vi.fn(),
  openGitHub: vi.fn(),
} as WorkspaceStoreState))

describe('Chat Renderer', () => {
  beforeEach(() => { vi.clearAllMocks() })

  it('has correct application properties', () => {
    expect(chatApplication.id).toBe('chat')
    expect(chatApplication.name).toBe('Chat')
    expect(chatApplication.canClose).toBe(true)
    expect(chatApplication.showInNewTabMenu).toBe(true)
    expect(chatApplication.displayStyle).toBe('flex')
    expect(chatApplication.isDefault).toBe(false)
  })

  it('creates initial state with empty messages', () => {
    const state = chatApplication.createInitialState()
    expect(state).toEqual({ messages: [] })
  })

  it('onWorkspaceLoad returns disposable ref', () => {
    const tab = { id: 'tab-1', state: { messages: [] } } as unknown as Tab
    const ref = chatApplication.onWorkspaceLoad(tab, mockWorkspaceStore)
    expect(ref).toHaveProperty('dispose')
    expect(typeof ref.dispose).toBe('function')
  })

  it('renders Chat component via createElement', () => {
    const tab = { id: 'tab-1', state: { messages: [] } } as unknown as Tab
    const result = chatApplication.render({ tab, workspace: mockWorkspaceStore, isVisible: true })
    expect(result).toEqual({
      component: expect.any(Function),
      props: expect.objectContaining({ tab }),
    })
  })
})
