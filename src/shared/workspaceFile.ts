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
  createdAt: z.number(),
  lastActivity: z.number(),
})

/** The body persisted to the JSON file (everything except the ref's id/path). */
export type WorkspaceFile = z.infer<typeof WorkspaceFileSchema>

/** A workspace as the renderer uses it: the file body joined with its ref. */
export type Workspace = WorkspaceFile & { id: string; path: string }

/** Serialize a workspace to its JSON file body, dropping the ref-only fields. */
export function toWorkspaceFile(workspace: Workspace): WorkspaceFile {
  const file: Partial<Workspace> = { ...workspace }
  delete file.id
  delete file.path
  return file as WorkspaceFile
}

/** Parse and validate a JSON file body, joining the ref's id/path back on.
 *  Throws on invalid JSON or schema mismatch (fail loudly — the caller surfaces
 *  the workspace as an error rather than guessing). */
export function parseWorkspaceFile(id: string, path: string, content: string): Workspace {
  const parsed: unknown = JSON.parse(content)
  const file = WorkspaceFileSchema.parse(parsed)
  return { ...file, id, path }
}
