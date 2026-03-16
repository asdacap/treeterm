import { createContext, useContext } from 'react'
import type { ElectronApi } from '../types'

export const ElectronContext = createContext<ElectronApi | null>(null)

export function useElectron(): ElectronApi {
  const electron = useContext(ElectronContext)
  if (!electron) {
    throw new Error('useElectron must be used within an ElectronContext.Provider')
  }
  return electron
}
