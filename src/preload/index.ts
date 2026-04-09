import { contextBridge } from 'electron'
import type { SandboxConfig, Session, TTYSessionInfo, WorkspaceInput, Settings, SSHConnectionConfig, ConnectionInfo, PortForwardConfig, PortForwardInfo } from '../shared/types'
import type { PtyEvent, ExecEvent } from '../shared/ipc-types'
import { IpcClient } from './ipc-client'
import type { PreloadApi, Platform } from '../renderer/types'

type PtyEventCallback = (event: PtyEvent) => void
const ptyEventListeners = new Map<string, PtyEventCallback[]>()

type ExecEventCallback = (event: ExecEvent) => void
const execEventListeners = new Map<string, ExecEventCallback[]>()

// Initialize IPC client
const client = new IpcClient()

// Listen for pty events from main process (unified stream)
client.onPtyEvent((handle, event) => {
  const listeners = ptyEventListeners.get(handle)
  if (listeners) {
    listeners.forEach((cb) => { cb(event); })
  }
  if (event.type === 'end') {
    ptyEventListeners.delete(handle)
  }
})

// Listen for exec events from main process
client.onExecEvent((execId, event) => {
  const listeners = execEventListeners.get(execId)
  if (listeners) {
    listeners.forEach((cb) => { cb(event); })
  }
  if (event.type === 'exit' || event.type === 'error') {
    execEventListeners.delete(execId)
  }
})

type SettingsOpenCallback = () => void
const settingsOpenListeners: SettingsOpenCallback[] = []

client.onSettingsOpen(() => {
  settingsOpenListeners.forEach((cb) => { cb(); })
})

type CloseConfirmCallback = () => void
const closeConfirmListeners: CloseConfirmCallback[] = []

client.onAppConfirmClose(() => {
  closeConfirmListeners.forEach((cb) => { cb(); })
})

type CapsLockCallback = (event: { type: string; key: string; code: string }) => void
const capsLockListeners: CapsLockCallback[] = []

client.onCapsLockEvent((event) => {
  capsLockListeners.forEach((cb) => { cb(event); })
})

type ReadyCallback = (session: Session | null) => void
const readyListeners: ReadyCallback[] = []
let isReady = false
let initialSession: Session | null = null

client.onAppReady((session) => {
  isReady = true
  initialSession = session
  readyListeners.forEach((cb) => { cb(session); })
})

type SessionsCallback = (sessions: TTYSessionInfo[]) => void
const daemonSessionsListeners: SessionsCallback[] = []

client.onDaemonSessions((sessions) => {
  daemonSessionsListeners.forEach((cb) => { cb(sessions); })
})


type SessionSyncCallback = (connectionId: string, session: Session) => void
const sessionSyncListeners: SessionSyncCallback[] = []

client.onSessionSync((connectionId, session) => {
  sessionSyncListeners.forEach((cb) => { cb(connectionId, session); })
})

type SshAutoConnectedCallback = (session: Session, connection: ConnectionInfo) => void
const sshAutoConnectedListeners: SshAutoConnectedCallback[] = []

client.onSshAutoConnected((session, connection) => {
  sshAutoConnectedListeners.forEach((cb) => { cb(session, connection); })
})

type ConnectionReconnectedCallback = (session: Session, connection: ConnectionInfo) => void
const connectionReconnectedListeners: ConnectionReconnectedCallback[] = []

client.onConnectionReconnected((session, connection) => {
  connectionReconnectedListeners.forEach((cb) => { cb(session, connection); })
})

type DaemonDisconnectedCallback = () => void
const daemonDisconnectedListeners: DaemonDisconnectedCallback[] = []

client.onDaemonDisconnected(() => {
  daemonDisconnectedListeners.forEach((cb) => { cb(); })
})

type ActiveProcessesOpenCallback = () => void
const activeProcessesOpenListeners: ActiveProcessesOpenCallback[] = []

client.onActiveProcessesOpen(() => {
  activeProcessesOpenListeners.forEach((cb) => { cb(); })
})

