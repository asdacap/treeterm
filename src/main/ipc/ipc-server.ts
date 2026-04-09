/**
 * Type-safe IPC server for the main process.
 * Provides business-level method names for registering handlers and emitting events.
 */

import { ipcMain, BrowserWindow, IpcMainInvokeEvent, IpcMainEvent } from 'electron'
import type { IpcRequests, IpcSends, IpcEvents } from '../../shared/ipc-types'

// Channel name mapping (camelCase → colon:separated)
const CHANNELS = {
  // Request channels
  ptyCreate: 'pty:create',
  ptyAttach: 'pty:attach',
  ptyList: 'pty:list',
  settingsLoad: 'settings:load',
  settingsSave: 'settings:save',
  fsReadDirectory: 'fs:readDirectory',
  fsReadFile: 'fs:readFile',
  fsWriteFile: 'fs:writeFile',
  fsSearchFiles: 'fs:searchFiles',
  ptyCreateSession: 'pty:createSession',
  sessionUpdate: 'session:update',
  sessionLock: 'session:lock',
  sessionUnlock: 'session:unlock',
  sessionForceUnlock: 'session:forceUnlock',
  daemonShutdown: 'daemon:shutdown',
  dialogSelectFolder: 'dialog:selectFolder',
  dialogGetRecentDirectories: 'dialog:getRecentDirectories',
  sandboxIsAvailable: 'sandbox:isAvailable',
  appGetInitialWorkspace: 'app:getInitialWorkspace',
  appGetWindowUuid: 'app:getWindowUuid',
  sshConnect: 'ssh:connect',
  sshDisconnect: 'ssh:disconnect',
  sshReconnect: 'ssh:reconnect',
  sshReconnectNow: 'ssh:reconnectNow',
  sshForceReconnect: 'ssh:forceReconnect',
  sshCancelReconnect: 'ssh:cancelReconnect',
  sshListConnections: 'ssh:listConnections',
  sshSaveConnection: 'ssh:saveConnection',
  sshGetSavedConnections: 'ssh:getSavedConnections',
  sshRemoveSavedConnection: 'ssh:removeSavedConnection',
  sshWatchBootstrapOutput: 'ssh:watchBootstrapOutput',
  sshUnwatchBootstrapOutput: 'ssh:unwatchBootstrapOutput',
  sshWatchTunnelOutput: 'ssh:watchTunnelOutput',
  sshUnwatchTunnelOutput: 'ssh:unwatchTunnelOutput',
  sshWatchDaemonOutput: 'ssh:watchDaemonOutput',
  sshUnwatchDaemonOutput: 'ssh:unwatchDaemonOutput',
  sshWatchConnectionStatus: 'ssh:watchConnectionStatus',
  sshUnwatchConnectionStatus: 'ssh:unwatchConnectionStatus',
  sshAddPortForward: 'ssh:addPortForward',
  sshRemovePortForward: 'ssh:removePortForward',
  sshListPortForwards: 'ssh:listPortForwards',
  sshWatchPortForwardOutput: 'ssh:watchPortForwardOutput',
  sshUnwatchPortForwardOutput: 'ssh:unwatchPortForwardOutput',

  // Clipboard operations
  clipboardReadText: 'clipboard:readText',
  clipboardWriteText: 'clipboard:writeText',

  // Exec operations
  execStart: 'exec:start',
  execKill: 'exec:kill',
  execEvent: 'exec:event',

  // Send channels
  ptyWrite: 'pty:write',
  ptyResize: 'pty:resize',
  ptyKill: 'pty:kill',
  appCloseConfirmed: 'app:close-confirmed',
  appCloseCancelled: 'app:close-cancelled',
  // Event channels
  ptyEvent: 'pty:event',
  settingsOpen: 'settings:open',
  appConfirmClose: 'app:confirm-close',
  appReady: 'app:ready',
  capsLockEvent: 'capslock-event',
  daemonSessions: 'daemon:sessions',
  sessionSync: 'session:sync',
  sshAutoConnected: 'ssh:autoConnected',
  connectionReconnected: 'connection:reconnected',
  daemonDisconnected: 'daemon:disconnected',
  activeProcessesOpen: 'active-processes:open',
  sshConnectionStatus: 'ssh:connectionStatus',
  sshBootstrapOutput: 'ssh:bootstrapOutput',
  sshTunnelOutput: 'ssh:tunnelOutput',
  sshDaemonOutput: 'ssh:daemonOutput',
  sshPortForwardStatus: 'ssh:portForwardStatus',
  sshPortForwardOutput: 'ssh:portForwardOutput',
} as const

