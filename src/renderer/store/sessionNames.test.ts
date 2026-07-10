import { describe, it, expect, beforeEach, vi, afterEach } from 'vitest'
import { useSessionNamesStore, deriveDefaultSessionName } from './sessionNames'
import { ConnectionStatus, ConnectionTargetType, type ConnectionInfo } from '../../shared/types'

describe('deriveDefaultSessionName', () => {
  it('returns LOCAL for a local connection', () => {
    const conn: ConnectionInfo = {
      id: 'local',
      target: { type: ConnectionTargetType.Local },
      status: ConnectionStatus.Connected,
    }
    expect(deriveDefaultSessionName(conn)).toBe('LOCAL')
  })

  it('returns user@host for a remote connection without a label', () => {
    const conn: ConnectionInfo = {
      id: 'c1',
      target: { type: ConnectionTargetType.Remote, config: { id: 'c1', host: 'myserver.com', user: 'alice', port: 22, portForwards: [] } },
      status: ConnectionStatus.Connected,
    }
    expect(deriveDefaultSessionName(conn)).toBe('alice@myserver.com')
  })

  it('prefers the label over user@host when a label is set', () => {
    const conn: ConnectionInfo = {
      id: 'c2',
      target: { type: ConnectionTargetType.Remote, config: { id: 'c2', host: 'myserver.com', user: 'alice', port: 22, label: 'Production', portForwards: [] } },
      status: ConnectionStatus.Connected,
    }
    expect(deriveDefaultSessionName(conn)).toBe('Production')
  })

  it('falls back to user@host when the label is an empty string', () => {
    const conn: ConnectionInfo = {
      id: 'c3',
      target: { type: ConnectionTargetType.Remote, config: { id: 'c3', host: 'myserver.com', user: 'alice', port: 22, label: '', portForwards: [] } },
      status: ConnectionStatus.Connected,
    }
    expect(deriveDefaultSessionName(conn)).toBe('alice@myserver.com')
  })
})

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

  describe('clearName', () => {
    it('clears an existing session name', () => {
      useSessionNamesStore.getState().setName('s1', 'Test')
      useSessionNamesStore.getState().clearName('s1')
      expect(useSessionNamesStore.getState().getName('s1')).toBeUndefined()
    })

    it('does nothing for non-existent session', () => {
      useSessionNamesStore.getState().clearName('nonexistent')
      expect(useSessionNamesStore.getState().names).toEqual(new Map())
    })

    it('keeps the session position when the name is cleared', () => {
      const ids = ['s1', 's2', 's3']
      useSessionNamesStore.getState().setName('s3', 'C')
      useSessionNamesStore.getState().reorderSession(ids, 's3', 's1', 'before')
      useSessionNamesStore.getState().clearName('s3')
      expect(useSessionNamesStore.getState().getSortedIds(ids)).toEqual(['s3', 's1', 's2'])
    })
  })

  describe('reorderSession', () => {
    const ids = ['s1', 's2', 's3']

    it('does nothing when dragId equals targetId', () => {
      useSessionNamesStore.getState().reorderSession(ids, 's1', 's1', 'before')
      expect(useSessionNamesStore.getState().names.size).toBe(0)
    })

    it('does nothing when dragId is not in the session list', () => {
      useSessionNamesStore.getState().reorderSession(ids, 'missing', 's1', 'before')
      expect(useSessionNamesStore.getState().names.size).toBe(0)
    })

    it('does nothing when targetId is not in the session list', () => {
      useSessionNamesStore.getState().reorderSession(ids, 's1', 'missing', 'before')
      expect(useSessionNamesStore.getState().names.size).toBe(0)
    })

    it('reorders sessions that have never been renamed', () => {
      useSessionNamesStore.getState().reorderSession(ids, 's3', 's1', 'before')
      expect(useSessionNamesStore.getState().getSortedIds(ids)).toEqual(['s3', 's1', 's2'])
    })

    it('does not invent a custom name for a reordered session', () => {
      useSessionNamesStore.getState().reorderSession(ids, 's3', 's1', 'before')
      expect(useSessionNamesStore.getState().getName('s3')).toBeUndefined()
    })

    it('reorders with position before', () => {
      useSessionNamesStore.getState().setName('s1', 'A')
      useSessionNamesStore.getState().setName('s2', 'B')
      useSessionNamesStore.getState().setName('s3', 'C')
      // Move s3 before s1
      useSessionNamesStore.getState().reorderSession(ids, 's3', 's1', 'before')
      expect(useSessionNamesStore.getState().getSortedIds(ids)).toEqual(['s3', 's1', 's2'])
    })

    it('reorders with position after', () => {
      useSessionNamesStore.getState().setName('s1', 'A')
      useSessionNamesStore.getState().setName('s2', 'B')
      useSessionNamesStore.getState().setName('s3', 'C')
      // Move s1 after s2
      useSessionNamesStore.getState().reorderSession(ids, 's1', 's2', 'after')
      expect(useSessionNamesStore.getState().getSortedIds(ids)).toEqual(['s2', 's1', 's3'])
    })

    it('preserves the custom name of a reordered session', () => {
      useSessionNamesStore.getState().setName('s3', 'C')
      useSessionNamesStore.getState().reorderSession(ids, 's3', 's1', 'before')
      expect(useSessionNamesStore.getState().getName('s3')).toBe('C')
    })

    it('interleaves a renamed session with never-renamed ones', () => {
      // s2 is the only session with an entry, so it starts with sortOrder 0 while
      // s1/s3 sort last. Dragging s1 must still place it relative to s2.
      useSessionNamesStore.getState().setName('s2', 'B')
      useSessionNamesStore.getState().reorderSession(ids, 's1', 's2', 'after')
      expect(useSessionNamesStore.getState().getSortedIds(ids)).toEqual(['s2', 's1', 's3'])
    })

    it('reorders a session dropped onto the last position', () => {
      useSessionNamesStore.getState().reorderSession(ids, 's1', 's3', 'after')
      expect(useSessionNamesStore.getState().getSortedIds(ids)).toEqual(['s2', 's3', 's1'])
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