type SshConnectionStatusCallback = (info: ConnectionInfo) => void
const sshConnectionStatusListeners: SshConnectionStatusCallback[] = []

client.onSshConnectionStatus((info) => {
  sshConnectionStatusListeners.forEach((cb) => { cb(info); })
})

type SshOutputCallback = (connectionId: string, line: string) => void
const sshBootstrapOutputListeners: SshOutputCallback[] = []
const sshTunnelOutputListeners: SshOutputCallback[] = []
const sshDaemonOutputListeners: SshOutputCallback[] = []

client.onSshBootstrapOutput((connectionId, line) => {
  sshBootstrapOutputListeners.forEach((cb) => { cb(connectionId, line); })
})
client.onSshTunnelOutput((connectionId, line) => {
  sshTunnelOutputListeners.forEach((cb) => { cb(connectionId, line); })
})
client.onSshDaemonOutput((connectionId, line) => {
  sshDaemonOutputListeners.forEach((cb) => { cb(connectionId, line); })
})

type SshOutputWatchCallback = (line: string) => void
const sshBootstrapWatchCallbacks = new Map<string, SshOutputWatchCallback>()
const sshTunnelWatchCallbacks = new Map<string, SshOutputWatchCallback>()
const sshDaemonWatchCallbacks = new Map<string, SshOutputWatchCallback>()

client.onSshBootstrapOutput((connectionId, line) => {
  sshBootstrapWatchCallbacks.get(connectionId)?.(line)
})
client.onSshTunnelOutput((connectionId, line) => {
  sshTunnelWatchCallbacks.get(connectionId)?.(line)
})
client.onSshDaemonOutput((connectionId, line) => {
  sshDaemonWatchCallbacks.get(connectionId)?.(line)
})

type SshStatusWatchCallback = (info: ConnectionInfo) => void
const sshStatusWatchCallbacks = new Map<string, SshStatusWatchCallback>()

client.onSshConnectionStatus((info) => {
  const cb = sshStatusWatchCallbacks.get(info.id)
  if (cb) cb(info)
})

type PortForwardStatusCallback = (info: PortForwardInfo) => void
const portForwardStatusListeners: PortForwardStatusCallback[] = []

client.onSshPortForwardStatus((info) => {
  portForwardStatusListeners.forEach((cb) => { cb(info); })
})

type PortForwardOutputWatchCallback = (line: string) => void
const portForwardOutputWatchCallbacks = new Map<string, PortForwardOutputWatchCallback>()

client.onSshPortForwardOutput((portForwardId, line) => {
  const cb = portForwardOutputWatchCallbacks.get(portForwardId)
  if (cb) cb(line)
})

