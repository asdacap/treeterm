import { createContext, useContext } from 'react'
import type { FilesystemApi } from '../types'

export const FilesystemApiContext = createContext<FilesystemApi | null>(null)

export function useFilesystemApi(): FilesystemApi {
  const api = useContext(FilesystemApiContext)
  if (!api) {
    throw new Error('useFilesystemApi must be used within a FilesystemApiContext.Provider')
  }
  return api
}
