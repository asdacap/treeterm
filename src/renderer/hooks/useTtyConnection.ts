import { useState, useEffect, useRef } from 'react'
import type { SandboxConfig } from '../types'
import { useSessionApi } from '../contexts/SessionStoreContext'

export type UseTtyCreationResult =
  | { status: 'loading' }
  | { status: 'ready' }
  | { status: 'error'; error: Error }

/**
 * Creates a PTY if one doesn't already exist for this tab.
 * Persists the new ptyId to workspace state via onCreated callback.
 * Returns loading/error state so the caller can show appropriate UI
 * before rendering BaseTerminal.
 */
export function useTtyCreation(
  existingPtyId: string | null,
  cwd: string,
  sandbox: SandboxConfig | undefined,
  startupCommand: string | undefined,
  onCreated: (ptyId: string) => void
): UseTtyCreationResult {
  const [status, setStatus] = useState<UseTtyCreationResult>(existingPtyId ? { status: 'ready' } : { status: 'loading' })
  const sessionStore = useSessionApi()
  const createTtyPromiseRef = useRef<Promise<string> | null>(null)
  const mountedRef = useRef(true)

  useEffect(() => {
    if (existingPtyId) return

    mountedRef.current = true
    let cancelled = false

    async function create() {
      const session = sessionStore.getState()
      try {
        // Deduplicate creation across StrictMode double-mounts
        let promise = createTtyPromiseRef.current
        if (!promise) {
          promise = session.createTty(cwd, sandbox, startupCommand)
          createTtyPromiseRef.current = promise
        }

        const ptyId = await promise
        createTtyPromiseRef.current = null

        if (cancelled) {
          if (!mountedRef.current) {
            session.killTty(ptyId)
          }
          return
        }

        onCreated(ptyId)
        setStatus({ status: 'ready' })
      } catch (err) {
        if (!cancelled && mountedRef.current) {
          setStatus({ status: 'error', error: err instanceof Error ? err : new Error(String(err)) })
        }
      }
    }

    create()

    return () => {
      cancelled = true
      mountedRef.current = false
    }
  }, [existingPtyId, cwd, sandbox?.enabled, startupCommand, sessionStore])

  if (existingPtyId) {
    return { status: 'ready' }
  }

  return status
}
