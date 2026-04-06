import { useStore } from 'zustand'
import type { WorkspaceStore, WorkspaceGitApi, WorkspaceFilesystemApi, ExecApi, RunActionsApi } from '../types'

export function useGitApi(workspace: WorkspaceStore): WorkspaceGitApi {
  return useStore(workspace, s => s.gitApi)
}

export function useFilesystemApi(workspace: WorkspaceStore): WorkspaceFilesystemApi {
  return useStore(workspace, s => s.filesystemApi)
}

export function useExecApi(workspace: WorkspaceStore): ExecApi {
  return useStore(workspace, s => s.execApi)
}

export function useRunActionsApi(workspace: WorkspaceStore): RunActionsApi {
  return useStore(workspace, s => s.runActionsApi)
}
