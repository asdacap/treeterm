import { describe, it, expect, beforeEach, vi, afterEach } from 'vitest'
import { useSessionNamesStore } from './sessionNames'

describe('SessionNamesStore', () => {
  beforeEach(() => {
    useSessionNamesStore.setState({ names: new Map() })
    vi.useFakeTimers()
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  describe('setName', () => {
    it('sets a session name', () => {
      vi.setSystemTime(new Date('2025-01-01'))
      useSessionNamesStore.getState().setName('s1', 'My Session')
      expect(useSessionNamesStore.getState().getName('s1')).toBe('My Session')
    })

    it('overwrites existing name', () => {
      useSessionNamesStore.getState().setName('s1', 'Old')
      useSessionNamesStore.getState().setName('s1', 'New')
      expect(useSessionNamesStore.getState().getName('s1')).toBe('New')
    })
  })

  describe('getName', () => {
    it('returns undefined for unknown session', () => {
      expect(useSessionNamesStore.getState().getName('unknown')).toBeUndefined()
    })
  })

  describe('removeName', () => {
    it('removes an existing session name', () => {
      useSessionNamesStore.getState().setName('s1', 'Test')
      useSessionNamesStore.getState().removeName('s1')
      expect(useSessionNamesStore.getState().getName('s1')).toBeUndefined()
    })

    it('does nothing for non-existent session', () => {
      useSessionNamesStore.getState().removeName('nonexistent')
      expect(useSessionNamesStore.getState().names).toEqual(new Map())
    })
  })

  describe('cleanupStale', () => {
    it('removes entries older than 7 days', () => {
      vi.setSystemTime(new Date('2025-01-01'))
      useSessionNamesStore.getState().setName('old', 'Old Session')

      vi.setSystemTime(new Date('2025-01-09'))
      useSessionNamesStore.getState().setName('new', 'New Session')

      useSessionNamesStore.getState().cleanupStale()

      expect(useSessionNamesStore.getState().getName('old')).toBeUndefined()
      expect(useSessionNamesStore.getState().getName('new')).toBe('New Session')
    })

    it('keeps entries within 7 days', () => {
      vi.setSystemTime(new Date('2025-01-01'))
      useSessionNamesStore.getState().setName('recent', 'Recent')

      vi.setSystemTime(new Date('2025-01-07'))
      useSessionNamesStore.getState().cleanupStale()

      expect(useSessionNamesStore.getState().getName('recent')).toBe('Recent')
    })
  })
})
