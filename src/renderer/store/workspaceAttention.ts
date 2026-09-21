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
