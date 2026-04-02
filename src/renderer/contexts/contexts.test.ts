import { describe, it, expect, vi, beforeEach } from 'vitest'

const { mockCreateContext, mockUseContext } = vi.hoisted(() => ({
  mockCreateContext: vi.fn(() => ({ _currentValue: null })),
  mockUseContext: vi.fn()
}))

vi.mock('react', () => ({
  createContext: mockCreateContext,
  useContext: mockUseContext
}))

import { useSessionApi } from './SessionStoreContext'

describe('Context hooks', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  describe('useSessionApi', () => {
    it('throws when context is null', () => {
      mockUseContext.mockReturnValue(null)
      expect(() => useSessionApi()).toThrow(
        'useSessionApi must be used within a SessionStoreContext.Provider'
      )
    })

    it('returns value when context is provided', () => {
      const store = { getState: vi.fn() }
      mockUseContext.mockReturnValue(store)
      expect(useSessionApi()).toBe(store)
    })
  })
})
