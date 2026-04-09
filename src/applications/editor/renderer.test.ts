import { describe, it, expect, vi, beforeEach } from 'vitest'
import { editorApplication } from './renderer'
import type { Tab, Workspace } from '../../renderer/types'
import { EditorStatus, EditorViewMode } from '../../renderer/types'
import { createStore } from 'zustand/vanilla'
import type { WorkspaceStoreState, WorkspaceStore } from '../../renderer/store/createWorkspaceStore'
import type { GitControllerState } from '../../renderer/store/createGitControllerStore'
import type { ReviewCommentState } from '../../renderer/store/createReviewCommentStore'

// Mock React
vi.mock('react', () => ({
  createElement: vi.fn((component: unknown, props: unknown) => ({ component, props }))
}))

// Mock FileEditor component
vi.mock('../../renderer/components/FileEditor', () => ({
  FileEditor: vi.fn(() => null)
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
    getCachedTerminal: vi.fn().mockReturnValue(null),
    setCachedTerminal: vi.fn(),
    disposeCachedTerminal: vi.fn(), disposeAllCachedTerminals: vi.fn(), disposeTabResources: vi.fn(),
    initAnalyzer: vi.fn(),
    createTty: vi.fn().mockResolvedValue('pty-1'), getTtyWriter: vi.fn().mockResolvedValue({ write: vi.fn<(data: string) => void>(), kill: vi.fn<() => void>() }),
    connectionId: 'local',
    focusTabId: null,
    requestFocus: vi.fn(),
    clearFocusRequest: vi.fn(),
    gitController: mockGitControllerStore,
    ...overrides,
  } as WorkspaceStoreState
}

const mockWorkspaceStoreStateData = createMockWorkspaceStoreStateData()

const mockWorkspaceStore = createStore<WorkspaceStoreState>()(() => mockWorkspaceStoreStateData)

describe('Editor Renderer', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  describe('editorApplication', () => {
    it('has correct application properties', () => {
      expect(editorApplication.id).toBe('editor')
      expect(editorApplication.name).toBe('Editor')
      expect(editorApplication.icon).toBe('\u270F')
      expect(editorApplication.canClose).toBe(true)
      expect(editorApplication.showInNewTabMenu).toBe(false)
      expect(editorApplication.displayStyle).toBe('flex')
      expect(editorApplication.isDefault).toBe(false)
    })

    describe('createInitialState', () => {
      it('returns editor state with default values', () => {
        const state = editorApplication.createInitialState()

        expect(state).toEqual({
          status: EditorStatus.Ready,
          filePath: '',
          originalContent: '',
          currentContent: '',
          language: 'plaintext',
          isDirty: false,
          viewMode: EditorViewMode.Editor,
        })
      })

      it('returns a fresh state object on each call', () => {
        const state1 = editorApplication.createInitialState()
        const state2 = editorApplication.createInitialState()

        expect(state1).toEqual(state2)
        expect(state1).not.toBe(state2)
      })
    })

    describe('onWorkspaceLoad', () => {
      it('returns an AppRef with dispose', () => {
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'editor',
          title: 'test.txt',
          state: {
            filePath: '/test/file.txt',
            originalContent: 'original',
            currentContent: 'modified',
            language: 'plaintext',
            isDirty: true,
            viewMode: 'editor',
            isLoading: false,
            error: null
          }
        }

        const ref = editorApplication.onWorkspaceLoad(tab, mockWorkspaceStore)

        expect(typeof ref.dispose).toBe('function')
        // dispose is a no-op for editor
        ref.dispose()
      })
    })

    describe('render', () => {
      it('renders FileEditor component with correct props', () => {
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'editor',
          title: 'test.txt',
          state: {
            filePath: '/test/file.txt',
            originalContent: 'content',
            currentContent: 'content',
            language: 'plaintext',
            isDirty: false,
            viewMode: 'editor',
            isLoading: false,
            error: null
          }
        }

        const result = editorApplication.render({
          tab,
          workspace: mockWorkspaceStore,
          isVisible: true,
        })

        expect(result).toEqual({
          component: expect.any(Function) as unknown,
          props: expect.objectContaining({
            key: 'tab-1',
            workspace: mockWorkspaceStore,
            tabId: 'tab-1'
          }) as unknown,
        })
      })

      it('renders with correct tabId', () => {
        const tab: Tab = {
          id: 'editor-tab-42',
          applicationId: 'editor',
          title: 'document.md',
          state: {
            filePath: '/workspace/doc.md',
            originalContent: '# Hello',
            currentContent: '# Hello World',
            language: 'markdown',
            isDirty: true,
            viewMode: 'editor',
            isLoading: false,
            error: null
          }
        }

        const wsHandle = createStore<WorkspaceStoreState>()(() => createMockWorkspaceStoreStateData({
          workspace: { id: 'ws-2', path: '/workspace' } as Workspace,
        }))

        const result = editorApplication.render({
          tab,
          workspace: wsHandle,
          isVisible: true,
        }) as { props: { tabId: string } }

        expect(result.props.tabId).toBe('editor-tab-42')
      })

      it('passes workspace correctly', () => {
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'editor',
          title: 'script.js',
          state: {
            filePath: '/project/src/script.js',
            originalContent: '',
            currentContent: 'const x = 1;',
            language: 'javascript',
            isDirty: true,
            viewMode: 'editor',
            isLoading: false,
            error: null
          }
        }

        const wsHandle = createStore<WorkspaceStoreState>()(() => createMockWorkspaceStoreStateData({
          workspace: { id: 'project-ws', path: '/project' } as Workspace,
        }))

        const result = editorApplication.render({
          tab,
          workspace: wsHandle,
          isVisible: true,
        }) as { props: { workspace: WorkspaceStore } }

        expect(result.props.workspace.getState().workspace.id).toBe('project-ws')
        expect(result.props.workspace.getState().workspace.path).toBe('/project')
      })

      it('renders regardless of isDirty state', () => {
        const dirtyTab: Tab = {
          id: 'tab-1',
          applicationId: 'editor',
          title: 'dirty.txt',
          state: {
            filePath: '/test/dirty.txt',
            originalContent: 'orig',
            currentContent: 'modified',
            language: 'plaintext',
            isDirty: true,
            viewMode: 'editor',
            isLoading: false,
            error: null
          }
        }

        const cleanTab: Tab = {
          id: 'tab-2',
          applicationId: 'editor',
          title: 'clean.txt',
          state: {
            filePath: '/test/clean.txt',
            originalContent: 'same',
            currentContent: 'same',
            language: 'plaintext',
            isDirty: false,
            viewMode: 'editor',
            isLoading: false,
            error: null
          }
        }

        const dirtyResult = editorApplication.render({
          tab: dirtyTab,
          workspace: mockWorkspaceStore,
          isVisible: true,
        })

        const cleanResult = editorApplication.render({
          tab: cleanTab,
          workspace: mockWorkspaceStore,
          isVisible: true,
        })

        expect(dirtyResult).toBeDefined()
        expect(cleanResult).toBeDefined()
      })
    })
  })
})
