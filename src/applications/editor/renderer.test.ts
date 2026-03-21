import { describe, it, expect, vi, beforeEach } from 'vitest'
import { editorApplication } from './renderer'
import type { Tab, Workspace, EditorState } from '../../renderer/types'
import type { WorkspaceHandle } from '../../renderer/store/createWorkspaceStore'

// Mock React
vi.mock('react', () => ({
  createElement: vi.fn((component: any, props: any) => ({ component, props }))
}))

// Mock FileEditor component
vi.mock('../../renderer/components/FileEditor', () => ({
  FileEditor: vi.fn(() => null)
}))

const mockWorkspaceHandle = {
  id: 'ws-1',
  data: { path: '/test' } as Workspace,
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
} satisfies WorkspaceHandle

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
      expect(editorApplication.canHaveMultiple).toBe(true)
      expect(editorApplication.showInNewTabMenu).toBe(false)
      expect(editorApplication.keepAlive).toBe(false)
      expect(editorApplication.displayStyle).toBe('flex')
      expect(editorApplication.isDefault).toBe(false)
    })

    describe('createInitialState', () => {
      it('returns editor state with default values', () => {
        const state = editorApplication.createInitialState()

        expect(state).toEqual({
          filePath: '',
          originalContent: '',
          currentContent: '',
          language: 'plaintext',
          isDirty: false,
          viewMode: 'editor',
          isLoading: false,
          error: null
        })
      })

      it('returns a fresh state object on each call', () => {
        const state1 = editorApplication.createInitialState()
        const state2 = editorApplication.createInitialState()

        expect(state1).toEqual(state2)
        expect(state1).not.toBe(state2)
      })
    })

    describe('cleanup', () => {
      it('logs warning when closing tab with unsaved changes', async () => {
        const consoleSpy = vi.spyOn(console, 'warn').mockImplementation(() => {})
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

        await editorApplication.cleanup?.(tab, workspace)

        expect(consoleSpy).toHaveBeenCalledWith('Editor tab closed with unsaved changes')
        consoleSpy.mockRestore()
      })

      it('does not log warning when tab has no unsaved changes', async () => {
        const consoleSpy = vi.spyOn(console, 'warn').mockImplementation(() => {})
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

        await editorApplication.cleanup?.(tab, workspace)

        expect(consoleSpy).not.toHaveBeenCalled()
        consoleSpy.mockRestore()
      })

      it('does not log warning when state is not editor state', async () => {
        const consoleSpy = vi.spyOn(console, 'warn').mockImplementation(() => {})
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'editor',
          title: 'test.txt',
          state: { someOtherState: true }
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

        await editorApplication.cleanup?.(tab, workspace)

        expect(consoleSpy).not.toHaveBeenCalled()
        consoleSpy.mockRestore()
      })

      it('handles null state gracefully', async () => {
        const consoleSpy = vi.spyOn(console, 'warn').mockImplementation(() => {})
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'editor',
          title: 'test.txt',
          state: null
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

        await editorApplication.cleanup?.(tab, workspace)

        expect(consoleSpy).not.toHaveBeenCalled()
        consoleSpy.mockRestore()
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
          workspace: mockWorkspaceHandle,
          isVisible: true,
        })

        expect(result).toEqual({
          component: expect.any(Function),
          props: expect.objectContaining({
            key: 'tab-1',
            workspace: mockWorkspaceHandle,
            tabId: 'tab-1'
          })
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

        const wsHandle = { ...mockWorkspaceHandle, id: 'ws-2', data: { path: '/workspace' } as Workspace }

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

        const wsHandle = { ...mockWorkspaceHandle, id: 'project-ws', data: { path: '/project' } as Workspace }

        const result = editorApplication.render({
          tab,
          workspace: wsHandle,
          isVisible: true,
        }) as { props: { workspace: WorkspaceHandle } }

        expect(result.props.workspace.id).toBe('project-ws')
        expect(result.props.workspace.data.path).toBe('/project')
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
          workspace: mockWorkspaceHandle,
          isVisible: true,
        })

        const cleanResult = editorApplication.render({
          tab: cleanTab,
          workspace: mockWorkspaceHandle,
          isVisible: true,
        })

        expect(dirtyResult).toBeDefined()
        expect(cleanResult).toBeDefined()
      })
    })
  })
})
