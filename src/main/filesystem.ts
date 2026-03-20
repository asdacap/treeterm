import type { IpcServer } from './ipc/ipc-server'
import type { GrpcDaemonClient } from './grpcClient'

export function registerFilesystemHandlers(server: IpcServer, daemonClient: GrpcDaemonClient): void {
  server.onFsReadDirectory(async (workspacePath, dirPath) => {
    try {
      return await daemonClient.readDirectory(workspacePath, dirPath)
    } catch (error) {
      return { success: false, error: String(error) }
    }
  })

  server.onFsReadFile(async (workspacePath, filePath) => {
    try {
      return await daemonClient.readFile(workspacePath, filePath)
    } catch (error) {
      return { success: false, error: String(error) }
    }
  })

  server.onFsWriteFile(async (workspacePath, filePath, content) => {
    try {
      return await daemonClient.writeFile(workspacePath, filePath, content)
    } catch (error) {
      return { success: false, error: String(error) }
    }
  })
}
