import { createContext, useContext } from 'react'
import type { STTApi } from '../types'

export const STTApiContext = createContext<STTApi | null>(null)

export function useSTTApi(): STTApi {
  const api = useContext(STTApiContext)
  if (!api) {
    throw new Error('useSTTApi must be used within a STTApiContext.Provider')
  }
  return api
}
