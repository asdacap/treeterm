import { describe, it, expect, vi } from 'vitest'
import { createHttpProbe, findFreePort, randomPort, waitForHttp, WaitOutcome } from './ports'
import type { HttpProbe } from './ports'
import type { ExecApi, ExecEvent } from '../../renderer/types'
import { DEFAULT_EXEC_TIMEOUT_MS, ExecEventType } from '../../shared/ipc-types'

function makeExec(events: ExecEvent[]): ExecApi {
  return {
    start: vi.fn<ExecApi['start']>().mockResolvedValue({ success: true, execId: 'exec-1' }),
    kill: vi.fn(),
    onEvent: vi.fn<ExecApi['onEvent']>((_id, cb) => {
      queueMicrotask(() => { for (const e of events) cb(e) })
      return vi.fn()
    }),
  }
}

function probeReturning(...codes: number[]): HttpProbe {
  const fn = vi.fn<HttpProbe>()
  for (const code of codes) fn.mockResolvedValueOnce(code)
  return fn
}

describe('randomPort', () => {
  it('maps the random value into the port range', () => {
    expect(randomPort(() => 0)).toBe(20000)
    expect(randomPort(() => 0.999999)).toBe(44999)
  })
})

describe('createHttpProbe', () => {
  it('runs curl against localhost on the connection and returns its exit code', async () => {
    const exec = makeExec([
      { type: ExecEventType.Stderr, data: 'noise' },
      { type: ExecEventType.Exit, exitCode: 7 },
    ])
    const code = await createHttpProbe(exec, 'conn-1')(3000)
    expect(code).toBe(7)
    expect(exec.start).toHaveBeenCalledWith('conn-1', '/', 'curl', expect.arrayContaining(['http://localhost:3000/']), DEFAULT_EXEC_TIMEOUT_MS)
  })

  it('rejects when the exec fails to start', async () => {
    const exec = makeExec([])
    vi.mocked(exec.start).mockResolvedValue({ success: false, error: 'no daemon' })
    await expect(createHttpProbe(exec, 'c')(1)).rejects.toThrow('no daemon')
  })

  it('rejects on an exec error event (e.g. curl missing)', async () => {
    const exec = makeExec([{ type: ExecEventType.Error, message: 'No such file' }])
    await expect(createHttpProbe(exec, 'c')(1)).rejects.toThrow('No such file')
  })
})

describe('findFreePort', () => {
  it('returns the first port nothing listens on', async () => {
    const probe = probeReturning(0, 52, 7)
    const random = vi.fn().mockReturnValueOnce(0).mockReturnValueOnce(0.5).mockReturnValueOnce(0.2)
    expect(await findFreePort(probe, random)).toBe(randomPort(() => 0.2))
    expect(probe).toHaveBeenCalledTimes(3)
  })

  it('throws after running out of attempts', async () => {
    const probe = vi.fn<HttpProbe>().mockResolvedValue(0)
    await expect(findFreePort(probe, () => 0)).rejects.toThrow('No free port found after 20 attempts')
  })
})

describe('waitForHttp', () => {
  it('polls until the server answers', async () => {
    const probe = probeReturning(7, 28, 0)
    const sleep = vi.fn().mockResolvedValue(undefined)
    expect(await waitForHttp(probe, 3000, () => false, sleep)).toBe(WaitOutcome.Ready)
    expect(sleep).toHaveBeenCalledTimes(2)
  })

  it('stops when cancelled', async () => {
    const probe = vi.fn<HttpProbe>().mockResolvedValue(7)
    let cancelled = false
    const sleep = vi.fn(() => { cancelled = true; return Promise.resolve() })
    expect(await waitForHttp(probe, 3000, () => cancelled, sleep)).toBe(WaitOutcome.Cancelled)
    expect(probe).toHaveBeenCalledTimes(1)
  })
})
