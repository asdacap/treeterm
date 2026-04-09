// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { renderHook, waitFor } from '@testing-library/react'
import React from 'react'
import { createStore } from 'zustand/vanilla'
import { SessionStoreContext } from '../contexts/SessionStoreContext'
import { useTtyCreation } from './useTtyConnection'
import type { SessionState } from '../store/createSessionStore'
import { TtyCreationStatus } from '../types'

function makeSessionStore(overrides: Partial<SessionState> = {}) {
  return createStore<SessionState>()(() => ({
    createTty: vi.fn().mockResolvedValue('pty-new'),
    killTty: vi.fn(),
    ...overrides,
  }) as unknown as SessionState)
}

function makeWrapper(sessionStore: ReturnType<typeof makeSessionStore>) {
  return function Wrapper({ children }: { children: React.ReactNode }) {
    return (
      <SessionStoreContext.Provider value={sessionStore}>
        {children}
      </SessionStoreContext.Provider>
    )
  }
}

describe('useTtyCreation', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('returns ready immediately when existingPtyId is provided', () => {
    const sessionStore = makeSessionStore()
    const onCreated = vi.fn()

    const { result } = renderHook(
      () => useTtyCreation('pty-existing', '/test', undefined, undefined, onCreated),
      { wrapper: makeWrapper(sessionStore) }
    )

    expect(result.current).toEqual({ status: TtyCreationStatus.Ready })
    expect(sessionStore.getState().createTty).not.toHaveBeenCalled()
    expect(onCreated).not.toHaveBeenCalled()
  })

  it('returns loading then ready after createTty resolves', async () => {
    const sessionStore = makeSessionStore()
    const onCreated = vi.fn()

    const { result } = renderHook(
      () => useTtyCreation(null, '/test', undefined, undefined, onCreated),
      { wrapper: makeWrapper(sessionStore) }
    )

    await waitFor(() => {
      expect(result.current).toEqual({ status: TtyCreationStatus.Ready })
    })

    expect(sessionStore.getState().createTty).toHaveBeenCalledWith('/test', undefined, undefined)
    expect(onCreated).toHaveBeenCalledWith('pty-new')
  })

  it('returns error when createTty rejects', async () => {
    const sessionStore = makeSessionStore({
      createTty: vi.fn().mockRejectedValue(new Error('pty creation failed')),
    } as unknown as Partial<SessionState>)
    const onCreated = vi.fn()

    const { result } = renderHook(
      () => useTtyCreation(null, '/test', undefined, undefined, onCreated),
      { wrapper: makeWrapper(sessionStore) }
    )

    await waitFor(() => {
      expect(result.current.status).toBe(TtyCreationStatus.Error)
    })

    expect(result.current.status).toBe(TtyCreationStatus.Error)
    if (result.current.status === TtyCreationStatus.Error) {
      expect(result.current.error.message).toBe('pty creation failed')
    }
    expect(onCreated).not.toHaveBeenCalled()
  })

  it('passes sandbox and startupCommand to createTty', async () => {
    const sessionStore = makeSessionStore()
    const onCreated = vi.fn()
    const sandbox = { enabled: true, allowNetwork: false }

    renderHook(
      () => useTtyCreation(null, '/test', sandbox as unknown as import('../../shared/types').SandboxConfig, 'echo hi', onCreated),
      { wrapper: makeWrapper(sessionStore) }
    )

    await waitFor(() => {
      expect(onCreated).toHaveBeenCalled()
    })

    expect(sessionStore.getState().createTty).toHaveBeenCalledWith('/test', sandbox, 'echo hi')
  })
})
