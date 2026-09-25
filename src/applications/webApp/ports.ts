import type { ExecApi } from '../../renderer/types'
import { DEFAULT_EXEC_TIMEOUT_MS, ExecEventType } from '../../shared/ipc-types'

/** Runs curl against a port on the daemon's host and returns curl's exit code. */
export type HttpProbe = (port: number) => Promise<number>

// curl exit codes we act on
const CURL_OK = 0
const CURL_COULDNT_CONNECT = 7

const PORT_RANGE_START = 20000
const PORT_RANGE_SIZE = 25000
const FIND_PORT_ATTEMPTS = 20
const POLL_INTERVAL_MS = 1000

export enum WaitOutcome {
  Ready = 'ready',
  Cancelled = 'cancelled',
}

export function randomPort(random: () => number): number {
  return PORT_RANGE_START + Math.floor(random() * PORT_RANGE_SIZE)
}

/**
 * Probes through the daemon (ExecStream), so the check runs on the same host as
 * the web app — local or remote alike. A missing curl surfaces as an exec Error.
 */
export function createHttpProbe(exec: ExecApi, connectionId: string): HttpProbe {
  return async (port: number): Promise<number> => {
    const startResult = await exec.start(connectionId, '/', 'curl', [
      '-s', '-o', '/dev/null', '--connect-timeout', '1', '--max-time', '2', `http://localhost:${String(port)}/`,
    ], DEFAULT_EXEC_TIMEOUT_MS)
    if (!startResult.success) throw new Error(startResult.error)
    const { execId } = startResult
    return new Promise((resolve, reject) => {
      const unsub = exec.onEvent(execId, (event) => {
        if (event.type === ExecEventType.Exit) {
          unsub()
          resolve(event.exitCode)
        } else if (event.type === ExecEventType.Error) {
          unsub()
          reject(new Error(event.message))
        }
      })
    })
  }
}

/** A port is free when nothing accepts a connection on it. */
export async function findFreePort(probe: HttpProbe, random: () => number): Promise<number> {
  for (let attempt = 0; attempt < FIND_PORT_ATTEMPTS; attempt++) {
    const port = randomPort(random)
    const exitCode = await probe(port)
    if (exitCode === CURL_COULDNT_CONNECT) return port
  }
  throw new Error(`No free port found after ${String(FIND_PORT_ATTEMPTS)} attempts`)
}

/** Polls until the server answers with any HTTP response (curl exits 0 regardless of status code). */
export async function waitForHttp(
  probe: HttpProbe,
  port: number,
  isCancelled: () => boolean,
  sleep: (ms: number) => Promise<void>,
): Promise<WaitOutcome> {
  for (;;) {
    if (isCancelled()) return WaitOutcome.Cancelled
    const exitCode = await probe(port)
    if (exitCode === CURL_OK) return WaitOutcome.Ready
    await sleep(POLL_INTERVAL_MS)
  }
}
