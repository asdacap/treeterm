import type { ExecApi } from '../types'

export async function resolveHomedir(exec: ExecApi, connectionId: string): Promise<string> {
  const startResult = await exec.start(connectionId, '/', 'sh', ['-c', 'echo $HOME'])
  if (!startResult.success) throw new Error(startResult.error)
  const { execId } = startResult

  return new Promise((resolve, reject) => {
    const stdout: string[] = []

    const unsub = exec.onEvent(execId, (event) => {
      if (event.type === 'stdout') {
        stdout.push(event.data)
      } else if (event.type === 'exit') {
        unsub()
        const home = stdout.join('').trim()
        if (event.exitCode === 0 && home) resolve(home)
        else reject(new Error('Failed to resolve remote home directory'))
      } else if (event.type === 'error') {
        unsub()
        reject(new Error(event.message))
      }
    })
  })
}
