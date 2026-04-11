import { app, BrowserWindow, clipboard, dialog, shell } from 'electron'
import { execSync } from 'child_process'
import { join } from 'path'
import { randomUUID } from 'crypto'
import { GrpcDaemonClient, PtyStream } from './grpcClient'
import { getDefaultSocketPath } from './socketPath'
import { IpcServer } from './ipc/ipc-server'
import { ConnectionManager } from './connectionManager'
import type { ExecInput, ExecOutput } from '../generated/treeterm'
import { ConnectionStatus } from '../shared/types'
import type { SSHConnectionConfig, PortForwardConfig } from '../shared/types'

// Parse initial workspace and SSH target from command line
let initialWorkspacePath: string | null = null
let initialSSHTarget: string | null = null
for (const arg of process.argv) {
  if (arg.startsWith('--workspace=')) {
    initialWorkspacePath = arg.substring('--workspace='.length)
  }
  if (arg.startsWith('--ssh=')) {
    initialSSHTarget = arg.substring('--ssh='.length)
  }
}
import { loadSettings, saveSettings, addRecentDirectory } from './settings'
import { createApplicationMenu } from './menu'
let loadingWindow: BrowserWindow | null = null
const closeConfirmedWindows: Set<number> = new Set()
let connectionManager: ConnectionManager | null = null
// Maps connectionId to a per-session GrpcDaemonClient (separate gRPC connection for lock identity)
const sessionClientMap = new Map<string, GrpcDaemonClient>()
// Maps webContentsId to window UUID for session sync deduplication
const windowUuids = new Map<number, string>()
// Maps windowUuid to the connectionId for its local gRPC client
const windowConnectionMap = new Map<string, string>()

async function createSessionClient(connectionId: string, socketPath: string): Promise<void> {
  const client = new GrpcDaemonClient(socketPath)
  await client.connect()
  sessionClientMap.set(connectionId, client)
  console.log(`[main] per-session gRPC client created for connection ${connectionId}`)
}
// Simple object storage — each entry is an independent terminal's stream.
const ptyStreams = new Map<string, PtyStream>()
// Active exec streams keyed by execId for streaming output and kill support.
const execStreams = new Map<string, ReturnType<GrpcDaemonClient['execStream']>>()
// Session watch unsubscribers per connectionId, so reconnect can re-establish watches
const sessionWatchUnsubs = new Map<string, { uuid: string; unsubscribe: () => void }[]>()
// Track previous connection status per connectionId for detecting reconnect transitions
const previousConnectionStatuses = new Map<string, ConnectionStatus>()

// Helper: get the daemon client for a given connectionId
function getClientForConnection(connId: string): GrpcDaemonClient {
  if (!connectionManager) throw new Error('ConnectionManager not initialized')
  return connectionManager.getClient(connId)
}

// Initialize IPC server
const server = new IpcServer()

function createLoadingWindow(): BrowserWindow {
  const isTest = process.env.NODE_ENV === 'test'

  loadingWindow = new BrowserWindow({
    width: 300,
    height: 200,
    frame: false,
    show: false,
    center: true,
    alwaysOnTop: true,
    resizable: false,
    movable: false,
    skipTaskbar: true,
    webPreferences: {
      preload: join(__dirname, '../preload/index.js'),
      nodeIntegration: false,
      contextIsolation: true
    },
    transparent: true,
    backgroundColor: '#00000000'
  })

  void loadingWindow.loadFile(join(__dirname, 'loading.html'))

  loadingWindow.once('ready-to-show', () => {
    if (!isTest) {
      loadingWindow?.show()
    }
  })

  return loadingWindow
}

