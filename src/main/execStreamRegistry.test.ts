import { describe, it, expect, vi } from 'vitest'
import { createExecStreamRegistry } from './execStreamRegistry'
import { ExecEventType, type ExecEvent } from '../shared/ipc-types'

interface FakeSender { id: string }

function makeStream(): { cancel: ReturnType<typeof vi.fn<() => void>> } {
  return { cancel: vi.fn<() => void>() }
}

describe('createExecStreamRegistry', () => {
  it('add/get/has/delete round-trip', () => {
    const registry = createExecStreamRegistry<{ cancel: () => void }, FakeSender>()
    const stream = makeStream()
    registry.add('e1', { stream, sender: { id: 'w1' }, connectionId: 'c1' })

    expect(registry.has('e1')).toBe(true)
    expect(registry.get('e1')?.stream).toBe(stream)

    registry.delete('e1')
    expect(registry.has('e1')).toBe(false)
    expect(registry.get('e1')).toBeUndefined()
  })

  it('failAllForConnection settles only the matching connection', () => {
    const registry = createExecStreamRegistry<{ cancel: () => void }, FakeSender>()
    const s1 = makeStream()
    const s2 = makeStream()
    const s3 = makeStream()
    registry.add('e1', { stream: s1, sender: { id: 'w1' }, connectionId: 'conn-a' })
    registry.add('e2', { stream: s2, sender: { id: 'w2' }, connectionId: 'conn-b' })
    registry.add('e3', { stream: s3, sender: { id: 'w1' }, connectionId: 'conn-a' })

    const notified: { sender: FakeSender; execId: string; event: ExecEvent }[] = []
    registry.failAllForConnection('conn-a', (sender, execId, event) => notified.push({ sender, execId, event }))

    // conn-a entries: cancelled, notified with a terminal Error, removed.
    expect(s1.cancel).toHaveBeenCalledOnce()
    expect(s3.cancel).toHaveBeenCalledOnce()
    expect(notified.map(n => ({ execId: n.execId, senderId: n.sender.id, type: n.event.type })).sort((a, b) => a.execId.localeCompare(b.execId))).toEqual([
      { execId: 'e1', senderId: 'w1', type: ExecEventType.Error },
      { execId: 'e3', senderId: 'w1', type: ExecEventType.Error },
    ])
    expect(registry.has('e1')).toBe(false)
    expect(registry.has('e3')).toBe(false)

    // conn-b entry untouched.
    expect(s2.cancel).not.toHaveBeenCalled()
    expect(registry.has('e2')).toBe(true)
  })

  it('a throwing cancel does not stop cleanup or notification', () => {
    const registry = createExecStreamRegistry<{ cancel: () => void }, FakeSender>()
    const bad = { cancel: vi.fn(() => { throw new Error('already dead') }) }
    const good = makeStream()
    registry.add('e1', { stream: bad, sender: { id: 'w1' }, connectionId: 'c' })
    registry.add('e2', { stream: good, sender: { id: 'w1' }, connectionId: 'c' })

    const notify = vi.fn()
    expect(() => { registry.failAllForConnection('c', notify); }).not.toThrow()

    expect(notify).toHaveBeenCalledTimes(2)
    expect(registry.has('e1')).toBe(false)
    expect(registry.has('e2')).toBe(false)
  })

  it('failAllForConnection is a no-op for an unknown connection', () => {
    const registry = createExecStreamRegistry<{ cancel: () => void }, FakeSender>()
    const stream = makeStream()
    registry.add('e1', { stream, sender: { id: 'w1' }, connectionId: 'c1' })

    const notify = vi.fn()
    registry.failAllForConnection('other', notify)

    expect(notify).not.toHaveBeenCalled()
    expect(registry.has('e1')).toBe(true)
  })
})
