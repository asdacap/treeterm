/**
 * Socket path utility - shared between daemon and main process
 */

import * as crypto from 'crypto'
import * as os from 'os'
import * as path from 'path'

export function getDefaultSocketPath(): string {
  const uid = process.getuid ? process.getuid() : os.userInfo().uid
  return path.join(os.tmpdir(), `treeterm-${String(uid)}`, 'daemon.sock')
}

export function getRemoteForwardSocketPath(connectionId: string): string {
  const uid = process.getuid ? process.getuid() : os.userInfo().uid
  const hash = crypto.createHash('sha256').update(connectionId).digest('hex').substring(0, 12)
  const unique = crypto.randomUUID().substring(0, 8)
  return path.join('/tmp', `treeterm-${String(uid)}`, `r-${hash}-${unique}.sock`)
}