export class IpcServer {
  private window: BrowserWindow | null = null

  /**
   * Set the browser window to send events to.
   */
  setWindow(window: BrowserWindow | null): void {
    this.window = window
  }

  // ==================== Request Handlers (invoke/handle pattern) ====================

  // PTY request handlers
  onPtyCreate(
    handler: (
      event: IpcMainInvokeEvent,
      ...args: IpcRequests['ptyCreate']['params']
    ) => IpcRequests['ptyCreate']['result'] | Promise<IpcRequests['ptyCreate']['result']>
  ): void {
    ipcMain.handle(CHANNELS.ptyCreate, (event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(event, ...(args as IpcRequests['ptyCreate']['params']))
    )
  }

  onPtyAttach(
    handler: (
      event: IpcMainInvokeEvent,
      ...args: IpcRequests['ptyAttach']['params']
    ) => IpcRequests['ptyAttach']['result'] | Promise<IpcRequests['ptyAttach']['result']>
  ): void {
    ipcMain.handle(CHANNELS.ptyAttach, (event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(event, ...(args as IpcRequests['ptyAttach']['params']))
    )
  }

  onPtyList(
    handler: (
      ...args: IpcRequests['ptyList']['params']
    ) => IpcRequests['ptyList']['result'] | Promise<IpcRequests['ptyList']['result']>
  ): void {
    ipcMain.handle(CHANNELS.ptyList, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['ptyList']['params']))
    )
  }

  onPtyWrite(handler: (...args: IpcSends['ptyWrite']['params']) => void): void {
    ipcMain.on(CHANNELS.ptyWrite, (_event: IpcMainEvent, ...args: unknown[]) =>
      { handler(...(args as IpcSends['ptyWrite']['params'])); }
    )
  }

  onPtyResize(handler: (...args: IpcSends['ptyResize']['params']) => void): void {
    ipcMain.on(CHANNELS.ptyResize, (_event: IpcMainEvent, ...args: unknown[]) =>
      { handler(...(args as IpcSends['ptyResize']['params'])); }
    )
  }

  onPtyKill(handler: (...args: IpcSends['ptyKill']['params']) => void): void {
    ipcMain.on(CHANNELS.ptyKill, (_event: IpcMainEvent, ...args: unknown[]) =>
      { handler(...(args as IpcSends['ptyKill']['params'])); }
    )
  }

