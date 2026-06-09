import type { ExecApi } from '../types'
import { ExecEventType } from '../../shared/ipc-types'
import { withTimeout } from './withTimeout'

// Backstop only — the daemon enforces a 30s exec timeout, so this fires only if the result event
// is never delivered to the renderer.
const HOMEDIR_TIMEOUT_MS = 35000

export async function resolveHomedir(exec: ExecApi, connectionId: string): Promise<string> {
  const startResult = await exec.start(connectionId, '/', 'sh', ['-c', 'echo $HOME'])
  if (!startResult.success) throw new Error(startResult.error)
  const { execId } = startResult

  let unsub: () => void = () => undefined
  return withTimeout(new Promise<string>((resolve, reject) => {
    const stdout: string[] = []

    unsub = exec.onEvent(execId, (event) => {
      if (event.type === ExecEventType.Stdout) {
        stdout.push(event.data)
      } else if (event.type === ExecEventType.Exit) {
        unsub()
        const home = stdout.join('').trim()
        if (event.exitCode === 0 && home) resolve(home)
        else reject(new Error('Failed to resolve remote home directory'))
      } else if (event.type === ExecEventType.Error) {
        unsub()
        reject(new Error(event.message))
      }
    })
  }), HOMEDIR_TIMEOUT_MS, 'resolveHomedir', () => { unsub(); })
}
