import { describe, it, expect, vi, beforeEach } from 'vitest'
import { filesystemApplication } from './renderer'
import type { Tab, Workspace, FilesystemState } from '../../renderer/types'

// Mock React
vi.mock('react', () => ({
  createElement: vi.fn((component: any, props: any) => ({ component, props }))
}))

// Mock FilesystemBrowser component
vi.mock('../../renderer/components/FilesystemBrowser', () => ({
  FilesystemBrowser: vi.fn(() => null)
}))

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
      expect(filesystemApplication.canHaveMultiple).toBe(true)
      expect(filesystemApplication.showInNewTabMenu).toBe(true)
      expect(filesystemApplication.keepAlive).toBe(false)
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
          workspaceId: 'ws-1',
          workspacePath: '/test',
          isVisible: true
        })

        expect(result).toEqual({
          component: expect.any(Function),
          props: {
            key: 'tab-1',
            workspacePath: '/test',
            workspaceId: 'ws-1',
            tabId: 'tab-1'
          }
        })
      })

      it('passes workspaceId and workspacePath correctly', () => {
        const tab: Tab = {
          id: 'fs-tab-123',
          applicationId: 'filesystem',
          title: 'Files',
          state: {
            selectedPath: '/project/src',
            expandedDirs: ['/project', '/project/src']
          }
        }

        const result = filesystemApplication.render({
          tab,
          workspaceId: 'project-ws',
          workspacePath: '/project',
          isVisible: true
        }) as { props: { workspaceId: string; workspacePath: string } }

        expect(result.props.workspaceId).toBe('project-ws')
        expect(result.props.workspacePath).toBe('/project')
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
          workspaceId: 'ws-1',
          workspacePath: '/test',
          isVisible: true
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
          workspaceId: 'ws-1',
          workspacePath: '/test',
          isVisible: true
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
          workspaceId: 'ws-1',
          workspacePath: '/test',
          isVisible: true
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

        const result = filesystemApplication.render({
          tab,
          workspaceId: 'ws-1',
          workspacePath: '/root',
          isVisible: true
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
          workspaceId: 'ws-1',
          workspacePath: '/test',
          isVisible: true
        })

        const hiddenResult = filesystemApplication.render({
          tab,
          workspaceId: 'ws-1',
          workspacePath: '/test',
          isVisible: false
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
          workspaceId: 'ws-1',
          workspacePath: '/test',
          isVisible: true
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
          workspaceId: 'ws-1',
          workspacePath: '/test',
          isVisible: true
        })

        // Filesystem renderer doesn't validate state, so it should still render
        expect(result).toBeDefined()
        expect(result).toHaveProperty('component')
        expect(result).toHaveProperty('props')
      })
    })

    describe('cleanup', () => {
      it('cleanup is undefined for filesystem application', () => {
        // Filesystem application doesn't need cleanup
        expect(filesystemApplication.cleanup).toBeUndefined()
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
