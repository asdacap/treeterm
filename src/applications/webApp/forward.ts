import type { SSHApi, PortForwardInfo, WebAppPort } from '../../renderer/types'
import { WebAppPortStatus } from '../../renderer/types'
import { PortForwardStatus } from '../../shared/types'
import { randomPort } from './ports'

const FORWARD_ATTEMPTS = 5
const FORWARD_POLL_MS = 500

export interface EnsureForwardOptions {
  ssh: SSHApi
  sleep: (ms: number) => Promise<void>
  random: () => number
  connectionId: string
  forwardId: string
  remotePort: number
  // Tried first so the browser origin (cookies, localStorage) survives restarts.
  preferredLocalPort: WebAppPort
}

/** Polls until the forward leaves Connecting. Undefined when it does not exist. */
async function settledForward(opts: EnsureForwardOptions): Promise<PortForwardInfo | undefined> {
  for (;;) {
    const pf = (await opts.ssh.listPortForwards(opts.connectionId)).find((p) => p.id === opts.forwardId)
    if (pf?.status !== PortForwardStatus.Connecting) return pf
    await opts.sleep(FORWARD_POLL_MS)
  }
}

/**
 * Makes sure an app-owned ssh forward to localhost:remotePort on the remote host is
 * active, reusing a live one. Returns the local port; persisting it is the caller's job.
 */
export async function ensureForward(opts: EnsureForwardOptions): Promise<number> {
  const existing = await settledForward(opts)
  if (existing?.status === PortForwardStatus.Active) return existing.localPort
  if (existing) await opts.ssh.removePortForward(opts.forwardId)

  for (let attempt = 0; attempt < FORWARD_ATTEMPTS; attempt++) {
    const localPort = attempt === 0 && opts.preferredLocalPort.status === WebAppPortStatus.Assigned
      ? opts.preferredLocalPort.port
      : randomPort(opts.random)
    await opts.ssh.addPortForward({
      id: opts.forwardId,
      connectionId: opts.connectionId,
      localPort,
      remoteHost: 'localhost',
      remotePort: opts.remotePort,
      persist: false,
    })
    const pf = await settledForward(opts)
    if (pf?.status === PortForwardStatus.Active) return localPort
    // ExitOnForwardFailure: most likely the local port is taken — try another.
    await opts.ssh.removePortForward(opts.forwardId)
  }
  throw new Error(`Port forward failed after ${String(FORWARD_ATTEMPTS)} attempts`)
}
