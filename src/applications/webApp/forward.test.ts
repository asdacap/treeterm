import { describe, it, expect, vi } from 'vitest'
import { ensureForward } from './forward'
import type { EnsureForwardOptions } from './forward'
import type { PortForwardConfig, PortForwardInfo, SSHApi, WebAppPort } from '../../renderer/types'
import { WebAppPortStatus } from '../../renderer/types'
import { PortForwardStatus } from '../../shared/types'

const ID = 'webapp-tab-1'

/** Minimal in-memory forward registry mirroring main's ConnectionManager. */
function makeSsh(statusOnAdd: (config: PortForwardConfig) => PortForwardStatus) {
  const forwards = new Map<string, PortForwardInfo>()
  const ssh = {
    addPortForward: vi.fn((config: PortForwardConfig) => {
      const info = { ...config, status: statusOnAdd(config), error: 'bind failed' } as PortForwardInfo
      forwards.set(config.id, info)
      return Promise.resolve(info)
    }),
    removePortForward: vi.fn((id: string) => { forwards.delete(id); return Promise.resolve() }),
    listPortForwards: vi.fn(() => Promise.resolve(Array.from(forwards.values()))),
  }
  return { ssh, forwards }
}

function existing(status: PortForwardStatus, localPort = 26000): PortForwardInfo {
  return { id: ID, connectionId: 'c', localPort, remoteHost: 'localhost', remotePort: 31000, persist: false, status } as PortForwardInfo
}

function opts(ssh: Partial<SSHApi>, preferredLocalPort: WebAppPort = { status: WebAppPortStatus.Unassigned }): EnsureForwardOptions {
  return {
    ssh: ssh as SSHApi,
    sleep: vi.fn(() => Promise.resolve()),
    random: () => 0,
    connectionId: 'c',
    forwardId: ID,
    remotePort: 31000,
    preferredLocalPort,
  }
}

describe('ensureForward', () => {
  it('adds a non-persisted forward to localhost on the remote side', async () => {
    const { ssh } = makeSsh(() => PortForwardStatus.Active)
    expect(await ensureForward(opts(ssh))).toBe(20000)
    expect(ssh.addPortForward).toHaveBeenCalledWith({
      id: ID, connectionId: 'c', localPort: 20000, remoteHost: 'localhost', remotePort: 31000, persist: false,
    })
  })

  it('reuses an active forward', async () => {
    const { ssh, forwards } = makeSsh(() => PortForwardStatus.Active)
    forwards.set(ID, existing(PortForwardStatus.Active, 25000))
    expect(await ensureForward(opts(ssh))).toBe(25000)
    expect(ssh.addPortForward).not.toHaveBeenCalled()
  })

  it('waits out Connecting, then replaces a dead forward on the preferred port', async () => {
    const { ssh, forwards } = makeSsh(() => PortForwardStatus.Active)
    forwards.set(ID, existing(PortForwardStatus.Connecting))
    const o = opts(ssh, { status: WebAppPortStatus.Assigned, port: 26000 })
    vi.mocked(o.sleep).mockImplementation(() => { forwards.set(ID, existing(PortForwardStatus.Stopped)); return Promise.resolve() })
    expect(await ensureForward(o)).toBe(26000)
    expect(ssh.removePortForward).toHaveBeenCalledWith(ID)
  })

  it('retries on a random port when the preferred one fails', async () => {
    const { ssh } = makeSsh((c) => c.localPort === 26000 ? PortForwardStatus.Error : PortForwardStatus.Active)
    expect(await ensureForward(opts(ssh, { status: WebAppPortStatus.Assigned, port: 26000 }))).toBe(20000)
    expect(ssh.addPortForward).toHaveBeenCalledTimes(2)
  })

  it('gives up after repeated failures', async () => {
    const { ssh } = makeSsh(() => PortForwardStatus.Error)
    await expect(ensureForward(opts(ssh))).rejects.toThrow('Port forward failed after 5 attempts')
    expect(ssh.addPortForward).toHaveBeenCalledTimes(5)
  })
})
