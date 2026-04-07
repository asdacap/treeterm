import { describe, it, expect, beforeEach } from 'vitest'
import { useRecentDirectoriesStore } from './recentDirectories'

describe('RecentDirectoriesStore', () => {
  beforeEach(() => {
    useRecentDirectoriesStore.setState({ directories: new Map() })
  })

  describe('getRecent', () => {
    it('returns empty array for unknown connection', () => {
      expect(useRecentDirectoriesStore.getState().getRecent('unknown')).toEqual([])
    })

    it('returns stable empty array reference', () => {
      const a = useRecentDirectoriesStore.getState().getRecent('x')
      const b = useRecentDirectoriesStore.getState().getRecent('y')
      expect(a).toBe(b)
    })
  })

  describe('addRecent', () => {
    it('adds a directory for a connection', () => {
      useRecentDirectoriesStore.getState().addRecent('conn1', '/home/user')
      expect(useRecentDirectoriesStore.getState().getRecent('conn1')).toEqual(['/home/user'])
    })

    it('puts most recent first', () => {
      const { addRecent } = useRecentDirectoriesStore.getState()
      addRecent('conn1', '/a')
      addRecent('conn1', '/b')
      expect(useRecentDirectoriesStore.getState().getRecent('conn1')).toEqual(['/b', '/a'])
    })

    it('deduplicates by moving existing to front', () => {
      const store = useRecentDirectoriesStore.getState()
      store.addRecent('conn1', '/a')
      store.addRecent('conn1', '/b')
      store.addRecent('conn1', '/a')
      expect(useRecentDirectoriesStore.getState().getRecent('conn1')).toEqual(['/a', '/b'])
    })

    it('limits to 10 entries', () => {
      const store = useRecentDirectoriesStore.getState()
      for (let i = 0; i < 15; i++) {
        store.addRecent('conn1', `/dir${String(i)}`)
      }
      const result = useRecentDirectoriesStore.getState().getRecent('conn1')
      expect(result).toHaveLength(10)
      expect(result[0]).toBe('/dir14')
    })

    it('keeps connections independent', () => {
      const store = useRecentDirectoriesStore.getState()
      store.addRecent('conn1', '/a')
      store.addRecent('conn2', '/b')
      expect(useRecentDirectoriesStore.getState().getRecent('conn1')).toEqual(['/a'])
      expect(useRecentDirectoriesStore.getState().getRecent('conn2')).toEqual(['/b'])
    })
  })
})