function createWindow(): BrowserWindow {
  const window = new BrowserWindow({
    width: 1200,
    height: 800,
    minWidth: 800,
    minHeight: 600,
    show: false,
    webPreferences: {
      preload: join(__dirname, '../preload/index.js'),
      nodeIntegration: false,
      contextIsolation: true
    },
    titleBarStyle: 'hiddenInset',
    trafficLightPosition: { x: 15, y: 15 }
  })

  window.once('ready-to-show', () => {
    window.show()
  })

  // Assign a unique UUID to this window for session sync deduplication
  const windowUuid = randomUUID()
  const webContentsId = window.webContents.id
  windowUuids.set(webContentsId, windowUuid)


  // Forward all keyboard events including Caps Lock to renderer
  window.webContents.on('before-input-event', (_event, input) => {
    // Forward Caps Lock events to renderer via IPC
    if (input.code === 'CapsLock' || input.key === 'CapsLock') {
      server.capsLockEventTo(window, {
        type: input.type, // 'keyDown' or 'keyUp'
        key: input.key,
        code: input.code
      })
    }
  })

  // Reset keyboard modifier state on window focus to prevent stuck keys
  // (Chromium can lose keyUp events when window is unfocused, corrupting input state)
  window.on('focus', () => {
    for (const keyCode of ['Shift', 'Control', 'Alt', 'Meta'] as const) {
      window.webContents.sendInputEvent({ type: 'keyUp', keyCode })
    }
  })

  // Open external links in the default browser instead of within Electron
  window.webContents.on('will-navigate', (event, url) => {
    const parsedUrl = new URL(url)
    if (
      parsedUrl.protocol === 'file:' ||
      (process.env.ELECTRON_RENDERER_URL && url.startsWith(process.env.ELECTRON_RENDERER_URL))
    ) {
      return
    }
    event.preventDefault()
    void shell.openExternal(url)
  })

  window.webContents.setWindowOpenHandler(({ url }) => {
    if (url && url !== 'about:blank') {
      void shell.openExternal(url)
    }
    return { action: 'deny' }
  })

  // Intercept close event to check for unmerged workspaces
  window.on('close', (event) => {
    if (!closeConfirmedWindows.delete(webContentsId)) {
      event.preventDefault()
      window.webContents.send('app:confirm-close')
    }
  })

  // Build the load URL
  const loadUrl = process.env.ELECTRON_RENDERER_URL
    ? process.env.ELECTRON_RENDERER_URL
    : `file://${join(__dirname, '../renderer/index.html')}`

  // Load the renderer
  void window.loadURL(loadUrl)

  // Signal renderer that main is ready — renderer drives session creation via localConnect
  window.webContents.on('did-finish-load', () => {
    server.appReadyTo(window)
  })

  window.on('closed', () => {
    // Unsubscribe session watches for this window across all connections
    for (const [connId, entries] of sessionWatchUnsubs.entries()) {
      const remaining = entries.filter(e => {
        if (e.uuid === windowUuid) {
          e.unsubscribe()
          return false
        }
        return true
      })
      sessionWatchUnsubs.set(connId, remaining)
    }
    windowUuids.delete(webContentsId)

    // Disconnect this window's local gRPC client
    const connId = windowConnectionMap.get(windowUuid)
    if (connId && connectionManager) {
      connectionManager.disconnect(connId)
      windowConnectionMap.delete(windowUuid)
    }
  })

  return window
}

// Helper: register a session watch unsubscriber for reconnect tracking
function registerSessionWatch(connectionId: string, uuid: string, unsubscribe: () => void): void {
  const existing = sessionWatchUnsubs.get(connectionId) ?? []
  const filtered = existing.filter(e => e.uuid !== uuid)
  filtered.push({ uuid, unsubscribe })
  sessionWatchUnsubs.set(connectionId, filtered)
}

// Helper: re-establish session watches for a connection after reconnect
function reestablishSessionWatches(connectionId: string, client: GrpcDaemonClient): void {
  const entries = sessionWatchUnsubs.get(connectionId) ?? []

  // Unsubscribe old watches (they're dead but clean up references)
  for (const entry of entries) {
    entry.unsubscribe()
  }

  // Create new watches for each uuid
  const newEntries: { uuid: string; unsubscribe: () => void }[] = []
  for (const entry of entries) {
    const watch = client.watchSession(entry.uuid, (updatedSession) => {
      console.log(`[main] session sync received after reconnect for uuid ${entry.uuid}`, {
        sessionId: updatedSession.id,
        workspaces: updatedSession.workspaces.map(ws => ({ path: ws.path, metadata: ws.metadata })),
      })
      server.sessionSync(connectionId, updatedSession)
    })

    newEntries.push({ uuid: entry.uuid, unsubscribe: watch.unsubscribe })

    // Send the initial session data to renderer as a reconnect event
    void watch.initial.then(async (session) => {
      console.log(`[main] reconnect: session loaded for connection ${connectionId}, session ${session.id}`)
      const reconnClient = connectionManager?.getClient(connectionId)
      if (reconnClient) {
        await createSessionClient(connectionId, reconnClient.socketPath)
      }
      const connectionInfo = connectionManager?.getConnection(connectionId)
      if (connectionInfo) {
        server.connectionReconnected(session, connectionInfo)
      }
    }).catch((error: unknown) => {
      console.error(`[main] reconnect: failed to load session for connection ${connectionId}:`, error)
    })
  }

  sessionWatchUnsubs.set(connectionId, newEntries)
}

