import { describe, it, expect, vi } from 'vitest'
import { createRunActionsStore, type RunActionsDeps } from './createRunActionsStore'
import type { RunAction } from '../types'

function makeMockDeps(overrides?: Partial<RunActionsDeps>): RunActionsDeps {
  return {
    detect: vi.fn().mockResolvedValue([]),
    run: vi.fn().mockResolvedValue({ success: true, data: { ptyId: 'pty-1' } }),
    ...overrides,
  }
}

describe('createRunActionsStore', () => {
  it('auto-detects on creation', async () => {
    const actions: RunAction[] = [{ id: 'test', name: 'Test', source: 'npm', description: 'Run tests' }]
    const deps = makeMockDeps({ detect: vi.fn().mockResolvedValue(actions) })
    const store = createRunActionsStore('/workspace', deps)

    // Wait for auto-detect
    await vi.waitFor(() => {
      expect(store.getState().actions).toEqual(actions)
    })
    expect(deps.detect).toHaveBeenCalledWith('/workspace')
  })

  it('detect sets detecting true then false', async () => {
    let resolveDetect: (v: RunAction[]) => void
    const detectPromise = new Promise<RunAction[]>((r) => { resolveDetect = r })
    const deps = makeMockDeps({ detect: vi.fn().mockReturnValue(detectPromise) })
    const store = createRunActionsStore('/workspace', deps)

    // Auto-detect is pending
    expect(store.getState().detecting).toBe(true)

    resolveDetect!([] as RunAction[])
    await vi.waitFor(() => {
      expect(store.getState().detecting).toBe(false)
    })
  })

  it('detect resets detecting on error', async () => {
    const mockDetect = vi.fn()
      .mockResolvedValueOnce([]) // auto-detect succeeds
      .mockRejectedValueOnce(new Error('fail')) // explicit call fails
    const deps = makeMockDeps({ detect: mockDetect })
    const store = createRunActionsStore('/workspace', deps)

    await vi.waitFor(() => {
      expect(store.getState().detecting).toBe(false)
    })

    await expect(store.getState().detect()).rejects.toThrow('fail')
    expect(store.getState().detecting).toBe(false)
  })

  it('run delegates to deps.run', async () => {
    const deps = makeMockDeps()
    const store = createRunActionsStore('/workspace', deps)

    await vi.waitFor(() => { expect(store.getState().detecting).toBe(false) })

    const result = await store.getState().run('build')
    expect(deps.run).toHaveBeenCalledWith('/workspace', 'build')
    expect(result).toEqual({ success: true, data: { ptyId: 'pty-1' } })
  })
})