const preloadApi: PreloadApi = {
  platform: process.platform as Platform,
  terminal: {
    create: (connectionId: string, handle: string, cwd: string, sandbox?: SandboxConfig, startupCommand?: string) => {
      return client.ptyCreate(connectionId, handle, cwd, sandbox, startupCommand)
    },
    attach: (connectionId: string, handle: string, sessionId: string) => {
      return client.ptyAttach(connectionId, handle, sessionId)
    },
    list: (connectionId: string): Promise<TTYSessionInfo[]> => {
      return client.ptyList(connectionId)
    },
    write: (id: string, data: string): void => {
      client.ptyWrite(id, data)
    },
    resize: (id: string, cols: number, rows: number): void => {
      client.ptyResize(id, cols, rows)
    },
    kill: (connectionId: string, id: string): void => {
      client.ptyKill(connectionId, id)
    },
    onEvent: (id: string, callback: PtyEventCallback): (() => void) => {
      if (!ptyEventListeners.has(id)) {
        ptyEventListeners.set(id, [])
      }
      ptyEventListeners.get(id)?.push(callback)

      return () => {
        const listeners = ptyEventListeners.get(id)
        if (listeners) {
          const index = listeners.indexOf(callback)
          if (index > -1) listeners.splice(index, 1)
        }
      }
    },
    onActiveProcessesOpen: (callback: ActiveProcessesOpenCallback): (() => void) => {
      activeProcessesOpenListeners.push(callback)
      return () => {
        const index = activeProcessesOpenListeners.indexOf(callback)
        if (index > -1) activeProcessesOpenListeners.splice(index, 1)
      }
    },
    createSession: (connectionId: string, cwd: string, startupCommand?: string) => {
      return client.ptyCreateSession(connectionId, cwd, startupCommand)
    }
  },
  selectFolder: (): Promise<string | null> => {
    return client.dialogSelectFolder()
  },
  getRecentDirectories: (): Promise<string[]> => {
    return client.dialogGetRecentDirectories()
  },
  settings: {
    load: () => {
      return client.settingsLoad()
    },
    save: (settings: Settings) => {
      return client.settingsSave(settings)
    },
    onOpen: (callback: SettingsOpenCallback): (() => void) => {
      settingsOpenListeners.push(callback)
      return () => {
        const index = settingsOpenListeners.indexOf(callback)
        if (index > -1) {
          settingsOpenListeners.splice(index, 1)
        }
      }
    }
  },
  filesystem: {
    readDirectory: (connectionId: string, workspacePath: string, dirPath: string) => {
      return client.fsReadDirectory(connectionId, workspacePath, dirPath)
    },
    readFile: (connectionId: string, workspacePath: string, filePath: string) => {
      return client.fsReadFile(connectionId, workspacePath, filePath)
    },
    writeFile: (connectionId: string, workspacePath: string, filePath: string, content: string) => {
      return client.fsWriteFile(connectionId, workspacePath, filePath, content)
    },
    searchFiles: (connectionId: string, workspacePath: string, query: string) => {
      return client.fsSearchFiles(connectionId, workspacePath, query)
    }
  },
  exec: {
    start: (connectionId: string, cwd: string, command: string, args: string[]) => {
      return client.execStart(connectionId, cwd, command, args)
    },
    kill: (execId: string): void => {
      client.execKill(execId)
    },
    onEvent: (execId: string, callback: ExecEventCallback): (() => void) => {
      if (!execEventListeners.has(execId)) {
        execEventListeners.set(execId, [])
      }
      execEventListeners.get(execId)?.push(callback)

      return () => {
        const listeners = execEventListeners.get(execId)
        if (listeners) {
          const index = listeners.indexOf(callback)
          if (index > -1) listeners.splice(index, 1)
        }
      }
    }
  },
  sandbox: {
    isAvailable: (): Promise<boolean> => {
      return client.sandboxIsAvailable()
    }
  },
  getInitialWorkspace: (): Promise<string | null> => {
    return client.appGetInitialWorkspace()
  },
  app: {
    onReady: (callback: ReadyCallback): (() => void) => {
      if (isReady) {
        // Already ready, call immediately with the initial session
        callback(initialSession)
      } else {
        readyListeners.push(callback)
      }
      return () => {
        const index = readyListeners.indexOf(callback)
        if (index > -1) {
          readyListeners.splice(index, 1)
        }
      }
    },
    onCloseConfirm: (callback: CloseConfirmCallback): (() => void) => {
      closeConfirmListeners.push(callback)
      return () => {
        const index = closeConfirmListeners.indexOf(callback)
        if (index > -1) {
          closeConfirmListeners.splice(index, 1)
        }
      }
    },
    confirmClose: (): void => {
      client.appCloseConfirmed()
    },
    cancelClose: (): void => {
      client.appCloseCancelled()
    },
    onCapsLockEvent: (callback: CapsLockCallback): (() => void) => {
      capsLockListeners.push(callback)
      return () => {
        const index = capsLockListeners.indexOf(callback)
        if (index > -1) {
          capsLockListeners.splice(index, 1)
        }
      }
    },
    onSshAutoConnected: (callback: SshAutoConnectedCallback): (() => void) => {
      sshAutoConnectedListeners.push(callback)
      return () => {
        const index = sshAutoConnectedListeners.indexOf(callback)
        if (index > -1) {
          sshAutoConnectedListeners.splice(index, 1)
        }
      }
    },
    onConnectionReconnected: (callback: ConnectionReconnectedCallback): (() => void) => {
      connectionReconnectedListeners.push(callback)
      return () => {
        const index = connectionReconnectedListeners.indexOf(callback)
        if (index > -1) {
          connectionReconnectedListeners.splice(index, 1)
        }
      }
    }
  },
  daemon: {
    shutdown: (connectionId: string) => {
      return client.daemonShutdown(connectionId)
    },
    onSessions: (callback: SessionsCallback): (() => void) => {
      daemonSessionsListeners.push(callback)
      return () => {
        const index = daemonSessionsListeners.indexOf(callback)
        if (index > -1) {
          daemonSessionsListeners.splice(index, 1)
        }
      }
    },
    onDisconnected: (callback: DaemonDisconnectedCallback): (() => void) => {
      daemonDisconnectedListeners.push(callback)
      return () => {
        const index = daemonDisconnectedListeners.indexOf(callback)
        if (index > -1) {
          daemonDisconnectedListeners.splice(index, 1)
        }
      }
    }
  },
  session: {
    update: (sessionId: string, workspaces: WorkspaceInput[], senderUuid?: string, expectedVersion?: number) => {
      return client.sessionUpdate(sessionId, workspaces, senderUuid, expectedVersion)
    },
    lock: (sessionId: string, ttlMs?: number) => {
      return client.sessionLock(sessionId, ttlMs)
    },
    unlock: (sessionId: string) => {
      return client.sessionUnlock(sessionId)
    },
    forceUnlock: (sessionId: string) => {
      return client.sessionForceUnlock(sessionId)
    },
    onSync: (callback: SessionSyncCallback): (() => void) => {
      sessionSyncListeners.push(callback)
      return () => {
        const index = sessionSyncListeners.indexOf(callback)
        if (index > -1) sessionSyncListeners.splice(index, 1)
      }
    }
  },
  clipboard: {
    writeText: (text: string): void => { client.clipboardWriteText(text) },
    readText: (): Promise<string> => client.clipboardReadText(),
  },
  getWindowUuid: (): Promise<string> => {
    return client.appGetWindowUuid()
  },
  ssh: {
    connect: (config: SSHConnectionConfig, options?: { refreshDaemon?: boolean; allowOutdatedDaemon?: boolean }) => {
      return client.sshConnect(config, options)
    },
    disconnect: (connectionId: string): Promise<void> => {
      return client.sshDisconnect(connectionId)
    },
    reconnect: (connectionId: string): Promise<void> => {
      return client.sshReconnect(connectionId)
    },
    reconnectNow: (connectionId: string): Promise<void> => {
      return client.sshReconnectNow(connectionId)
    },
    forceReconnect: (connectionId: string): Promise<void> => {
      return client.sshForceReconnect(connectionId)
    },
    cancelReconnect: (connectionId: string): Promise<void> => {
      return client.sshCancelReconnect(connectionId)
    },
    listConnections: (): Promise<ConnectionInfo[]> => {
      return client.sshListConnections()
    },
    saveConnection: (config: SSHConnectionConfig): Promise<void> => {
      return client.sshSaveConnection(config)
    },
    getSavedConnections: (): Promise<SSHConnectionConfig[]> => {
      return client.sshGetSavedConnections()
    },
    removeSavedConnection: (id: string): Promise<void> => {
      return client.sshRemoveSavedConnection(id)
    },
    onConnectionStatus: (callback: SshConnectionStatusCallback): (() => void) => {
      sshConnectionStatusListeners.push(callback)
      return () => {
        const index = sshConnectionStatusListeners.indexOf(callback)
        if (index > -1) sshConnectionStatusListeners.splice(index, 1)
      }
    },
    onBootstrapOutput: (callback: SshOutputCallback): (() => void) => {
      sshBootstrapOutputListeners.push(callback)
      return () => {
        const index = sshBootstrapOutputListeners.indexOf(callback)
        if (index > -1) sshBootstrapOutputListeners.splice(index, 1)
      }
    },
    onTunnelOutput: (callback: SshOutputCallback): (() => void) => {
      sshTunnelOutputListeners.push(callback)
      return () => {
        const index = sshTunnelOutputListeners.indexOf(callback)
        if (index > -1) sshTunnelOutputListeners.splice(index, 1)
      }
    },
    onDaemonOutput: (callback: SshOutputCallback): (() => void) => {
      sshDaemonOutputListeners.push(callback)
      return () => {
        const index = sshDaemonOutputListeners.indexOf(callback)
        if (index > -1) sshDaemonOutputListeners.splice(index, 1)
      }
    },
    watchBootstrapOutput: async (connectionId: string, cb: (line: string) => void): Promise<{ scrollback: string[], unsubscribe: () => void }> => {
      sshBootstrapWatchCallbacks.set(connectionId, cb)
      const result = await client.sshWatchBootstrapOutput(connectionId)
      return {
        scrollback: result.scrollback,
        unsubscribe: () => {
          sshBootstrapWatchCallbacks.delete(connectionId)
          client.sshUnwatchBootstrapOutput(connectionId).catch(() => {})
        }
      }
    },
    watchTunnelOutput: async (connectionId: string, cb: (line: string) => void): Promise<{ scrollback: string[], unsubscribe: () => void }> => {
      sshTunnelWatchCallbacks.set(connectionId, cb)
      const result = await client.sshWatchTunnelOutput(connectionId)
      return {
        scrollback: result.scrollback,
        unsubscribe: () => {
          sshTunnelWatchCallbacks.delete(connectionId)
          client.sshUnwatchTunnelOutput(connectionId).catch(() => {})
        }
      }
    },
    watchDaemonOutput: async (connectionId: string, cb: (line: string) => void): Promise<{ scrollback: string[], unsubscribe: () => void }> => {
      sshDaemonWatchCallbacks.set(connectionId, cb)
      const result = await client.sshWatchDaemonOutput(connectionId)
      return {
        scrollback: result.scrollback,
        unsubscribe: () => {
          sshDaemonWatchCallbacks.delete(connectionId)
          client.sshUnwatchDaemonOutput(connectionId).catch(() => {})
        }
      }
    },
    watchConnectionStatus: async (connectionId: string, cb: (info: ConnectionInfo) => void): Promise<{ initial: ConnectionInfo, unsubscribe: () => void }> => {
      sshStatusWatchCallbacks.set(connectionId, cb)
      const result = await client.sshWatchConnectionStatus(connectionId)
      return {
        initial: result.initial,
        unsubscribe: () => {
          sshStatusWatchCallbacks.delete(connectionId)
          client.sshUnwatchConnectionStatus(connectionId).catch(() => {})
        }
      }
    },
    addPortForward: (config: PortForwardConfig): Promise<PortForwardInfo> => {
      return client.sshAddPortForward(config)
    },
    removePortForward: (portForwardId: string): Promise<void> => {
      return client.sshRemovePortForward(portForwardId)
    },
    listPortForwards: (connectionId: string): Promise<PortForwardInfo[]> => {
      return client.sshListPortForwards(connectionId)
    },
    onPortForwardStatus: (callback: PortForwardStatusCallback): (() => void) => {
      portForwardStatusListeners.push(callback)
      return () => {
        const index = portForwardStatusListeners.indexOf(callback)
        if (index > -1) portForwardStatusListeners.splice(index, 1)
      }
    },
    watchPortForwardOutput: async (portForwardId: string, cb: (line: string) => void): Promise<{ scrollback: string[], unsubscribe: () => void }> => {
      portForwardOutputWatchCallbacks.set(portForwardId, cb)
      const result = await client.sshWatchPortForwardOutput(portForwardId)
      return {
        scrollback: result.scrollback,
        unsubscribe: () => {
          portForwardOutputWatchCallbacks.delete(portForwardId)
          client.sshUnwatchPortForwardOutput(portForwardId).catch(() => {})
        }
      }
    }
  }
}

contextBridge.exposeInMainWorld('electron', preloadApi)