// IPC Handlers
server.onPtyCreate(async (event, connectionId, handle, cwd, sandbox, startupCommand) => {
  if (!connectionManager) throw new Error('ConnectionManager not initialized')

  try {
    const client = getClientForConnection(connectionId)
    await client.ensureDaemonRunning()
    const sessionId = await client.createPtySession({ cwd, sandbox: sandbox, startupCommand })
    const ptyStream = client.openPtyStream(handle, sessionId, (evt) => {
      if (!event.sender.isDestroyed()) {
        event.sender.send('pty:event', handle, evt)
      }
      if (evt.type === 'end' || evt.type === 'error') ptyStreams.delete(handle)
    })
    ptyStreams.set(handle, ptyStream)

    return { success: true, sessionId }
  } catch (error) {
    const errorMessage = error instanceof Error ? error.message : String(error)
    console.error('[main] failed to create PTY session via daemon:', error)
    return { success: false, error: errorMessage }
  }
})

server.onPtyAttach(async (event, connectionId, handle, sessionId) => {
  if (!connectionManager) {
    return { success: false, error: 'ConnectionManager not initialized' }
  }

  try {
    const client = getClientForConnection(connectionId)
    await client.ensureDaemonRunning()
    const ptyStream = client.openPtyStream(handle, sessionId, (evt) => {
      if (!event.sender.isDestroyed()) {
        event.sender.send('pty:event', handle, evt)
      }
      if (evt.type === 'end' || evt.type === 'error') ptyStreams.delete(handle)
    })
    ptyStreams.set(handle, ptyStream)

    return { success: true }
  } catch (error) {
    const errorMessage = error instanceof Error ? error.message : 'Unknown error'
    console.error('[main] failed to attach to PTY session:', errorMessage)
    return { success: false, error: errorMessage }
  }
})

server.onPtyList(async (connectionId) => {
  const client = getClientForConnection(connectionId)
  await client.ensureDaemonRunning()
  return client.listPtySessions()
})

server.onPtyWrite((handle, data) => {
  ptyStreams.get(handle)?.write(data)
})

server.onPtyResize((handle, cols, rows) => {
  ptyStreams.get(handle)?.resize(cols, rows)
})

server.onPtyKill((connectionId, sessionId) => {
  // Close any PtyStreams for this session
  for (const [handle, stream] of ptyStreams) {
    if (stream.sessionId === sessionId) {
      stream.close()
      ptyStreams.delete(handle)
    }
  }
  try {
    const client = getClientForConnection(connectionId)
    void client.killPtySession(sessionId).catch((error: unknown) => {
      console.error('[main] failed to kill PTY:', error)
    })
  } catch (error) {
    console.error('[main] failed to kill PTY:', error)
  }
})


server.onDaemonShutdown(async (connectionId) => {
  if (!connectionManager) {
    return { success: false, error: 'ConnectionManager not initialized' }
  }

  try {
    const client = connectionManager.getClient(connectionId)
    await client.shutdownDaemon()
    return { success: true }
  } catch (error) {
    const errorMessage = error instanceof Error ? error.message : 'Unknown error'
    console.error('[main] failed to shutdown daemon:', errorMessage)
    return { success: false, error: errorMessage }
  }
})

// Session IPC Handlers (workspace sessions)
// Session operations use per-session gRPC clients for daemon-generated lock identity.

function getSessionClient(connectionId: string): GrpcDaemonClient {
  const client = sessionClientMap.get(connectionId)
  if (!client) {
    // Fallback to connection manager client
    if (!connectionManager) throw new Error('ConnectionManager not initialized')
    return connectionManager.getClient(connectionId)
  }
  return client
}

server.onSessionUpdate(async (connectionId, workspaces, senderUuid, expectedVersion) => {
  try {
    const client = getSessionClient(connectionId)
    const result = await client.updateSession(workspaces, senderUuid, expectedVersion)
    return { success: true, session: result }
  } catch (error) {
    const errorMessage = error instanceof Error ? error.message : 'Unknown error'
    console.error('[main] failed to update session:', errorMessage)
    return { success: false, error: errorMessage }
  }
})

server.onSessionLock(async (connectionId, ttlMs) => {
  try {
    const client = getSessionClient(connectionId)
    const result = await client.lockSession(ttlMs)
    return { success: true, acquired: result.acquired, session: result.session }
  } catch (error) {
    const errorMessage = error instanceof Error ? error.message : 'Unknown error'
    console.error('[main] failed to lock session:', errorMessage)
    return { success: false, error: errorMessage }
  }
})

server.onSessionUnlock(async (connectionId) => {
  try {
    const client = getSessionClient(connectionId)
    const session = await client.unlockSession()
    return { success: true, session }
  } catch (error) {
    const errorMessage = error instanceof Error ? error.message : 'Unknown error'
    console.error('[main] failed to unlock session:', errorMessage)
    return { success: false, error: errorMessage }
  }
})

