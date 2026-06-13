import type { ExecApi, FilesystemApi } from '../types'
import type { Workspace } from '../types'
import { resolveHomedir } from './homedir'
import { sha256Hex } from './sha256'

export const REGISTRY_DIR_REL = '.treeterm'
export const REGISTRY_FILE = 'worktrees.json'

/**
 * The registry file is shared across windows (and app instances). Each mutation is a
 * compare-and-swap read-modify-write: on conflict (another writer got in between) we
 * re-read and retry up to this many times before failing loudly.
 */
const MAX_CAS_RETRIES = 5

export interface WorktreeRegistryEntry {
  path: string
  branch: string
  displayName: string | null
  description: string | null
  lastUsedAt: number
}

interface WorktreeRegistryFile {
  version: 1
  entries: WorktreeRegistryEntry[]
}

export interface WorktreeRegistryApi {
  list: () => Promise<WorktreeRegistryEntry[]>
  upsert: (entry: Omit<WorktreeRegistryEntry, 'lastUsedAt'>) => Promise<void>
  remove: (path: string) => Promise<void>
}

function isMissingFileError(message: string): boolean {
  const lower = message.toLowerCase()
  return lower.includes('no such file') || lower.includes('not found') || lower.includes('os error 2')
}

async function resolveRegistryDir(exec: ExecApi, connectionId: string): Promise<string> {
  const home = await resolveHomedir(exec, connectionId)
  return `${home}/${REGISTRY_DIR_REL}`
}

/** Reads + parses the registry file. `sha256` is `''` when the file does not exist (CAS "must not exist"). */
async function readRegistryFile(
  fs: FilesystemApi,
  dir: string,
): Promise<{ entries: WorktreeRegistryEntry[]; sha256: string }> {
  const result = await fs.readFile(dir, REGISTRY_FILE)
  if (!result.success) {
    if (isMissingFileError(result.error)) return { entries: [], sha256: '' }
    throw new Error(`Failed to read worktree registry: ${result.error}`)
  }

  const parsed: unknown = JSON.parse(result.file.content)
  if (!parsed || typeof parsed !== 'object') {
    throw new Error('Worktree registry file has invalid shape')
  }
  const file = parsed as Partial<WorktreeRegistryFile>
  if (file.version !== 1 || !Array.isArray(file.entries)) {
    throw new Error(`Unsupported worktree registry version: ${String(file.version)}`)
  }
  return { entries: file.entries, sha256: await sha256Hex(result.file.content) }
}

export async function loadRegistry(
  fs: FilesystemApi,
  exec: ExecApi,
  connectionId: string,
): Promise<WorktreeRegistryEntry[]> {
  const dir = await resolveRegistryDir(exec, connectionId)
  return (await readRegistryFile(fs, dir)).entries
}

/**
 * Compare-and-swap read-modify-write. `mutate` returns the new entry list, or `null`
 * when no write is needed. A conflicting concurrent writer triggers a fresh
 * read + re-apply, so no window's update is silently lost.
 */
async function mutateRegistry(
  fs: FilesystemApi,
  exec: ExecApi,
  connectionId: string,
  mutate: (entries: WorktreeRegistryEntry[]) => WorktreeRegistryEntry[] | null,
): Promise<void> {
  const dir = await resolveRegistryDir(exec, connectionId)
  for (let attempt = 0; attempt < MAX_CAS_RETRIES; attempt++) {
    const { entries, sha256 } = await readRegistryFile(fs, dir)
    const mutated = mutate(entries)
    if (mutated === null) return
    const file: WorktreeRegistryFile = { version: 1, entries: mutated }
    const content = JSON.stringify(file, null, 2)
    const result = await fs.writeFile(dir, REGISTRY_FILE, content, sha256)
    if (result.success) return
    if (!('conflict' in result)) {
      throw new Error(`Failed to write worktree registry: ${result.error}`)
    }
    // Conflict: another window wrote between our read and write — retry from a fresh read.
  }
  throw new Error(`Failed to write worktree registry after ${String(MAX_CAS_RETRIES)} attempts (concurrent writers)`)
}

export async function upsertRegistryEntry(
  fs: FilesystemApi,
  exec: ExecApi,
  connectionId: string,
  entry: Omit<WorktreeRegistryEntry, 'lastUsedAt'>,
): Promise<void> {
  await mutateRegistry(fs, exec, connectionId, (entries) => {
    const stamped: WorktreeRegistryEntry = { ...entry, lastUsedAt: Date.now() }
    const idx = entries.findIndex(e => e.path === entry.path)
    if (idx >= 0) entries[idx] = stamped
    else entries.push(stamped)
    return entries
  })
}

export async function removeRegistryEntry(
  fs: FilesystemApi,
  exec: ExecApi,
  connectionId: string,
  path: string,
): Promise<void> {
  await mutateRegistry(fs, exec, connectionId, (entries) => {
    const filtered = entries.filter(e => e.path !== path)
    if (filtered.length === entries.length) return null
    return filtered
  })
}

export function buildEntryFromWorkspace(ws: Workspace, metadata: Record<string, string>): Omit<WorktreeRegistryEntry, 'lastUsedAt'> {
  return {
    path: ws.path,
    branch: ws.gitBranch ?? '',
    displayName: metadata.displayName ?? null,
    description: metadata.description ?? null,
  }
}

export function createWorktreeRegistryApi(
  fs: FilesystemApi,
  exec: ExecApi,
  connectionId: string,
): WorktreeRegistryApi {
  // Serialize all registry ops through a single promise chain. The registry is a single
  // shared JSON file and read-modify-write is not atomic — concurrent upserts race,
  // leaving the tail of a longer prior write when a shorter write lands at offset 0.
  let queue: Promise<unknown> = Promise.resolve()
  const enqueue = <T>(op: () => Promise<T>): Promise<T> => {
    const run = queue.then(op, op)
    queue = run.then(() => undefined, () => undefined)
    return run
  }
  return {
    list: () => enqueue(() => loadRegistry(fs, exec, connectionId)),
    upsert: (entry) => enqueue(() => upsertRegistryEntry(fs, exec, connectionId, entry)),
    remove: (path) => enqueue(() => removeRegistryEntry(fs, exec, connectionId, path)),
  }
}
