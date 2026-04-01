import { useStore } from 'zustand'
import { useRef } from 'react'
import type { WorkspaceStore, WorkspaceGitApi, WorkspaceFilesystemApi } from '../types'

export function useGitApi(workspace: WorkspaceStore): WorkspaceGitApi {
  const { getGitApi } = useStore(workspace)
  const ref = useRef(getGitApi())
  return ref.current
}

export function useFilesystemApi(workspace: WorkspaceStore): WorkspaceFilesystemApi {
  const { getFilesystemApi } = useStore(workspace)
  const ref = useRef(getFilesystemApi())
  return ref.current
}