server.onSessionForceUnlock(async (connectionId) => {
  try {
    const client = getSessionClient(connectionId)
    const session = await client.forceUnlockSession()
    return { success: true, session }
  } catch (error) {
    const errorMessage = error instanceof Error ? error.message : 'Unknown error'
    console.error('[main] failed to force unlock session:', errorMessage)
    return { success: false, error: errorMessage }
  }
})

server.onClipboardWriteText((text) => { clipboard.writeText(text) })
server.onClipboardReadText(() => clipboard.readText())

server.onDialogSelectFolder(async () => {
  const focusedWindow = BrowserWindow.getFocusedWindow()
  if (!focusedWindow) return null
  const result = await dialog.showOpenDialog(focusedWindow, {
    properties: ['openDirectory']
  })
  if (result.canceled || result.filePaths.length === 0) {
    return null
  }
  const selectedPath = result.filePaths[0]
  if (!selectedPath) return null
  // Add to recent directories
  try {
    const settings = loadSettings()
    const updatedSettings = addRecentDirectory(settings, selectedPath)
    saveSettings(updatedSettings)
  } catch (error) {
    console.error('[main] failed to save recent directory:', error)
  }
  return selectedPath
})

server.onDialogGetRecentDirectories(() => {
  const settings = loadSettings()
  return settings.recentDirectories
})

// PTY create session (no stream) handler
server.onPtyCreateSession(async (connectionId, cwd, startupCommand) => {
  try {
    const client = getClientForConnection(connectionId)
    await client.ensureDaemonRunning()
    const sessionId = await client.createPtySession({ cwd, startupCommand })
    return { success: true, sessionId }
  } catch (error) {
    return { success: false, error: error instanceof Error ? error.message : String(error) }
  }
})

// Settings IPC Handlers
server.onSettingsLoad(() => {
  return loadSettings()
})

server.onSettingsSave((settings) => {
  saveSettings(settings)
  return { success: true }
})

// Filesystem IPC Handlers - All proxied to daemon
server.onFsReadDirectory((connectionId, workspacePath, dirPath) => {
  return getClientForConnection(connectionId).readDirectory(workspacePath, dirPath)
})

server.onFsReadFile((connectionId, workspacePath, filePath) => {
  return getClientForConnection(connectionId).readFile(workspacePath, filePath)
})

server.onFsWriteFile((connectionId, workspacePath, filePath, content) => {
  return getClientForConnection(connectionId).writeFile(workspacePath, filePath, content)
})

server.onFsSearchFiles((connectionId, workspacePath, query) => {
  return getClientForConnection(connectionId).searchFiles(workspacePath, query)
})

// Exec IPC Handlers
server.onExecStart((connectionId, cwd, command, args) => {
  try {
    const client = getClientForConnection(connectionId)
    const execId = randomUUID()
    const stream = client.execStream()
    execStreams.set(execId, stream)

    const startInput: ExecInput = {
      start: { cwd, command, args, env: {}, timeoutMs: 30000 }
    }
    stream.write(startInput)
    stream.end()

    stream.on('data', (output: ExecOutput) => {
      if (output.stdout) {
        server.execEvent(execId, { type: 'stdout', data: output.stdout.data.toString('utf-8') })
      } else if (output.stderr) {
        server.execEvent(execId, { type: 'stderr', data: output.stderr.data.toString('utf-8') })
      } else if (output.result) {
        server.execEvent(execId, { type: 'exit', exitCode: output.result.exitCode })
        execStreams.delete(execId)
      }
    })

    stream.on('error', (error) => {
      server.execEvent(execId, { type: 'error', message: error.message })
      execStreams.delete(execId)
    })

    stream.on('end', () => {
      execStreams.delete(execId)
    })

    return { success: true, execId }
  } catch (error) {
    return { success: false, error: error instanceof Error ? error.message : String(error) }
  }
})

server.onExecKill((execId) => {
  const stream = execStreams.get(execId)
  if (stream) {
    stream.cancel()
    execStreams.delete(execId)
  }
})

// Sandbox IPC Handlers
server.onSandboxIsAvailable(() => {
  if (process.platform === 'darwin') {
    return true // macOS always has sandbox-exec
  }
  if (process.platform === 'linux') {
    try {
      execSync('which bwrap', { stdio: 'ignore' })
      return true
    } catch {
      return false
    }
  }
  return false // Windows: no sandbox support
})

server.onAppGetInitialWorkspace(() => {
  const path = initialWorkspacePath
  initialWorkspacePath = null // Clear after first read
  return path
})

server.onAppGetWindowUuid((event) => {
  return windowUuids.get(event.sender.id) || ''
})

// Helper: sync port forwards for a connection to the saved connection in settings
function syncSavedPortForwards(connectionId: string): void {
  if (!connectionManager) return
  const settings = loadSettings()
  const saved = settings.ssh.savedConnections.find(c => c.id === connectionId)
  if (!saved) return
  const activeForwards = connectionManager.listPortForwards(connectionId)
  saved.portForwards = activeForwards.map(pf => ({
    localPort: pf.localPort,
    remoteHost: pf.remoteHost,
    remotePort: pf.remotePort
  }))
  saveSettings(settings)
}

