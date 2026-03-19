/**
 * Socket path utility - shared between daemon and main process
 * This file has no dependencies on the logger to avoid initialization issues
 */

import * as crypto from 'crypto'
import * as os from 'os'
import * as path from 'path'

export function getDefaultSocketPath(): string {
  const uid = process.getuid ? process.getuid() : os.userInfo().uid
  return path.join(os.tmpdir(), `treeterm-${uid}`, 'daemon.sock')
}

export function getRemoteForwardSocketPath(connectionId: string): string {
  const uid = process.getuid ? process.getuid() : os.userInfo().uid
  // Use /tmp directly instead of os.tmpdir() to keep the path short enough
  // for Unix domain sockets (macOS has a 104-char limit and os.tmpdir()
  // returns a long path under /var/folders/...)
  const hash = crypto.createHash('sha256').update(connectionId).digest('hex').substring(0, 12)
  return path.join('/tmp', `treeterm-${uid}`, `r-${hash}.sock`)
}
