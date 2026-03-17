import { describe, it, expect, vi, beforeEach } from 'vitest'
import { createStore } from 'zustand/vanilla'
import { editorApplication } from './renderer'
import type { Tab, Workspace, EditorState } from '../../renderer/types'

// Mock React
vi.mock('react', () => ({
  createElement: vi.fn((component: any, props: any) => ({ component, props }))
}))

// Mock FileEditor component
vi.mock('../../renderer/components/FileEditor', () => ({
  FileEditor: vi.fn(() => null)
}))

import type { WorkspaceState } from "../../renderer/store/createWorkspaceStore"
const mockWorkspaceStore = createStore(() => ({} as WorkspaceState))

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
          tabs: [],
          activeTabId: null,
          metadata: {},
          createdAt: Date.now(),
          lastActivity: Date.now(),
          attachedClients: 1
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
          tabs: [],
          activeTabId: null,
          metadata: {},
          createdAt: Date.now(),
          lastActivity: Date.now(),
          attachedClients: 1
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
          tabs: [],
          activeTabId: null,
          metadata: {},
          createdAt: Date.now(),
          lastActivity: Date.now(),
          attachedClients: 1
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
          tabs: [],
          activeTabId: null,
          metadata: {},
          createdAt: Date.now(),
          lastActivity: Date.now(),
          attachedClients: 1
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
          workspaceId: 'ws-1',
          workspacePath: '/test',
          isVisible: true,
          workspaceStore: mockWorkspaceStore
        })

        expect(result).toEqual({
          component: expect.any(Function),
          props: expect.objectContaining({
            key: 'tab-1',
            workspaceId: 'ws-1',
            workspacePath: '/test',
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

        const result = editorApplication.render({
          tab,
          workspaceId: 'ws-2',
          workspacePath: '/workspace',
          isVisible: true,
          workspaceStore: mockWorkspaceStore
        }) as { props: { tabId: string } }

        expect(result.props.tabId).toBe('editor-tab-42')
      })

      it('passes workspaceId and workspacePath correctly', () => {
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

        const result = editorApplication.render({
          tab,
          workspaceId: 'project-ws',
          workspacePath: '/project',
          isVisible: true,
          workspaceStore: mockWorkspaceStore
        }) as { props: { workspaceId: string; workspacePath: string } }

        expect(result.props.workspaceId).toBe('project-ws')
        expect(result.props.workspacePath).toBe('/project')
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
          workspaceId: 'ws-1',
          workspacePath: '/test',
          isVisible: true,
          workspaceStore: mockWorkspaceStore
        })

        const cleanResult = editorApplication.render({
          tab: cleanTab,
          workspaceId: 'ws-1',
          workspacePath: '/test',
          isVisible: true,
          workspaceStore: mockWorkspaceStore
        })

        expect(dirtyResult).toBeDefined()
        expect(cleanResult).toBeDefined()
      })
    })
  })
})
