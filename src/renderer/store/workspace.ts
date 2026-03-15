// Re-exports for backward compatibility — consumers keep their existing import paths.
export { useWorkspaceStore } from './WorkspaceStoreContext'
export type { WorkspaceState } from './createWorkspaceStore'
export { getUnmergedSubWorkspaces } from './createWorkspaceStore'
