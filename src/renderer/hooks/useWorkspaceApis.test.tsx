// @vitest-environment jsdom
import { describe, it, expect } from 'vitest'
import { renderHook } from '@testing-library/react'
import { createStore } from 'zustand/vanilla'
import { useGitApi, useFilesystemApi, useRunActionsApi } from './useWorkspaceApis'
import type { WorkspaceStoreState } from '../store/createWorkspaceStore'

function makeWorkspaceStore() {
  const gitApi = { getInfo: () => Promise.resolve({}) } as any
  const filesystemApi = { readDirectory: () => Promise.resolve([]) } as any
  const runActionsApi = { detect: () => Promise.resolve([]), run: () => Promise.resolve(null) } as any

  return createStore<WorkspaceStoreState>()(() => ({
    gitApi,
    filesystemApi,
    runActionsApi,
  }) as any)
}

describe('useWorkspaceApis', () => {
  it('useGitApi returns gitApi from workspace store', () => {
    const store = makeWorkspaceStore()
    const { result } = renderHook(() => useGitApi(store))
    expect(result.current).toBe(store.getState().gitApi)
  })

  it('useFilesystemApi returns filesystemApi from workspace store', () => {
    const store = makeWorkspaceStore()
    const { result } = renderHook(() => useFilesystemApi(store))
    expect(result.current).toBe(store.getState().filesystemApi)
  })

  it('useRunActionsApi returns runActionsApi from workspace store', () => {
    const store = makeWorkspaceStore()
    const { result } = renderHook(() => useRunActionsApi(store))
    expect(result.current).toBe(store.getState().runActionsApi)
  })
})