// Helper: start port forwards from config and register watchers
function autoStartPortForwards(
  config: SSHConnectionConfig,
): void {
  if (!connectionManager || config.portForwards.length === 0) return

  for (const spec of config.portForwards) {
    const pfConfig: PortForwardConfig = {
      id: randomUUID(),
      connectionId: config.id,
      localPort: spec.localPort,
      remoteHost: spec.remoteHost,
      remotePort: spec.remotePort,
    }

    try {
      connectionManager.addPortForward(pfConfig)

      const { unsubscribe } = connectionManager.watchPortForwardStatus(pfConfig.id, (pfInfo) => {
        server.sshPortForwardStatus(pfInfo)
      })
      pfStatusWatchUnsubs.set(pfConfig.id, unsubscribe)
    } catch (err) {
      console.error(`[main:ssh] Failed to auto-start port forward ${String(spec.localPort)}:${spec.remoteHost}:${String(spec.remotePort)}:`, err)
    }
  }
}

// Local connection handler — renderer-driven, each window gets its own gRPC client
server.onLocalConnect(async (windowUuid) => {
  if (!connectionManager) throw new Error('ConnectionManager not initialized')

  // Cancel previous watch for this window (HMR reload)
  const prevConnId = windowConnectionMap.get(windowUuid)
  if (prevConnId) {
    const existing = sessionWatchUnsubs.get(prevConnId) ?? []
    for (const entry of existing) {
      entry.unsubscribe()
    }
    sessionWatchUnsubs.delete(prevConnId)
    connectionManager.disconnect(prevConnId)
    windowConnectionMap.delete(windowUuid)
  }

  // Each window gets its own gRPC client with autogenerated connection ID
  const info = await connectionManager.connectLocal()
  const connectionId = info.id
  windowConnectionMap.set(windowUuid, connectionId)

  const client = connectionManager.getClient(connectionId)

  const watch = client.watchSession(connectionId, (updatedSession) => {
    console.log('[main] session sync received for connection', connectionId, {
      sessionId: updatedSession.id,
      workspaces: updatedSession.workspaces.map(ws => ({ path: ws.path, metadata: ws.metadata })),
    })
    server.sessionSync(connectionId, updatedSession)
  })

  registerSessionWatch(connectionId, connectionId, watch.unsubscribe)

  const session = await watch.initial
  console.log('[main] localConnect: loaded session:', session.id)
  await createSessionClient(connectionId, client.socketPath)

  return { info, session }
})

// SSH IPC Handlers
server.onSshConnect(async (config, options) => {
  if (!connectionManager) throw new Error('ConnectionManager not initialized')

  console.log(`[main:ssh] onSshConnect called for host=${config.host}, id=${config.id}, refreshDaemon=${String(options?.refreshDaemon ?? false)}, allowOutdatedDaemon=${String(options?.allowOutdatedDaemon ?? false)}`)
  const info = await connectionManager.connectRemote(config, { refreshDaemon: options?.refreshDaemon, allowOutdatedDaemon: options?.allowOutdatedDaemon })
  console.log(`[main:ssh] connectRemote returned status=${info.status}${info.status === ConnectionStatus.Error ? `, error=${info.error}` : ''}`)

  if (info.status === ConnectionStatus.Connected) {
    // Load session from remote daemon and return it alongside connection info
    const remoteClient = connectionManager.getClient(config.id)
    try {
      console.log(`[main:ssh] Starting session watch for remote daemon`)
      const watchUuid = randomUUID()
      const remoteWatch = remoteClient.watchSession(watchUuid, (updatedSession) => {
        console.log(`[main:ssh] Session sync update received for session=${updatedSession.id}, workspaces=${String(updatedSession.workspaces.length)}`)
        server.sessionSync(config.id, updatedSession)
      })
      // Register for reconnect re-establishment
      registerSessionWatch(config.id, watchUuid, remoteWatch.unsubscribe)

      const session = await remoteWatch.initial
      console.log(`[main:ssh] Initial session loaded: id=${session.id}, workspaces=${String(session.workspaces.length)}`)
      await createSessionClient(config.id, remoteClient.socketPath)

      // Auto-start saved port forwards
      autoStartPortForwards(config)

      return { info, session }
    } catch (error) {
      const errorMsg = error instanceof Error ? error.message : String(error)
      console.error('[main:ssh] Failed to load remote session:', errorMsg)
      const isOldDaemon = errorMsg.includes('NOT_FOUND') && errorMsg.includes('session')
      const userError = isOldDaemon
        ? `Remote daemon is outdated. Retry with 'Refresh remote daemon' checked. (${errorMsg})`
        : `Connected but failed to load session: ${errorMsg}`
      return {
        info: { ...info, status: ConnectionStatus.Error, error: userError },
        session: null
      }
    }
  }

  return { info, session: null }
})

