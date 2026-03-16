import { createContext, useContext } from 'react'
import type { GitApi } from '../types'

export const GitApiContext = createContext<GitApi | null>(null)

export function useGitApi(): GitApi {
  const api = useContext(GitApiContext)
  if (!api) {
    throw new Error('useGitApi must be used within a GitApiContext.Provider')
  }
  return api
}
