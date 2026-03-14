import { describe, it, expect, vi, beforeEach } from 'vitest'
import {
  createTerminalApplication,
  terminalApplication,
  createTerminalVariant
} from './renderer'
import type { Tab, Workspace, TerminalInstance, TerminalState } from '../../renderer/types'

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

// Mock window.electron
const mockTerminalKill = vi.fn()
;(globalThis as unknown as { window: { electron: { terminal: { kill: typeof mockTerminalKill } } } }).window = {
  electron: {
    terminal: {
      kill: mockTerminalKill
    }
  }
}

describe('Terminal Renderer', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  describe('createTerminalApplication', () => {
    it('creates terminal application with correct properties', () => {
      const app = createTerminalApplication(true)

      expect(app.id).toBe('terminal')
      expect(app.name).toBe('Terminal')
      expect(app.icon).toBe('>')
      expect(app.canClose).toBe(true)
      expect(app.canHaveMultiple).toBe(true)
      expect(app.showInNewTabMenu).toBe(true)
      expect(app.keepAlive).toBe(true)
      expect(app.displayStyle).toBe('flex')
    })

    it('sets isDefault based on parameter', () => {
      const appWithDefault = createTerminalApplication(true)
      expect(appWithDefault.isDefault).toBe(true)

      const appWithoutDefault = createTerminalApplication(false)
      expect(appWithoutDefault.isDefault).toBe(false)
    })

    describe('createInitialState', () => {
      it('returns terminal state with null ptyId', () => {
        const app = createTerminalApplication(true)
        const state = app.createInitialState()

        expect(state).toEqual({ ptyId: null })
      })
    })

    describe('cleanup', () => {
      it('kills PTY when tab has ptyId', async () => {
        const app = createTerminalApplication(true)
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'terminal',
          title: 'Terminal',
          state: { ptyId: 'pty-123' }
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
          createdAt: Date.now(),
          lastActivity: Date.now(),
          attachedClients: 1
        }

        await app.cleanup?.(tab, workspace)

        expect(mockTerminalKill).toHaveBeenCalledWith('pty-123')
        expect(mockRemoveTabState).toHaveBeenCalledWith('tab-1')
      })

      it('does not kill PTY when tab has no ptyId', async () => {
        const app = createTerminalApplication(true)
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'terminal',
          title: 'Terminal',
          state: { ptyId: null }
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
          createdAt: Date.now(),
          lastActivity: Date.now(),
          attachedClients: 1
        }

        await app.cleanup?.(tab, workspace)

        expect(mockTerminalKill).not.toHaveBeenCalled()
        expect(mockRemoveTabState).toHaveBeenCalledWith('tab-1')
      })

      it('does not kill PTY when state is not terminal state', async () => {
        const app = createTerminalApplication(true)
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'terminal',
          title: 'Terminal',
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
          createdAt: Date.now(),
          lastActivity: Date.now(),
          attachedClients: 1
        }

        await app.cleanup?.(tab, workspace)

        expect(mockTerminalKill).not.toHaveBeenCalled()
        expect(mockRemoveTabState).toHaveBeenCalledWith('tab-1')
      })

      it('removes activity state regardless of PTY', async () => {
        const app = createTerminalApplication(true)
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'terminal',
          title: 'Terminal',
          state: { ptyId: null }
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
          createdAt: Date.now(),
          lastActivity: Date.now(),
          attachedClients: 1
        }

        await app.cleanup?.(tab, workspace)

        expect(mockRemoveTabState).toHaveBeenCalledTimes(1)
        expect(mockRemoveTabState).toHaveBeenCalledWith('tab-1')
      })
    })

    describe('render', () => {
      it('renders Terminal component with correct props', () => {
        const app = createTerminalApplication(true)
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'terminal',
          title: 'Terminal',
          state: { ptyId: null }
        }

        const result = app.render({
          tab,
          workspaceId: 'ws-1',
          workspacePath: '/test',
          isVisible: true
        })

        expect(result).toEqual({
          component: expect.any(Function),
          props: {
            key: 'tab-1',
            cwd: '/test',
            workspaceId: 'ws-1',
            tabId: 'tab-1',
            isVisible: true
          }
        })
      })
    })
  })

  describe('terminalApplication', () => {
    it('is a terminal application with isDefault set to true', () => {
      expect(terminalApplication.id).toBe('terminal')
      expect(terminalApplication.name).toBe('Terminal')
      expect(terminalApplication.isDefault).toBe(true)
    })

    it('can create initial state', () => {
      const state = terminalApplication.createInitialState()
      expect(state).toEqual({ ptyId: null })
    })

    it('can render Terminal component', () => {
      const tab: Tab = {
        id: 'tab-1',
        applicationId: 'terminal',
        title: 'Terminal',
        state: { ptyId: null }
      }

      const result = terminalApplication.render({
        tab,
        workspaceId: 'ws-1',
        workspacePath: '/test',
        isVisible: true
      })

      expect(result).toBeDefined()
      expect(result).toHaveProperty('component')
      expect(result).toHaveProperty('props')
    })

    it('can cleanup tabs', async () => {
      const tab: Tab = {
        id: 'tab-1',
        applicationId: 'terminal',
        title: 'Terminal',
        state: { ptyId: 'pty-123' }
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
        createdAt: Date.now(),
        lastActivity: Date.now(),
        attachedClients: 1
      }

      await terminalApplication.cleanup?.(tab, workspace)

      expect(mockTerminalKill).toHaveBeenCalledWith('pty-123')
      expect(mockRemoveTabState).toHaveBeenCalledWith('tab-1')
    })
  })

  describe('createTerminalVariant', () => {
    const mockInstance: TerminalInstance = {
      id: 'custom-term',
      name: 'Custom Terminal',
      icon: '🚀',
      startupCommand: 'echo "Hello"',
      isDefault: false
    }

    it('creates variant with custom id and name', () => {
      const variant = createTerminalVariant(mockInstance)

      expect(variant.id).toBe('terminal-custom-term')
      expect(variant.name).toBe('Custom Terminal')
      expect(variant.icon).toBe('🚀')
    })

    it('preserves other application properties', () => {
      const variant = createTerminalVariant(mockInstance)

      expect(variant.canClose).toBe(true)
      expect(variant.canHaveMultiple).toBe(true)
      expect(variant.showInNewTabMenu).toBe(true)
      expect(variant.keepAlive).toBe(true)
      expect(variant.displayStyle).toBe('flex')
    })

    it('sets isDefault from instance', () => {
      const variant = createTerminalVariant(mockInstance)
      expect(variant.isDefault).toBe(false)

      const defaultVariant = createTerminalVariant({
        ...mockInstance,
        isDefault: true
      })
      expect(defaultVariant.isDefault).toBe(true)
    })

    describe('createInitialState', () => {
      it('returns terminal state with null ptyId', () => {
        const variant = createTerminalVariant(mockInstance)
        const state = variant.createInitialState()

        expect(state).toEqual({ ptyId: null })
      })
    })

    describe('cleanup', () => {
      it('kills PTY when tab has ptyId', async () => {
        const variant = createTerminalVariant(mockInstance)
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'terminal-custom-term',
          title: 'Custom Terminal',
          state: { ptyId: 'pty-456' }
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
          createdAt: Date.now(),
          lastActivity: Date.now(),
          attachedClients: 1
        }

        await variant.cleanup?.(tab, workspace)

        expect(mockTerminalKill).toHaveBeenCalledWith('pty-456')
        expect(mockRemoveTabState).toHaveBeenCalledWith('tab-1')
      })

      it('handles invalid state gracefully', async () => {
        const variant = createTerminalVariant(mockInstance)
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'terminal-custom-term',
          title: 'Custom Terminal',
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
          createdAt: Date.now(),
          lastActivity: Date.now(),
          attachedClients: 1
        }

        await variant.cleanup?.(tab, workspace)

        expect(mockTerminalKill).not.toHaveBeenCalled()
        expect(mockRemoveTabState).toHaveBeenCalledWith('tab-1')
      })
    })

    describe('render', () => {
      it('renders Terminal component with startup command', () => {
        const variant = createTerminalVariant(mockInstance)
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'terminal-custom-term',
          title: 'Custom Terminal',
          state: { ptyId: null }
        }

        const result = variant.render({
          tab,
          workspaceId: 'ws-1',
          workspacePath: '/test',
          isVisible: true
        })

        expect(result).toEqual({
          component: expect.any(Function),
          props: {
            key: 'tab-1',
            cwd: '/test',
            workspaceId: 'ws-1',
            tabId: 'tab-1',
            isVisible: true,
            startupCommand: 'echo "Hello"'
          }
        })
      })

      it('passes empty startupCommand when not set', () => {
        const variantWithoutCommand = createTerminalVariant({
          ...mockInstance,
          startupCommand: ''
        })
        const tab: Tab = {
          id: 'tab-1',
          applicationId: 'terminal-custom-term',
          title: 'Custom Terminal',
          state: { ptyId: null }
        }

        const result = variantWithoutCommand.render({
          tab,
          workspaceId: 'ws-1',
          workspacePath: '/test',
          isVisible: true
        }) as { props: { startupCommand: string } }

        expect(result.props.startupCommand).toBe('')
      })
    })
  })
})
