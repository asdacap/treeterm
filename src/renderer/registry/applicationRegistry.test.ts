import { describe, it, expect, beforeEach } from 'vitest'
import { applicationRegistry } from './applicationRegistry'
import type { Application } from '../types'

describe('ApplicationRegistry', () => {
  const mockApp1: Application = {
    id: 'app1',
    name: 'Application 1',
    icon: '📱',
    createInitialState: () => ({}),
    render: () => null,
    canClose: true,
    canHaveMultiple: false,
    showInNewTabMenu: true,
    keepAlive: false,
    displayStyle: 'flex',
    isDefault: true
  }

  const mockApp2: Application = {
    id: 'app2',
    name: 'Application 2',
    icon: '🔧',
    createInitialState: () => ({}),
    render: () => null,
    canClose: true,
    canHaveMultiple: true,
    showInNewTabMenu: true,
    keepAlive: false,
    displayStyle: 'flex',
    isDefault: false
  }

  const mockApp3: Application = {
    id: 'app3',
    name: 'Application 3',
    icon: '📝',
    createInitialState: () => ({}),
    render: () => null,
    canClose: true,
    canHaveMultiple: false,
    showInNewTabMenu: false,
    keepAlive: false,
    displayStyle: 'flex',
    isDefault: false
  }

  beforeEach(() => {
    // Clear registry before each test
    applicationRegistry.unregister('app1')
    applicationRegistry.unregister('app2')
    applicationRegistry.unregister('app3')
  })

  describe('register', () => {
    it('registers an application', () => {
      applicationRegistry.register(mockApp1)
      const app = applicationRegistry.get('app1')
      expect(app).toEqual(mockApp1)
    })

    it('registers multiple applications', () => {
      applicationRegistry.register(mockApp1)
      applicationRegistry.register(mockApp2)
      
      expect(applicationRegistry.get('app1')).toEqual(mockApp1)
      expect(applicationRegistry.get('app2')).toEqual(mockApp2)
    })
  })

  describe('unregister', () => {
    it('removes a registered application', () => {
      applicationRegistry.register(mockApp1)
      applicationRegistry.unregister('app1')
      
      expect(applicationRegistry.get('app1')).toBeUndefined()
    })

    it('does nothing when unregistering non-existent app', () => {
      expect(() => applicationRegistry.unregister('nonexistent')).not.toThrow()
    })
  })

  describe('get', () => {
    it('returns undefined for non-existent application', () => {
      expect(applicationRegistry.get('nonexistent')).toBeUndefined()
    })

    it('returns the correct application', () => {
      applicationRegistry.register(mockApp1)
      expect(applicationRegistry.get('app1')).toEqual(mockApp1)
    })
  })

  describe('getAll', () => {
    it('returns empty array when no apps registered', () => {
      expect(applicationRegistry.getAll()).toEqual([])
    })

    it('returns all registered applications', () => {
      applicationRegistry.register(mockApp1)
      applicationRegistry.register(mockApp2)
      
      const allApps = applicationRegistry.getAll()
      expect(allApps).toHaveLength(2)
      expect(allApps).toContainEqual(mockApp1)
      expect(allApps).toContainEqual(mockApp2)
    })
  })

  describe('getMenuItems', () => {
    it('returns only apps with showInNewTabMenu=true', () => {
      applicationRegistry.register(mockApp1)
      applicationRegistry.register(mockApp2)
      applicationRegistry.register(mockApp3)
      
      const menuItems = applicationRegistry.getMenuItems()
      expect(menuItems).toHaveLength(2)
      expect(menuItems).toContainEqual(mockApp1)
      expect(menuItems).toContainEqual(mockApp2)
      expect(menuItems).not.toContainEqual(mockApp3)
    })
  })

  describe('getDefaultApps', () => {
    it('returns only default apps', () => {
      applicationRegistry.register(mockApp1)
      applicationRegistry.register(mockApp2)
      
      const defaultApps = applicationRegistry.getDefaultApps()
      expect(defaultApps).toHaveLength(1)
      expect(defaultApps[0]).toEqual(mockApp1)
    })
  })

  describe('getDefaultApp', () => {
    it('returns app by ID if found', () => {
      applicationRegistry.register(mockApp1)
      applicationRegistry.register(mockApp2)
      
      const app = applicationRegistry.getDefaultApp('app2')
      expect(app).toEqual(mockApp2)
    })

    it('falls back to first available app when ID not specified', () => {
      applicationRegistry.register(mockApp1)
      applicationRegistry.register(mockApp2)
      
      const app = applicationRegistry.getDefaultApp()
      expect(app).toBeDefined()
    })

    it('returns null when no apps registered', () => {
      const app = applicationRegistry.getDefaultApp()
      expect(app).toBeNull()
    })

    it('falls back to first app when specified ID not found', () => {
      applicationRegistry.register(mockApp1)
      
      const app = applicationRegistry.getDefaultApp('nonexistent')
      expect(app).toEqual(mockApp1)
    })
  })
})
