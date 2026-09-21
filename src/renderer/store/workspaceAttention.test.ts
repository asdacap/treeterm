import { describe, expect, it } from 'vitest'
import {
  AttentionMutationType, WORKSPACE_ATTENTION_KEY, reconcileAttentionWrite, withWatchedAttention,
  type AttentionMutation, type AttentionSyncState,
} from './workspaceAttention'

function metadata(revision: string, acknowledgedRevision = ''): Record<string, string> {
  return { [WORKSPACE_ATTENTION_KEY]: JSON.stringify({ revision, acknowledgedRevision }) }
}

function record(revision: string): AttentionMutation {
  return { type: AttentionMutationType.Record, revision }
}

describe('attention write reconciliation', () => {
  it('acknowledges an own echo while retaining mutations queued after the write', () => {
    const first = record('first')
    const second = record('second')
    const state: AttentionSyncState = { mutations: [first, second], writes: new Map([['own-sha', [first]]]) }
    reconcileAttentionWrite(state, metadata('first'), 'own-sha')
    expect(state.mutations).toEqual([second])
    expect(state.writes.size).toBe(0)
  })

  it('reconciles a coalesced cross-window acknowledgement and cleans skipped write hashes', () => {
    const notification = record('first')
    const ack: AttentionMutation = { type: AttentionMutationType.Acknowledge, revision: 'first' }
    const later = record('second')
    const state: AttentionSyncState = {
      mutations: [notification, ack, later],
      writes: new Map([['skipped', [notification, ack]], ['pending', [later]]]),
    }
    reconcileAttentionWrite(state, metadata('first', 'first'), 'external')
    expect(state.mutations).toEqual([later])
    expect(Array.from(state.writes.keys())).toEqual(['pending'])
  })

  it('acknowledges superseded mutations included in the committed body by hash', () => {
    const first = record('first')
    const second = record('second')
    const state: AttentionSyncState = { mutations: [first, second], writes: new Map([['own', [first, second]]]) }
    reconcileAttentionWrite(state, metadata('second'), 'own')
    expect(state.mutations).toEqual([])
    expect(state.writes.size).toBe(0)
  })

  it('rejects invalid attention before changing queued mutations', () => {
    const pending = record('pending')
    const state: AttentionSyncState = { mutations: [pending], writes: new Map() }
    expect(() => { reconcileAttentionWrite(state, { [WORKSPACE_ATTENTION_KEY]: '{}' }, 'invalid') }).toThrow()
    expect(state.mutations).toEqual([pending])
  })
})

describe('own-echo attention metadata', () => {
  it('keeps local metadata while publishing only watched attention', () => {
    const current = { ...metadata('old'), displayName: 'local name' }
    expect(withWatchedAttention(current, { ...metadata('new'), displayName: 'old name' }))
      .toEqual({ ...metadata('new'), displayName: 'local name' })
    expect(current).toEqual({ ...metadata('old'), displayName: 'local name' })
  })

  it('removes attention absent from the watched body and preserves identity when unchanged', () => {
    const current = { ...metadata('old'), displayName: 'local name' }
    expect(withWatchedAttention(current, {})).toEqual({ displayName: 'local name' })
    expect(withWatchedAttention(current, metadata('old'))).toBe(current)
    const empty = {}
    expect(withWatchedAttention(empty, {})).toBe(empty)
  })
})