// eslint-disable-next-line @typescript-eslint/require-await
server.onSshDisconnect(async (connectionId) => {
  if (!connectionManager) throw new Error('ConnectionManager not initialized')
  connectionManager.disconnect(connectionId)
})

// eslint-disable-next-line @typescript-eslint/require-await
server.onSshReconnect(async (connectionId) => {
  if (!connectionManager) throw new Error('ConnectionManager not initialized')
  connectionManager.reconnect(connectionId)
})

// eslint-disable-next-line @typescript-eslint/require-await
server.onSshReconnectNow(async (connectionId) => {
  if (!connectionManager) throw new Error('ConnectionManager not initialized')
  connectionManager.reconnectNow(connectionId)
})

// eslint-disable-next-line @typescript-eslint/require-await
server.onSshForceReconnect(async (connectionId) => {
  if (!connectionManager) throw new Error('ConnectionManager not initialized')
  connectionManager.forceReconnect(connectionId)
})

// eslint-disable-next-line @typescript-eslint/require-await
server.onSshCancelReconnect(async (connectionId) => {
  if (!connectionManager) throw new Error('ConnectionManager not initialized')
  connectionManager.cancelReconnect(connectionId)
})

server.onSshListConnections(() => {
  if (!connectionManager) throw new Error('ConnectionManager not initialized')
  return connectionManager.listConnections()
})

// eslint-disable-next-line @typescript-eslint/require-await
server.onSshSaveConnection(async (config) => {
  const settings = loadSettings()
  const existing = settings.ssh.savedConnections.findIndex(
    c => c.host === config.host && c.user === config.user && c.port === config.port
  )
  if (existing >= 0) {
    settings.ssh.savedConnections[existing] = config
  } else {
    settings.ssh.savedConnections.push(config)
  }
  saveSettings(settings)
})

server.onSshGetSavedConnections(() => {
  const settings = loadSettings()
  return settings.ssh.savedConnections
})

// eslint-disable-next-line @typescript-eslint/require-await
server.onSshRemoveSavedConnection(async (id) => {
  const settings = loadSettings()
  settings.ssh.savedConnections = settings.ssh.savedConnections.filter(c => c.id !== id)
  saveSettings(settings)
})

// Flat watch subscriptions (keyed by "type:connId" or connId or pfId)
const outputWatchUnsubs = new Map<string, () => void>()
const statusWatchUnsubs = new Map<string, () => void>()

function registerOutputWatch(
  unsubs: Map<string, () => void>,
  key: string,
  watchFn: (id: string, cb: (line: string) => void) => { scrollback: string[], unsubscribe: () => void },
  connectionId: string,
  emitFn: (connectionId: string, line: string) => void,
): { scrollback: string[] } {
  unsubs.get(key)?.()

  const { scrollback, unsubscribe } = watchFn.call(connectionManager, connectionId, (line: string) => {
    emitFn(connectionId, line)
  })

  unsubs.set(key, unsubscribe)

  return { scrollback }
}

function unregisterOutputWatch(
  unsubs: Map<string, () => void>,
  key: string,
): void {
  unsubs.get(key)?.()
  unsubs.delete(key)
}

server.onSshWatchBootstrapOutput((connectionId) => {
  if (!connectionManager) throw new Error('ConnectionManager not initialized')
  return registerOutputWatch(outputWatchUnsubs, `bootstrap:${connectionId}`, connectionManager.watchBootstrapOutput.bind(connectionManager),
    connectionId, (cid, line) => { server.sshBootstrapOutput(cid, line) })
})

// eslint-disable-next-line @typescript-eslint/require-await
server.onSshUnwatchBootstrapOutput(async (connectionId) => {
  unregisterOutputWatch(outputWatchUnsubs, `bootstrap:${connectionId}`)
})

server.onSshWatchTunnelOutput((connectionId) => {
  if (!connectionManager) throw new Error('ConnectionManager not initialized')
  return registerOutputWatch(outputWatchUnsubs, `tunnel:${connectionId}`, connectionManager.watchTunnelOutput.bind(connectionManager),
    connectionId, (cid, line) => { server.sshTunnelOutput(cid, line) })
})

// eslint-disable-next-line @typescript-eslint/require-await
server.onSshUnwatchTunnelOutput(async (connectionId) => {
  unregisterOutputWatch(outputWatchUnsubs, `tunnel:${connectionId}`)
})

