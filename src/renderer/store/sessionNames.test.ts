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

  describe('reorderSession', () => {
    it('does nothing when dragId equals targetId', () => {
      useSessionNamesStore.getState().setName('s1', 'A')
      useSessionNamesStore.getState().reorderSession('s1', 's1', 'before')
      expect(useSessionNamesStore.getState().names.get('s1')?.sortOrder).toBe(0)
    })

    it('does nothing when dragId is not in names', () => {
      useSessionNamesStore.getState().setName('s1', 'A')
      useSessionNamesStore.getState().reorderSession('missing', 's1', 'before')
      expect(useSessionNamesStore.getState().names.size).toBe(1)
    })

    it('does nothing when targetId is not in names', () => {
      useSessionNamesStore.getState().setName('s1', 'A')
      useSessionNamesStore.getState().reorderSession('s1', 'missing', 'before')
      expect(useSessionNamesStore.getState().names.get('s1')?.sortOrder).toBe(0)
    })

    it('reorders with position before', () => {
      useSessionNamesStore.getState().setName('s1', 'A')
      useSessionNamesStore.getState().setName('s2', 'B')
      useSessionNamesStore.getState().setName('s3', 'C')
      // Move s3 before s1
      useSessionNamesStore.getState().reorderSession('s3', 's1', 'before')
      const sorted = useSessionNamesStore.getState().getSortedIds(['s1', 's2', 's3'])
      expect(sorted).toEqual(['s3', 's1', 's2'])
    })

    it('reorders with position after', () => {
      useSessionNamesStore.getState().setName('s1', 'A')
      useSessionNamesStore.getState().setName('s2', 'B')
      useSessionNamesStore.getState().setName('s3', 'C')
      // Move s1 after s2
      useSessionNamesStore.getState().reorderSession('s1', 's2', 'after')
      const sorted = useSessionNamesStore.getState().getSortedIds(['s1', 's2', 's3'])
      expect(sorted).toEqual(['s2', 's1', 's3'])
    })
  })

  describe('getSortedIds', () => {
    it('sorts known IDs by sortOrder', () => {
      useSessionNamesStore.getState().setName('s1', 'First')
      useSessionNamesStore.getState().setName('s2', 'Second')
      useSessionNamesStore.getState().setName('s3', 'Third')
      const sorted = useSessionNamesStore.getState().getSortedIds(['s3', 's1', 's2'])
      expect(sorted).toEqual(['s1', 's2', 's3'])
    })

    it('puts unknown IDs at the end', () => {
      useSessionNamesStore.getState().setName('s1', 'Known')
      const sorted = useSessionNamesStore.getState().getSortedIds(['unknown', 's1', 'also-unknown'])
      expect(sorted[0]).toBe('s1')
    })

    it('returns empty for empty input', () => {
      expect(useSessionNamesStore.getState().getSortedIds([])).toEqual([])
    })
  })

  describe('setName sortOrder', () => {
    it('preserves existing sortOrder on overwrite', () => {
      useSessionNamesStore.getState().setName('s1', 'A')
      useSessionNamesStore.getState().setName('s2', 'B')
      const originalOrder = useSessionNamesStore.getState().names.get('s1')?.sortOrder
      useSessionNamesStore.getState().setName('s1', 'Updated')
      expect(useSessionNamesStore.getState().names.get('s1')?.sortOrder).toBe(originalOrder)
    })

    it('assigns new sortOrder for new entry', () => {
      useSessionNamesStore.getState().setName('s1', 'A')
      useSessionNamesStore.getState().setName('s2', 'B')
      const s1Order = useSessionNamesStore.getState().names.get('s1')?.sortOrder ?? -1
      const s2Order = useSessionNamesStore.getState().names.get('s2')?.sortOrder ?? -1
      expect(s2Order).toBeGreaterThan(s1Order)
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
