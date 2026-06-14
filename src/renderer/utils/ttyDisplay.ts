import type { Workspace } from '../types'

/** Compact "Ns/Nm/Nh/Nd ago" for a millisecond timestamp. */
export function formatRelativeTime(timestamp: number): string {
  const seconds = Math.floor((Date.now() - timestamp) / 1000)
  if (seconds < 60) return `${String(seconds)}s ago`
  const minutes = Math.floor(seconds / 60)
  if (minutes < 60) return `${String(minutes)}m ago`
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return `${String(hours)}h ago`
  const days = Math.floor(hours / 24)
  return `${String(days)}d ago`
}

/** The trailing path segment, falling back to the whole string. */
export function lastSegment(cwd: string): string {
  const parts = cwd.split('/')
  return parts[parts.length - 1] || cwd
}

/** Resolve a PTY's cwd to a human label: the owning workspace's name when the
 *  cwd matches a known workspace path, else the cwd's last segment. */
export function getDisplayName(cwd: string, workspaces: Record<string, Workspace>): string {
  const workspace = Object.values(workspaces).find((ws) => ws.path === cwd)
  if (workspace) return workspace.name
  return lastSegment(cwd)
}
