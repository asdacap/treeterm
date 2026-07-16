/**
 * Schema and types for the per-workspace JSON files.
 *
 * Workspace bodies live in `<session.workspaceDataDir>/<workspace-id>.json`,
 * written through the daemon's CAS `WriteFile` and observed through `WatchFile`.
 * The daemon treats the file as opaque bytes — this module is the sole owner of
 * its schema. `id` and `path` are NOT stored in the file: they come from the
 * in-process `WorkspaceRef` list and are joined back on at read time, so there
 * is no duplicated state to keep in sync.
 */
import { z } from 'zod'

export enum WorkspaceStatus {
  Active = 'active',
  Merged = 'merged',
  Abandoned = 'abandoned',
}

// Per-tab application state. `state` is application-defined and validated by the
// owning application, so it stays `unknown` here.
export const AppStateSchema = z.object({
  applicationId: z.string(),
  title: z.string(),
  state: z.unknown(),
})

export type AppState = z.infer<typeof AppStateSchema>

export const WorkspaceFileSchema = z.object({
  name: z.string(),
  parentId: z.string().optional(),
  status: z.enum(WorkspaceStatus),
  isGitRepo: z.boolean(),
  gitBranch: z.string().optional(),
  gitRootPath: z.string().optional(),
  isWorktree: z.boolean(),
  isDetached: z.boolean().optional(),
  appStates: z.record(z.string(), AppStateSchema),
  activeTabId: z.string().optional(),
  metadata: z.record(z.string(), z.string()),
  favouritePaths: z.array(z.string()).default([]),
  createdAt: z.number(),
  lastActivity: z.number(),
})

/** The logical body the store mutates (everything except the ref's id/path). */
export type WorkspaceFile = z.infer<typeof WorkspaceFileSchema>

/** The envelope actually persisted to disk: the logical body plus `parentHash`,
 *  the sha256 of the body this write supersedes (`''` for the first version).
 *  Chaining the parent's hash into every body makes successive bodies hash to
 *  distinct values even when their logical content reverts to an earlier state —
 *  the hash sequence never repeats, which lets the writer dedup watch echoes by
 *  sha alone. `parentHash` is write-time bookkeeping, not mutable state, so it is
 *  stripped on read (it equals the writer's last-seen sha and is recalculable). */
export const StoredWorkspaceFileSchema = WorkspaceFileSchema.extend({
  parentHash: z.string(),
})

export type StoredWorkspaceFile = z.infer<typeof StoredWorkspaceFileSchema>

/** A workspace as the renderer uses it: the file body joined with its ref. */
export type Workspace = WorkspaceFile & { id: string; path: string }

/** Build the on-disk envelope for a workspace, dropping the ref-only fields and
 *  chaining in the parent body's hash. The caller stringifies it (with stable key
 *  ordering) before writing. */
export function toStoredWorkspaceFile(workspace: Workspace, parentHash: string): StoredWorkspaceFile {
  const file: Partial<Workspace> = { ...workspace }
  delete file.id
  delete file.path
  return { ...(file as WorkspaceFile), parentHash }
}

/** Parse and validate a JSON file body, joining the ref's id/path back on and
 *  dropping `parentHash` (bookkeeping, not store state).
 *  Throws on invalid JSON or schema mismatch (fail loudly — the caller surfaces
 *  the workspace as an error rather than guessing). */
export function parseWorkspaceFile(id: string, path: string, content: string): Workspace {
  const parsed: unknown = JSON.parse(content)
  const file: Partial<StoredWorkspaceFile> = { ...StoredWorkspaceFileSchema.parse(parsed) }
  delete file.parentHash
  return { ...(file as WorkspaceFile), id, path }
}