server.onSshWatchDaemonOutput((connectionId) => {
  if (!connectionManager) throw new Error('ConnectionManager not initialized')
  return registerOutputWatch(outputWatchUnsubs, `daemon:${connectionId}`, connectionManager.watchDaemonOutput.bind(connectionManager),
    connectionId, (cid, line) => { server.sshDaemonOutput(cid, line) })
})

// eslint-disable-next-line @typescript-eslint/require-await
server.onSshUnwatchDaemonOutput(async (connectionId) => {
  unregisterOutputWatch(outputWatchUnsubs, `daemon:${connectionId}`)
})

server.onSshWatchConnectionStatus((connectionId) => {
  if (!connectionManager) throw new Error('ConnectionManager not initialized')

  // Clean up any existing watch for this connection
  statusWatchUnsubs.get(connectionId)?.()

  const { initial, unsubscribe } = connectionManager.watchConnectionStatus(connectionId, (info) => {
    server.sshConnectionStatus(info)
  })

  if (!initial) throw new Error(`Connection not found: ${connectionId}`)

  statusWatchUnsubs.set(connectionId, unsubscribe)

  return { initial }
})

// eslint-disable-next-line @typescript-eslint/require-await
server.onSshUnwatchConnectionStatus(async (connectionId) => {
  statusWatchUnsubs.get(connectionId)?.()
  statusWatchUnsubs.delete(connectionId)
})

// Port forward IPC handlers
const pfStatusWatchUnsubs = new Map<string, () => void>()

server.onSshAddPortForward((config) => {
  if (!connectionManager) throw new Error('ConnectionManager not initialized')
  const info = connectionManager.addPortForward(config)

  // Register a status watcher (broadcast to all windows)
  const { unsubscribe } = connectionManager.watchPortForwardStatus(config.id, (pfInfo) => {
    server.sshPortForwardStatus(pfInfo)
  })
  pfStatusWatchUnsubs.set(config.id, unsubscribe)

  // Sync saved connection with updated port forwards
  syncSavedPortForwards(config.connectionId)

  return info
})

// eslint-disable-next-line @typescript-eslint/require-await
server.onSshRemovePortForward(async (portForwardId) => {
  if (!connectionManager) return
  // Find the owning connection before removal so we can sync saved config
  let ownerConnectionId: string | undefined
  for (const connInfo of connectionManager.listConnections()) {
    if (connectionManager.listPortForwards(connInfo.id).some(pf => pf.id === portForwardId)) {
      ownerConnectionId = connInfo.id
      break
    }
  }

  connectionManager.removePortForward(portForwardId)

  if (ownerConnectionId) {
    syncSavedPortForwards(ownerConnectionId)
  }
})

server.onSshListPortForwards((connectionId) => {
  if (!connectionManager) throw new Error('ConnectionManager not initialized')
  return connectionManager.listPortForwards(connectionId)
})

// Port forward output watch subscriptions
const pfOutputWatchUnsubs = new Map<string, () => void>()

server.onSshWatchPortForwardOutput((portForwardId) => {
  if (!connectionManager) throw new Error('ConnectionManager not initialized')

  pfOutputWatchUnsubs.get(portForwardId)?.()

  const { scrollback, unsubscribe } = connectionManager.watchPortForwardOutput(portForwardId, (line) => {
    server.sshPortForwardOutput(portForwardId, line)
  })

  pfOutputWatchUnsubs.set(portForwardId, unsubscribe)

  return { scrollback }
})

// eslint-disable-next-line @typescript-eslint/require-await
server.onSshUnwatchPortForwardOutput(async (portForwardId) => {
  pfOutputWatchUnsubs.get(portForwardId)?.()
  pfOutputWatchUnsubs.delete(portForwardId)
})

// App close confirmation IPC handlers
server.onAppCloseConfirmed((event) => {
  const window = BrowserWindow.fromWebContents(event.sender)
  if (window) {
    closeConfirmedWindows.add(event.sender.id)
    window.close()
  }
})

server.onAppCloseCancelled(() => {
  // No-op: closeConfirmedWindows only tracks confirmed windows
})

