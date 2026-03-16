import { createContext, useContext } from 'react'
import type { TerminalApi } from '../types'

export const TerminalApiContext = createContext<TerminalApi | null>(null)

export function useTerminalApi(): TerminalApi {
  const api = useContext(TerminalApiContext)
  if (!api) {
    throw new Error('useTerminalApi must be used within a TerminalApiContext.Provider')
  }
  return api
}
