import { z } from 'zod'

export const WORKSPACE_ATTENTION_KEY = 'workspaceAttention'
const AttentionSchema = z.object({ revision: z.string(), acknowledgedRevision: z.string() })
export type WorkspaceAttention = z.infer<typeof AttentionSchema>

export function getWorkspaceAttention(metadata: Record<string, string>): WorkspaceAttention {
  const raw = metadata[WORKSPACE_ATTENTION_KEY]
  return raw === undefined ? { revision: '', acknowledgedRevision: '' } : AttentionSchema.parse(JSON.parse(raw) as unknown)
}

export function hasUnreadWorkspaceAttention(metadata: Record<string, string>): boolean {
  const attention = getWorkspaceAttention(metadata)
  return attention.revision !== attention.acknowledgedRevision
}

export enum AttentionMutationType {
  Record = 'record',
  Acknowledge = 'acknowledge',
}
export interface AttentionMutation {
  type: AttentionMutationType
  revision: string
}

export function applyAttentionMutations(metadata: Record<string, string>, mutations: AttentionMutation[]): Record<string, string> {
  if (mutations.length === 0) return metadata
  let attention = getWorkspaceAttention(metadata)
  for (const mutation of mutations) {
    if (mutation.type === AttentionMutationType.Record) {
      attention = { ...attention, revision: mutation.revision }
    } else if (attention.revision === mutation.revision) {
      attention = { ...attention, acknowledgedRevision: mutation.revision }
    }
  }
  return { ...metadata, [WORKSPACE_ATTENTION_KEY]: JSON.stringify(attention) }
}

// Attention owns its revision acknowledgements; file transport supplies only the
// committed body hash. Entries are registered before WriteFile so an early watch
// echo can acknowledge them, and discarded by transport when a write fails.
export interface AttentionSyncState {
  mutations: AttentionMutation[]
  writes: Map<string, AttentionMutation[]>
}

export function reconcileAttentionWrite(state: AttentionSyncState, metadata: Record<string, string>, sha: string): void {
  const watched = getWorkspaceAttention(metadata)
  // A coalesced watch can skip our body and contain another window's acknowledgement.
  state.mutations = state.mutations.filter(mutation =>
    mutation.type === AttentionMutationType.Record
      ? mutation.revision !== watched.revision
      : mutation.revision !== watched.acknowledgedRevision)
  const committed = state.writes.get(sha)
  if (committed) {
    state.mutations = state.mutations.filter(mutation => !committed.includes(mutation))
    state.writes.delete(sha)
  }
  for (const [hash, mutations] of Array.from(state.writes)) {
    if (mutations.every(mutation => !state.mutations.includes(mutation))) state.writes.delete(hash)
  }
}

// Own echoes must publish watched attention without replacing optimistic tab or
// other metadata edits made while the write was in flight.
export function withWatchedAttention(current: Record<string, string>, watched: Record<string, string>): Record<string, string> {
  const value = watched[WORKSPACE_ATTENTION_KEY]
  if (current[WORKSPACE_ATTENTION_KEY] === value) return current
  const metadata = Object.fromEntries(Object.entries(current).filter(([key]) => key !== WORKSPACE_ATTENTION_KEY))
  if (value !== undefined) metadata[WORKSPACE_ATTENTION_KEY] = value
  return metadata
}