// App lifecycle
void app.whenReady().then(async () => {
  // Always use daemon mode
  console.log('[main] daemon mode enabled')

  // Show loading screen while connecting to daemon (skip in test mode)
  if (process.env.NODE_ENV !== 'test') {
    createLoadingWindow()
  }

  const socketPath = process.env.TREETERM_SOCKET_PATH || getDefaultSocketPath()

  // Bootstrap: ensure daemon is running before any window connects
  const bootstrapClient = new GrpcDaemonClient(socketPath)
  await bootstrapClient.ensureDaemonRunning()
  bootstrapClient.disconnect()

  // Create ConnectionManager — local connections are created per-window via connectLocal()
  connectionManager = new ConnectionManager(socketPath)

  // Push connection status changes to all renderer windows
  connectionManager.onStatusChange((info) => {
    const prevStatus = previousConnectionStatuses.get(info.id)
    previousConnectionStatuses.set(info.id, info.status)

    server.sshConnectionStatus(info)

    // On reconnect success: re-establish session watch and notify renderer
    if (info.status === ConnectionStatus.Connected && prevStatus === ConnectionStatus.Reconnecting) {
      console.log(`[main] connection ${info.id} reconnected, re-establishing session watches`)
      try {
        if (!connectionManager) throw new Error('ConnectionManager not initialized')
        const client = connectionManager.getClient(info.id)
        reestablishSessionWatches(info.id, client)
      } catch (error) {
        console.error(`[main] failed to re-establish session watches after reconnect:`, error)
      }
    }
  })

  // Close loading window and show main window
  if (loadingWindow) {
    loadingWindow.close()
    loadingWindow = null
  }

  createWindow()
  createApplicationMenu(server, () => { void quitAndKillDaemon() })

  // Handle --ssh startup argument
  if (initialSSHTarget) {
    const parsed = parseSSHTarget(initialSSHTarget)
    if (parsed) {
      console.log('[main] Auto-connecting SSH:', initialSSHTarget)
      void connectionManager.connectRemote(parsed).then(async (info) => {
        if (info.status === ConnectionStatus.Connected) {
          console.log('[main] SSH connected:', info.id)
          // Load session from remote daemon and re-initialize the renderer
          try {
            if (!connectionManager) throw new Error('ConnectionManager not initialized')
            const remoteClient = connectionManager.getClient(parsed.id)
            const autoWatchUuid = randomUUID()
            const remoteWatch = remoteClient.watchSession(autoWatchUuid, (updatedSession) => {
              server.sessionSync(parsed.id, updatedSession)
            })
            // Register for reconnect re-establishment
            registerSessionWatch(parsed.id, autoWatchUuid, remoteWatch.unsubscribe)

            const session = await remoteWatch.initial
            const autoClient = connectionManager.getClient(parsed.id)
            await createSessionClient(parsed.id, autoClient.socketPath)
            server.sshAutoConnected(session, info)
          } catch (error) {
            console.error('[main] Failed to load remote session:', error)
          }
        } else {
          console.error('[main] SSH connection failed:', info.status === ConnectionStatus.Error ? info.error : `status=${info.status}`)
        }
      }).catch((error: unknown) => {
        console.error('[main] SSH connection error:', error)
      })
    }
    initialSSHTarget = null
  }

  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0) {
      createWindow()
    }
  })
}).catch((error: unknown) => {
  console.error('[main] startup failed:', error)
  dialog.showErrorBox('Startup Error', `TreeTerm failed to start: ${error instanceof Error ? error.message : String(error)}`)
  app.quit()
})

  app.on('window-all-closed', () => {
  app.quit()
})

// Parse SSH target string like "user@host" or "user@host:port"
function parseSSHTarget(target: string): SSHConnectionConfig | null {
  const match = target.match(/^([^@]+)@([^:]+)(?::(\d+))?$/)
  if (!match) return null
  // eslint-disable-next-line @typescript-eslint/no-non-null-assertion -- regex groups guaranteed by match
  const user = match[1]!
  // eslint-disable-next-line @typescript-eslint/no-non-null-assertion -- regex groups guaranteed by match
  const host = match[2]!
  return {
    id: `ssh-${host}-${String(Date.now())}`,
    user,
    host,
    port: match[3] ? parseInt(match[3], 10) : 22,
    label: target,
    portForwards: [],
  }
}

async function quitAndKillDaemon(): Promise<void> {
  if (connectionManager) {
    try {
      console.log('[main] shutting down daemon before quit')
      // Use any connected local client to send shutdown, or create a temporary one
      const localConns = connectionManager.listConnections().filter(c => c.target.type === 'local' && c.status === ConnectionStatus.Connected)
      if (localConns.length > 0) {
        // eslint-disable-next-line @typescript-eslint/no-non-null-assertion -- length > 0 checked above
        await connectionManager.getClient(localConns[0]!.id).shutdownDaemon()
      } else {
        const tempClient = new GrpcDaemonClient(connectionManager.socketPath)
        await tempClient.connect()
        await tempClient.shutdownDaemon()
        tempClient.disconnect()
      }
    } catch (error) {
      const errorMessage = error instanceof Error ? error.message : 'Unknown error'
      console.error('[main] failed to shutdown daemon:', errorMessage)
    }
  }
  app.quit()
}

app.on('before-quit', () => {
  if (connectionManager) {
    connectionManager.disconnectAll()
  }
})