  // Settings request handlers
  onSettingsLoad(
    handler: (
      ...args: IpcRequests['settingsLoad']['params']
    ) => IpcRequests['settingsLoad']['result'] | Promise<IpcRequests['settingsLoad']['result']>
  ): void {
    ipcMain.handle(CHANNELS.settingsLoad, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['settingsLoad']['params']))
    )
  }

  onSettingsSave(
    handler: (
      ...args: IpcRequests['settingsSave']['params']
    ) => IpcRequests['settingsSave']['result'] | Promise<IpcRequests['settingsSave']['result']>
  ): void {
    ipcMain.handle(CHANNELS.settingsSave, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['settingsSave']['params']))
    )
  }

  // Filesystem request handlers
  onFsReadDirectory(
    handler: (
      ...args: IpcRequests['fsReadDirectory']['params']
    ) => IpcRequests['fsReadDirectory']['result'] | Promise<IpcRequests['fsReadDirectory']['result']>
  ): void {
    ipcMain.handle(CHANNELS.fsReadDirectory, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['fsReadDirectory']['params']))
    )
  }

  onFsReadFile(
    handler: (
      ...args: IpcRequests['fsReadFile']['params']
    ) => IpcRequests['fsReadFile']['result'] | Promise<IpcRequests['fsReadFile']['result']>
  ): void {
    ipcMain.handle(CHANNELS.fsReadFile, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['fsReadFile']['params']))
    )
  }

  onFsWriteFile(
    handler: (
      ...args: IpcRequests['fsWriteFile']['params']
    ) => IpcRequests['fsWriteFile']['result'] | Promise<IpcRequests['fsWriteFile']['result']>
  ): void {
    ipcMain.handle(CHANNELS.fsWriteFile, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['fsWriteFile']['params']))
    )
  }

  onFsSearchFiles(
    handler: (
      ...args: IpcRequests['fsSearchFiles']['params']
    ) => IpcRequests['fsSearchFiles']['result'] | Promise<IpcRequests['fsSearchFiles']['result']>
  ): void {
    ipcMain.handle(CHANNELS.fsSearchFiles, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['fsSearchFiles']['params']))
    )
  }

  // Session request handlers
  onSessionUpdate(
    handler: (
      ...args: IpcRequests['sessionUpdate']['params']
    ) => IpcRequests['sessionUpdate']['result'] | Promise<IpcRequests['sessionUpdate']['result']>
  ): void {
    ipcMain.handle(CHANNELS.sessionUpdate, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['sessionUpdate']['params']))
    )
  }

  onSessionLock(
    handler: (
      ...args: IpcRequests['sessionLock']['params']
    ) => IpcRequests['sessionLock']['result'] | Promise<IpcRequests['sessionLock']['result']>
  ): void {
    ipcMain.handle(CHANNELS.sessionLock, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['sessionLock']['params']))
    )
  }

  onSessionUnlock(
    handler: (
      ...args: IpcRequests['sessionUnlock']['params']
    ) => IpcRequests['sessionUnlock']['result'] | Promise<IpcRequests['sessionUnlock']['result']>
  ): void {
    ipcMain.handle(CHANNELS.sessionUnlock, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['sessionUnlock']['params']))
    )
  }

  onSessionForceUnlock(
    handler: (
      ...args: IpcRequests['sessionForceUnlock']['params']
    ) => IpcRequests['sessionForceUnlock']['result'] | Promise<IpcRequests['sessionForceUnlock']['result']>
  ): void {
    ipcMain.handle(CHANNELS.sessionForceUnlock, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['sessionForceUnlock']['params']))
    )
  }

  // PTY create session handler (no stream)
  onPtyCreateSession(
    handler: (
      ...args: IpcRequests['ptyCreateSession']['params']
    ) => IpcRequests['ptyCreateSession']['result'] | Promise<IpcRequests['ptyCreateSession']['result']>
  ): void {
    ipcMain.handle(CHANNELS.ptyCreateSession, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['ptyCreateSession']['params']))
    )
  }

  // Other request handlers
  onDaemonShutdown(
    handler: (
      ...args: IpcRequests['daemonShutdown']['params']
    ) => IpcRequests['daemonShutdown']['result'] | Promise<IpcRequests['daemonShutdown']['result']>
  ): void {
    ipcMain.handle(CHANNELS.daemonShutdown, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['daemonShutdown']['params']))
    )
  }

  onDialogSelectFolder(
    handler: (
      ...args: IpcRequests['dialogSelectFolder']['params']
    ) => IpcRequests['dialogSelectFolder']['result'] | Promise<IpcRequests['dialogSelectFolder']['result']>
  ): void {
    ipcMain.handle(CHANNELS.dialogSelectFolder, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['dialogSelectFolder']['params']))
    )
  }

  onDialogGetRecentDirectories(
    handler: (
      ...args: IpcRequests['dialogGetRecentDirectories']['params']
    ) => IpcRequests['dialogGetRecentDirectories']['result'] | Promise<IpcRequests['dialogGetRecentDirectories']['result']>
  ): void {
    ipcMain.handle(CHANNELS.dialogGetRecentDirectories, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['dialogGetRecentDirectories']['params']))
    )
  }

  onSandboxIsAvailable(
    handler: (
      ...args: IpcRequests['sandboxIsAvailable']['params']
    ) => IpcRequests['sandboxIsAvailable']['result'] | Promise<IpcRequests['sandboxIsAvailable']['result']>
  ): void {
    ipcMain.handle(CHANNELS.sandboxIsAvailable, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['sandboxIsAvailable']['params']))
    )
  }

  onAppGetInitialWorkspace(
    handler: (
      ...args: IpcRequests['appGetInitialWorkspace']['params']
    ) => IpcRequests['appGetInitialWorkspace']['result'] | Promise<IpcRequests['appGetInitialWorkspace']['result']>
  ): void {
    ipcMain.handle(CHANNELS.appGetInitialWorkspace, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['appGetInitialWorkspace']['params']))
    )
  }

  onAppGetWindowUuid(
    handler: (event: IpcMainInvokeEvent) => IpcRequests['appGetWindowUuid']['result'] | Promise<IpcRequests['appGetWindowUuid']['result']>
  ): void {
    ipcMain.handle(CHANNELS.appGetWindowUuid, (event: IpcMainInvokeEvent) => handler(event))
  }

  // SSH request handlers
  onSshConnect(
    handler: (
      event: IpcMainInvokeEvent,
      ...args: IpcRequests['sshConnect']['params']
    ) => IpcRequests['sshConnect']['result'] | Promise<IpcRequests['sshConnect']['result']>
  ): void {
    ipcMain.handle(CHANNELS.sshConnect, (event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(event, ...(args as IpcRequests['sshConnect']['params']))
    )
  }

  onSshDisconnect(
    handler: (
      ...args: IpcRequests['sshDisconnect']['params']
    ) => IpcRequests['sshDisconnect']['result'] | Promise<IpcRequests['sshDisconnect']['result']>
  ): void {
    ipcMain.handle(CHANNELS.sshDisconnect, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['sshDisconnect']['params']))
    )
  }

  onSshReconnect(
    handler: (
      ...args: IpcRequests['sshReconnect']['params']
    ) => IpcRequests['sshReconnect']['result'] | Promise<IpcRequests['sshReconnect']['result']>
  ): void {
    ipcMain.handle(CHANNELS.sshReconnect, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['sshReconnect']['params']))
    )
  }

  onSshReconnectNow(
    handler: (
      ...args: IpcRequests['sshReconnectNow']['params']
    ) => IpcRequests['sshReconnectNow']['result'] | Promise<IpcRequests['sshReconnectNow']['result']>
  ): void {
    ipcMain.handle(CHANNELS.sshReconnectNow, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['sshReconnectNow']['params']))
    )
  }

  onSshForceReconnect(
    handler: (
      ...args: IpcRequests['sshForceReconnect']['params']
    ) => IpcRequests['sshForceReconnect']['result'] | Promise<IpcRequests['sshForceReconnect']['result']>
  ): void {
    ipcMain.handle(CHANNELS.sshForceReconnect, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['sshForceReconnect']['params']))
    )
  }

  onSshCancelReconnect(
    handler: (
      ...args: IpcRequests['sshCancelReconnect']['params']
    ) => IpcRequests['sshCancelReconnect']['result'] | Promise<IpcRequests['sshCancelReconnect']['result']>
  ): void {
    ipcMain.handle(CHANNELS.sshCancelReconnect, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['sshCancelReconnect']['params']))
    )
  }

  onSshListConnections(
    handler: (
      ...args: IpcRequests['sshListConnections']['params']
    ) => IpcRequests['sshListConnections']['result'] | Promise<IpcRequests['sshListConnections']['result']>
  ): void {
    ipcMain.handle(CHANNELS.sshListConnections, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['sshListConnections']['params']))
    )
  }

  onSshSaveConnection(
    handler: (
      ...args: IpcRequests['sshSaveConnection']['params']
    ) => IpcRequests['sshSaveConnection']['result'] | Promise<IpcRequests['sshSaveConnection']['result']>
  ): void {
    ipcMain.handle(CHANNELS.sshSaveConnection, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['sshSaveConnection']['params']))
    )
  }

  onSshGetSavedConnections(
    handler: (
      ...args: IpcRequests['sshGetSavedConnections']['params']
    ) => IpcRequests['sshGetSavedConnections']['result'] | Promise<IpcRequests['sshGetSavedConnections']['result']>
  ): void {
    ipcMain.handle(CHANNELS.sshGetSavedConnections, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['sshGetSavedConnections']['params']))
    )
  }

  onSshRemoveSavedConnection(
    handler: (
      ...args: IpcRequests['sshRemoveSavedConnection']['params']
    ) => IpcRequests['sshRemoveSavedConnection']['result'] | Promise<IpcRequests['sshRemoveSavedConnection']['result']>
  ): void {
    ipcMain.handle(CHANNELS.sshRemoveSavedConnection, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['sshRemoveSavedConnection']['params']))
    )
  }

  onSshWatchBootstrapOutput(
    handler: (
      event: IpcMainInvokeEvent,
      ...args: IpcRequests['sshWatchBootstrapOutput']['params']
    ) => IpcRequests['sshWatchBootstrapOutput']['result'] | Promise<IpcRequests['sshWatchBootstrapOutput']['result']>
  ): void {
    ipcMain.handle(CHANNELS.sshWatchBootstrapOutput, (event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(event, ...(args as IpcRequests['sshWatchBootstrapOutput']['params']))
    )
  }

  onSshUnwatchBootstrapOutput(
    handler: (
      event: IpcMainInvokeEvent,
      ...args: IpcRequests['sshUnwatchBootstrapOutput']['params']
    ) => IpcRequests['sshUnwatchBootstrapOutput']['result'] | Promise<IpcRequests['sshUnwatchBootstrapOutput']['result']>
  ): void {
    ipcMain.handle(CHANNELS.sshUnwatchBootstrapOutput, (event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(event, ...(args as IpcRequests['sshUnwatchBootstrapOutput']['params']))
    )
  }

  onSshWatchTunnelOutput(
    handler: (
      event: IpcMainInvokeEvent,
      ...args: IpcRequests['sshWatchTunnelOutput']['params']
    ) => IpcRequests['sshWatchTunnelOutput']['result'] | Promise<IpcRequests['sshWatchTunnelOutput']['result']>
  ): void {
    ipcMain.handle(CHANNELS.sshWatchTunnelOutput, (event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(event, ...(args as IpcRequests['sshWatchTunnelOutput']['params']))
    )
  }

  onSshUnwatchTunnelOutput(
    handler: (
      event: IpcMainInvokeEvent,
      ...args: IpcRequests['sshUnwatchTunnelOutput']['params']
    ) => IpcRequests['sshUnwatchTunnelOutput']['result'] | Promise<IpcRequests['sshUnwatchTunnelOutput']['result']>
  ): void {
    ipcMain.handle(CHANNELS.sshUnwatchTunnelOutput, (event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(event, ...(args as IpcRequests['sshUnwatchTunnelOutput']['params']))
    )
  }

  onSshWatchDaemonOutput(
    handler: (
      event: IpcMainInvokeEvent,
      ...args: IpcRequests['sshWatchDaemonOutput']['params']
    ) => IpcRequests['sshWatchDaemonOutput']['result'] | Promise<IpcRequests['sshWatchDaemonOutput']['result']>
  ): void {
    ipcMain.handle(CHANNELS.sshWatchDaemonOutput, (event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(event, ...(args as IpcRequests['sshWatchDaemonOutput']['params']))
    )
  }

  onSshUnwatchDaemonOutput(
    handler: (
      event: IpcMainInvokeEvent,
      ...args: IpcRequests['sshUnwatchDaemonOutput']['params']
    ) => IpcRequests['sshUnwatchDaemonOutput']['result'] | Promise<IpcRequests['sshUnwatchDaemonOutput']['result']>
  ): void {
    ipcMain.handle(CHANNELS.sshUnwatchDaemonOutput, (event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(event, ...(args as IpcRequests['sshUnwatchDaemonOutput']['params']))
    )
  }

  onSshWatchConnectionStatus(
    handler: (
      event: IpcMainInvokeEvent,
      ...args: IpcRequests['sshWatchConnectionStatus']['params']
    ) => IpcRequests['sshWatchConnectionStatus']['result'] | Promise<IpcRequests['sshWatchConnectionStatus']['result']>
  ): void {
    ipcMain.handle(CHANNELS.sshWatchConnectionStatus, (event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(event, ...(args as IpcRequests['sshWatchConnectionStatus']['params']))
    )
  }

  onSshUnwatchConnectionStatus(
    handler: (
      event: IpcMainInvokeEvent,
      ...args: IpcRequests['sshUnwatchConnectionStatus']['params']
    ) => IpcRequests['sshUnwatchConnectionStatus']['result'] | Promise<IpcRequests['sshUnwatchConnectionStatus']['result']>
  ): void {
    ipcMain.handle(CHANNELS.sshUnwatchConnectionStatus, (event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(event, ...(args as IpcRequests['sshUnwatchConnectionStatus']['params']))
    )
  }

  onSshAddPortForward(
    handler: (
      event: IpcMainInvokeEvent,
      ...args: IpcRequests['sshAddPortForward']['params']
    ) => IpcRequests['sshAddPortForward']['result'] | Promise<IpcRequests['sshAddPortForward']['result']>
  ): void {
    ipcMain.handle(CHANNELS.sshAddPortForward, (event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(event, ...(args as IpcRequests['sshAddPortForward']['params']))
    )
  }

  onSshRemovePortForward(
    handler: (
      ...args: IpcRequests['sshRemovePortForward']['params']
    ) => IpcRequests['sshRemovePortForward']['result'] | Promise<IpcRequests['sshRemovePortForward']['result']>
  ): void {
    ipcMain.handle(CHANNELS.sshRemovePortForward, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['sshRemovePortForward']['params']))
    )
  }

  onSshListPortForwards(
    handler: (
      ...args: IpcRequests['sshListPortForwards']['params']
    ) => IpcRequests['sshListPortForwards']['result'] | Promise<IpcRequests['sshListPortForwards']['result']>
  ): void {
    ipcMain.handle(CHANNELS.sshListPortForwards, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['sshListPortForwards']['params']))
    )
  }

  onSshWatchPortForwardOutput(
    handler: (
      event: IpcMainInvokeEvent,
      ...args: IpcRequests['sshWatchPortForwardOutput']['params']
    ) => IpcRequests['sshWatchPortForwardOutput']['result'] | Promise<IpcRequests['sshWatchPortForwardOutput']['result']>
  ): void {
    ipcMain.handle(CHANNELS.sshWatchPortForwardOutput, (event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(event, ...(args as IpcRequests['sshWatchPortForwardOutput']['params']))
    )
  }

  onSshUnwatchPortForwardOutput(
    handler: (
      event: IpcMainInvokeEvent,
      ...args: IpcRequests['sshUnwatchPortForwardOutput']['params']
    ) => IpcRequests['sshUnwatchPortForwardOutput']['result'] | Promise<IpcRequests['sshUnwatchPortForwardOutput']['result']>
  ): void {
    ipcMain.handle(CHANNELS.sshUnwatchPortForwardOutput, (event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(event, ...(args as IpcRequests['sshUnwatchPortForwardOutput']['params']))
    )
  }

  // ==================== Fire-and-Forget Handlers (send/on pattern) ====================

  onAppCloseConfirmed(handler: (event: IpcMainEvent) => void): void {
    ipcMain.on(CHANNELS.appCloseConfirmed, (event: IpcMainEvent) => { handler(event); })
  }

  onAppCloseCancelled(handler: (event: IpcMainEvent) => void): void {
    ipcMain.on(CHANNELS.appCloseCancelled, (event: IpcMainEvent) => { handler(event); })
  }

  onClipboardWriteText(handler: (...args: IpcSends['clipboardWriteText']['params']) => void): void {
    ipcMain.on(CHANNELS.clipboardWriteText, (_event: IpcMainEvent, ...args: unknown[]) =>
      { handler(...(args as IpcSends['clipboardWriteText']['params'])); }
    )
  }

  onClipboardReadText(
    handler: () => IpcRequests['clipboardReadText']['result'] | Promise<IpcRequests['clipboardReadText']['result']>
  ): void {
    ipcMain.handle(CHANNELS.clipboardReadText, () => handler())
  }

  // Exec handlers
  onExecStart(
    handler: (
      ...args: IpcRequests['execStart']['params']
    ) => IpcRequests['execStart']['result'] | Promise<IpcRequests['execStart']['result']>
  ): void {
    ipcMain.handle(CHANNELS.execStart, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['execStart']['params']))
    )
  }

  onExecKill(handler: (...args: IpcSends['execKill']['params']) => void): void {
    ipcMain.on(CHANNELS.execKill, (_event: IpcMainEvent, ...args: unknown[]) =>
      { handler(...(args as IpcSends['execKill']['params'])); }
    )
  }

  // ==================== Event Emitters (main → renderer) ====================

  ptyEvent(...args: IpcEvents['ptyEvent']['params']): void {
    this.window?.webContents.send(CHANNELS.ptyEvent, ...args)
  }

  settingsOpen(): void {
    this.window?.webContents.send(CHANNELS.settingsOpen)
  }

  appConfirmClose(): void {
    this.window?.webContents.send(CHANNELS.appConfirmClose)
  }

  appReady(...args: IpcEvents['appReady']['params']): void {
    this.window?.webContents.send(CHANNELS.appReady, ...args)
  }

  capsLockEvent(...args: IpcEvents['capsLockEvent']['params']): void {
    this.window?.webContents.send(CHANNELS.capsLockEvent, ...args)
  }

  daemonSessions(...args: IpcEvents['daemonSessions']['params']): void {
    this.window?.webContents.send(CHANNELS.daemonSessions, ...args)
  }

  sessionSync(...args: IpcEvents['sessionSync']['params']): void {
    this.window?.webContents.send(CHANNELS.sessionSync, ...args)
  }

  sshAutoConnected(...args: IpcEvents['sshAutoConnected']['params']): void {
    this.window?.webContents.send(CHANNELS.sshAutoConnected, ...args)
  }

  connectionReconnected(...args: IpcEvents['connectionReconnected']['params']): void {
    this.window?.webContents.send(CHANNELS.connectionReconnected, ...args)
  }

  daemonDisconnected(): void {
    this.window?.webContents.send(CHANNELS.daemonDisconnected)
  }

  activeProcessesOpen(): void {
    this.window?.webContents.send(CHANNELS.activeProcessesOpen)
  }

  sshConnectionStatus(...args: IpcEvents['sshConnectionStatus']['params']): void {
    this.window?.webContents.send(CHANNELS.sshConnectionStatus, ...args)
  }

  sshBootstrapOutput(...args: IpcEvents['sshBootstrapOutput']['params']): void {
    this.window?.webContents.send(CHANNELS.sshBootstrapOutput, ...args)
  }

  sshTunnelOutput(...args: IpcEvents['sshTunnelOutput']['params']): void {
    this.window?.webContents.send(CHANNELS.sshTunnelOutput, ...args)
  }

  sshDaemonOutput(...args: IpcEvents['sshDaemonOutput']['params']): void {
    this.window?.webContents.send(CHANNELS.sshDaemonOutput, ...args)
  }

  sshPortForwardStatus(...args: IpcEvents['sshPortForwardStatus']['params']): void {
    this.window?.webContents.send(CHANNELS.sshPortForwardStatus, ...args)
  }

  sshPortForwardOutput(...args: IpcEvents['sshPortForwardOutput']['params']): void {
    this.window?.webContents.send(CHANNELS.sshPortForwardOutput, ...args)
  }

  execEvent(...args: IpcEvents['execEvent']['params']): void {
    this.window?.webContents.send(CHANNELS.execEvent, ...args)
  }
}
