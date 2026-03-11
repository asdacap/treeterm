/**
 * Socket path utility - shared between daemon and main process
 * This file has no dependencies on the logger to avoid initialization issues
 */

import * as os from 'os'
import * as path from 'path'

export function getDefaultSocketPath(): string {
  const uid = process.getuid ? process.getuid() : os.userInfo().uid
  return path.join(os.tmpdir(), `treeterm-${uid}`, 'daemon.sock')
}
