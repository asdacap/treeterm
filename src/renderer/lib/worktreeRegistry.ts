import type { ExecApi, FilesystemApi } from '../types'
import type { Workspace } from '../types'
import { resolveHomedir } from './homedir'

export const REGISTRY_DIR_REL = '.treeterm'
export const REGISTRY_FILE = 'worktrees.json'

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

export async function loadRegistry(
  fs: FilesystemApi,
  exec: ExecApi,
  connectionId: string,
): Promise<WorktreeRegistryEntry[]> {
  const dir = await resolveRegistryDir(exec, connectionId)
  const result = await fs.readFile(dir, REGISTRY_FILE)
  if (!result.success) {
    if (isMissingFileError(result.error)) return []
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
  return file.entries
}

async function writeRegistry(
  fs: FilesystemApi,
  exec: ExecApi,
  connectionId: string,
  entries: WorktreeRegistryEntry[],
): Promise<void> {
  const dir = await resolveRegistryDir(exec, connectionId)
  const file: WorktreeRegistryFile = { version: 1, entries }
  const content = JSON.stringify(file, null, 2)
  const result = await fs.writeFile(dir, REGISTRY_FILE, content)
  if (!result.success) {
    throw new Error(`Failed to write worktree registry: ${result.error}`)
  }
}

export async function upsertRegistryEntry(
  fs: FilesystemApi,
  exec: ExecApi,
  connectionId: string,
  entry: Omit<WorktreeRegistryEntry, 'lastUsedAt'>,
): Promise<void> {
  const entries = await loadRegistry(fs, exec, connectionId)
  const stamped: WorktreeRegistryEntry = { ...entry, lastUsedAt: Date.now() }
  const idx = entries.findIndex(e => e.path === entry.path)
  if (idx >= 0) entries[idx] = stamped
  else entries.push(stamped)
  await writeRegistry(fs, exec, connectionId, entries)
}

export async function removeRegistryEntry(
  fs: FilesystemApi,
  exec: ExecApi,
  connectionId: string,
  path: string,
): Promise<void> {
  const entries = await loadRegistry(fs, exec, connectionId)
  const filtered = entries.filter(e => e.path !== path)
  if (filtered.length === entries.length) return
  await writeRegistry(fs, exec, connectionId, filtered)
}

export function buildEntryFromWorkspace(ws: Workspace): Omit<WorktreeRegistryEntry, 'lastUsedAt'> {
  return {
    path: ws.path,
    branch: ws.gitBranch ?? '',
    displayName: ws.metadata.displayName ?? null,
    description: ws.metadata.description ?? null,
  }
}

export function createWorktreeRegistryApi(
  fs: FilesystemApi,
  exec: ExecApi,
  connectionId: string,
): WorktreeRegistryApi {
  return {
    list: () => loadRegistry(fs, exec, connectionId),
    upsert: (entry) => upsertRegistryEntry(fs, exec, connectionId, entry),
    remove: (path) => removeRegistryEntry(fs, exec, connectionId, path),
  }
}
