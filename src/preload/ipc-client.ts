/**
 * Type-safe IPC client for the preload/renderer process.
 * Provides business-level method names for invoking procedures and listening to events.
 */

import { ipcRenderer, IpcRendererEvent } from 'electron'
import type { IpcRequests, IpcSends, IpcEvents } from '../shared/ipc-types'

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

export class IpcClient {
  // ==================== Request Methods (invoke pattern, returns Promise) ====================

  // PTY requests
  ptyCreate(...args: IpcRequests['ptyCreate']['params']): Promise<IpcRequests['ptyCreate']['result']> {
    return ipcRenderer.invoke(CHANNELS.ptyCreate, ...args)
  }

  ptyAttach(...args: IpcRequests['ptyAttach']['params']): Promise<IpcRequests['ptyAttach']['result']> {
    return ipcRenderer.invoke(CHANNELS.ptyAttach, ...args)
  }

  ptyList(...args: IpcRequests['ptyList']['params']): Promise<IpcRequests['ptyList']['result']> {
    return ipcRenderer.invoke(CHANNELS.ptyList, ...args)
  }

  // Settings requests
  settingsLoad(...args: IpcRequests['settingsLoad']['params']): Promise<IpcRequests['settingsLoad']['result']> {
    return ipcRenderer.invoke(CHANNELS.settingsLoad, ...args)
  }

  settingsSave(...args: IpcRequests['settingsSave']['params']): Promise<IpcRequests['settingsSave']['result']> {
    return ipcRenderer.invoke(CHANNELS.settingsSave, ...args)
  }

  // Filesystem requests
  fsReadDirectory(
    ...args: IpcRequests['fsReadDirectory']['params']
  ): Promise<IpcRequests['fsReadDirectory']['result']> {
    return ipcRenderer.invoke(CHANNELS.fsReadDirectory, ...args)
  }

  fsReadFile(...args: IpcRequests['fsReadFile']['params']): Promise<IpcRequests['fsReadFile']['result']> {
    return ipcRenderer.invoke(CHANNELS.fsReadFile, ...args)
  }

  fsWriteFile(...args: IpcRequests['fsWriteFile']['params']): Promise<IpcRequests['fsWriteFile']['result']> {
    return ipcRenderer.invoke(CHANNELS.fsWriteFile, ...args)
  }

  fsSearchFiles(...args: IpcRequests['fsSearchFiles']['params']): Promise<IpcRequests['fsSearchFiles']['result']> {
    return ipcRenderer.invoke(CHANNELS.fsSearchFiles, ...args)
  }

  // PTY create session (no stream)
  ptyCreateSession(
    ...args: IpcRequests['ptyCreateSession']['params']
  ): Promise<IpcRequests['ptyCreateSession']['result']> {
    return ipcRenderer.invoke(CHANNELS.ptyCreateSession, ...args)
  }

  // Session requests
  sessionUpdate(...args: IpcRequests['sessionUpdate']['params']): Promise<IpcRequests['sessionUpdate']['result']> {
    return ipcRenderer.invoke(CHANNELS.sessionUpdate, ...args)
  }

  sessionLock(...args: IpcRequests['sessionLock']['params']): Promise<IpcRequests['sessionLock']['result']> {
    return ipcRenderer.invoke(CHANNELS.sessionLock, ...args)
  }

  sessionUnlock(...args: IpcRequests['sessionUnlock']['params']): Promise<IpcRequests['sessionUnlock']['result']> {
    return ipcRenderer.invoke(CHANNELS.sessionUnlock, ...args)
  }

  sessionForceUnlock(...args: IpcRequests['sessionForceUnlock']['params']): Promise<IpcRequests['sessionForceUnlock']['result']> {
    return ipcRenderer.invoke(CHANNELS.sessionForceUnlock, ...args)
  }

  // Other requests
  daemonShutdown(
    ...args: IpcRequests['daemonShutdown']['params']
  ): Promise<IpcRequests['daemonShutdown']['result']> {
    return ipcRenderer.invoke(CHANNELS.daemonShutdown, ...args)
  }

  dialogSelectFolder(
    ...args: IpcRequests['dialogSelectFolder']['params']
  ): Promise<IpcRequests['dialogSelectFolder']['result']> {
    return ipcRenderer.invoke(CHANNELS.dialogSelectFolder, ...args)
  }

  dialogGetRecentDirectories(
    ...args: IpcRequests['dialogGetRecentDirectories']['params']
  ): Promise<IpcRequests['dialogGetRecentDirectories']['result']> {
    return ipcRenderer.invoke(CHANNELS.dialogGetRecentDirectories, ...args)
  }

  sandboxIsAvailable(
    ...args: IpcRequests['sandboxIsAvailable']['params']
  ): Promise<IpcRequests['sandboxIsAvailable']['result']> {
    return ipcRenderer.invoke(CHANNELS.sandboxIsAvailable, ...args)
  }

  appGetInitialWorkspace(
    ...args: IpcRequests['appGetInitialWorkspace']['params']
  ): Promise<IpcRequests['appGetInitialWorkspace']['result']> {
    return ipcRenderer.invoke(CHANNELS.appGetInitialWorkspace, ...args)
  }

  appGetWindowUuid(): Promise<IpcRequests['appGetWindowUuid']['result']> {
    return ipcRenderer.invoke(CHANNELS.appGetWindowUuid)
  }

  // SSH requests
  sshConnect(...args: IpcRequests['sshConnect']['params']): Promise<IpcRequests['sshConnect']['result']> {
    return ipcRenderer.invoke(CHANNELS.sshConnect, ...args)
  }

  sshDisconnect(...args: IpcRequests['sshDisconnect']['params']): Promise<IpcRequests['sshDisconnect']['result']> {
    return ipcRenderer.invoke(CHANNELS.sshDisconnect, ...args)
  }

  sshReconnect(...args: IpcRequests['sshReconnect']['params']): Promise<IpcRequests['sshReconnect']['result']> {
    return ipcRenderer.invoke(CHANNELS.sshReconnect, ...args)
  }

  sshReconnectNow(...args: IpcRequests['sshReconnectNow']['params']): Promise<IpcRequests['sshReconnectNow']['result']> {
    return ipcRenderer.invoke(CHANNELS.sshReconnectNow, ...args)
  }

  sshForceReconnect(...args: IpcRequests['sshForceReconnect']['params']): Promise<IpcRequests['sshForceReconnect']['result']> {
    return ipcRenderer.invoke(CHANNELS.sshForceReconnect, ...args)
  }

  sshCancelReconnect(...args: IpcRequests['sshCancelReconnect']['params']): Promise<IpcRequests['sshCancelReconnect']['result']> {
    return ipcRenderer.invoke(CHANNELS.sshCancelReconnect, ...args)
  }

  sshListConnections(...args: IpcRequests['sshListConnections']['params']): Promise<IpcRequests['sshListConnections']['result']> {
    return ipcRenderer.invoke(CHANNELS.sshListConnections, ...args)
  }

  sshSaveConnection(...args: IpcRequests['sshSaveConnection']['params']): Promise<IpcRequests['sshSaveConnection']['result']> {
    return ipcRenderer.invoke(CHANNELS.sshSaveConnection, ...args)
  }

  sshGetSavedConnections(...args: IpcRequests['sshGetSavedConnections']['params']): Promise<IpcRequests['sshGetSavedConnections']['result']> {
    return ipcRenderer.invoke(CHANNELS.sshGetSavedConnections, ...args)
  }

  sshRemoveSavedConnection(...args: IpcRequests['sshRemoveSavedConnection']['params']): Promise<IpcRequests['sshRemoveSavedConnection']['result']> {
    return ipcRenderer.invoke(CHANNELS.sshRemoveSavedConnection, ...args)
  }

  sshWatchBootstrapOutput(...args: IpcRequests['sshWatchBootstrapOutput']['params']): Promise<IpcRequests['sshWatchBootstrapOutput']['result']> {
    return ipcRenderer.invoke(CHANNELS.sshWatchBootstrapOutput, ...args)
  }

  sshUnwatchBootstrapOutput(...args: IpcRequests['sshUnwatchBootstrapOutput']['params']): Promise<IpcRequests['sshUnwatchBootstrapOutput']['result']> {
    return ipcRenderer.invoke(CHANNELS.sshUnwatchBootstrapOutput, ...args)
  }

  sshWatchTunnelOutput(...args: IpcRequests['sshWatchTunnelOutput']['params']): Promise<IpcRequests['sshWatchTunnelOutput']['result']> {
    return ipcRenderer.invoke(CHANNELS.sshWatchTunnelOutput, ...args)
  }

  sshUnwatchTunnelOutput(...args: IpcRequests['sshUnwatchTunnelOutput']['params']): Promise<IpcRequests['sshUnwatchTunnelOutput']['result']> {
    return ipcRenderer.invoke(CHANNELS.sshUnwatchTunnelOutput, ...args)
  }

  sshWatchDaemonOutput(...args: IpcRequests['sshWatchDaemonOutput']['params']): Promise<IpcRequests['sshWatchDaemonOutput']['result']> {
    return ipcRenderer.invoke(CHANNELS.sshWatchDaemonOutput, ...args)
  }

  sshUnwatchDaemonOutput(...args: IpcRequests['sshUnwatchDaemonOutput']['params']): Promise<IpcRequests['sshUnwatchDaemonOutput']['result']> {
    return ipcRenderer.invoke(CHANNELS.sshUnwatchDaemonOutput, ...args)
  }

  sshWatchConnectionStatus(...args: IpcRequests['sshWatchConnectionStatus']['params']): Promise<IpcRequests['sshWatchConnectionStatus']['result']> {
    return ipcRenderer.invoke(CHANNELS.sshWatchConnectionStatus, ...args)
  }

  sshUnwatchConnectionStatus(...args: IpcRequests['sshUnwatchConnectionStatus']['params']): Promise<IpcRequests['sshUnwatchConnectionStatus']['result']> {
    return ipcRenderer.invoke(CHANNELS.sshUnwatchConnectionStatus, ...args)
  }

  sshAddPortForward(...args: IpcRequests['sshAddPortForward']['params']): Promise<IpcRequests['sshAddPortForward']['result']> {
    return ipcRenderer.invoke(CHANNELS.sshAddPortForward, ...args)
  }

  sshRemovePortForward(...args: IpcRequests['sshRemovePortForward']['params']): Promise<IpcRequests['sshRemovePortForward']['result']> {
    return ipcRenderer.invoke(CHANNELS.sshRemovePortForward, ...args)
  }

  sshListPortForwards(...args: IpcRequests['sshListPortForwards']['params']): Promise<IpcRequests['sshListPortForwards']['result']> {
    return ipcRenderer.invoke(CHANNELS.sshListPortForwards, ...args)
  }

  sshWatchPortForwardOutput(...args: IpcRequests['sshWatchPortForwardOutput']['params']): Promise<IpcRequests['sshWatchPortForwardOutput']['result']> {
    return ipcRenderer.invoke(CHANNELS.sshWatchPortForwardOutput, ...args)
  }

  sshUnwatchPortForwardOutput(...args: IpcRequests['sshUnwatchPortForwardOutput']['params']): Promise<IpcRequests['sshUnwatchPortForwardOutput']['result']> {
    return ipcRenderer.invoke(CHANNELS.sshUnwatchPortForwardOutput, ...args)
  }

  // ==================== Fire-and-Forget Methods (send pattern, no return) ====================

  ptyWrite(...args: IpcSends['ptyWrite']['params']): void {
    ipcRenderer.send(CHANNELS.ptyWrite, ...args)
  }

  ptyResize(...args: IpcSends['ptyResize']['params']): void {
    ipcRenderer.send(CHANNELS.ptyResize, ...args)
  }

  ptyKill(...args: IpcSends['ptyKill']['params']): void {
    ipcRenderer.send(CHANNELS.ptyKill, ...args)
  }

  appCloseConfirmed(...args: IpcSends['appCloseConfirmed']['params']): void {
    ipcRenderer.send(CHANNELS.appCloseConfirmed, ...args)
  }

  appCloseCancelled(...args: IpcSends['appCloseCancelled']['params']): void {
    ipcRenderer.send(CHANNELS.appCloseCancelled, ...args)
  }

  clipboardWriteText(...args: IpcSends['clipboardWriteText']['params']): void {
    ipcRenderer.send(CHANNELS.clipboardWriteText, ...args)
  }

  clipboardReadText(): Promise<IpcRequests['clipboardReadText']['result']> {
    return ipcRenderer.invoke(CHANNELS.clipboardReadText)
  }

  // Exec methods
  execStart(...args: IpcRequests['execStart']['params']): Promise<IpcRequests['execStart']['result']> {
    return ipcRenderer.invoke(CHANNELS.execStart, ...args)
  }

  execKill(...args: IpcSends['execKill']['params']): void {
    ipcRenderer.send(CHANNELS.execKill, ...args)
  }

  // ==================== Event Listeners (on pattern, returns unsubscribe function) ====================

  onPtyEvent(callback: (...args: IpcEvents['ptyEvent']['params']) => void): () => void {
    const handler = (_event: IpcRendererEvent, ...args: unknown[]) =>
      { callback(...(args as IpcEvents['ptyEvent']['params'])); }
    ipcRenderer.on(CHANNELS.ptyEvent, handler)
    return () => ipcRenderer.removeListener(CHANNELS.ptyEvent, handler)
  }

  onSettingsOpen(callback: () => void): () => void {
    const handler = () => { callback(); }
    ipcRenderer.on(CHANNELS.settingsOpen, handler)
    return () => ipcRenderer.removeListener(CHANNELS.settingsOpen, handler)
  }

  onAppConfirmClose(callback: () => void): () => void {
    const handler = () => { callback(); }
    ipcRenderer.on(CHANNELS.appConfirmClose, handler)
    return () => ipcRenderer.removeListener(CHANNELS.appConfirmClose, handler)
  }

  onAppReady(callback: (...args: IpcEvents['appReady']['params']) => void): () => void {
    const handler = (_event: IpcRendererEvent, ...args: unknown[]) =>
      { callback(...(args as IpcEvents['appReady']['params'])); }
    ipcRenderer.on(CHANNELS.appReady, handler)
    return () => ipcRenderer.removeListener(CHANNELS.appReady, handler)
  }

  onCapsLockEvent(callback: (...args: IpcEvents['capsLockEvent']['params']) => void): () => void {
    const handler = (_event: IpcRendererEvent, ...args: unknown[]) =>
      { callback(...(args as IpcEvents['capsLockEvent']['params'])); }
    ipcRenderer.on(CHANNELS.capsLockEvent, handler)
    return () => ipcRenderer.removeListener(CHANNELS.capsLockEvent, handler)
  }

  onDaemonSessions(callback: (...args: IpcEvents['daemonSessions']['params']) => void): () => void {
    const handler = (_event: IpcRendererEvent, ...args: unknown[]) =>
      { callback(...(args as IpcEvents['daemonSessions']['params'])); }
    ipcRenderer.on(CHANNELS.daemonSessions, handler)
    return () => ipcRenderer.removeListener(CHANNELS.daemonSessions, handler)
  }

  onSessionSync(callback: (...args: IpcEvents['sessionSync']['params']) => void): () => void {
    const handler = (_event: IpcRendererEvent, ...args: unknown[]) =>
      { callback(...(args as IpcEvents['sessionSync']['params'])); }
    ipcRenderer.on(CHANNELS.sessionSync, handler)
    return () => ipcRenderer.removeListener(CHANNELS.sessionSync, handler)
  }

  onSshAutoConnected(callback: (...args: IpcEvents['sshAutoConnected']['params']) => void): () => void {
    const handler = (_event: IpcRendererEvent, ...args: unknown[]) =>
      { callback(...(args as IpcEvents['sshAutoConnected']['params'])); }
    ipcRenderer.on(CHANNELS.sshAutoConnected, handler)
    return () => ipcRenderer.removeListener(CHANNELS.sshAutoConnected, handler)
  }

  onConnectionReconnected(callback: (...args: IpcEvents['connectionReconnected']['params']) => void): () => void {
    const handler = (_event: IpcRendererEvent, ...args: unknown[]) =>
      { callback(...(args as IpcEvents['connectionReconnected']['params'])); }
    ipcRenderer.on(CHANNELS.connectionReconnected, handler)
    return () => ipcRenderer.removeListener(CHANNELS.connectionReconnected, handler)
  }

  onDaemonDisconnected(callback: () => void): () => void {
    const handler = () => { callback(); }
    ipcRenderer.on(CHANNELS.daemonDisconnected, handler)
    return () => ipcRenderer.removeListener(CHANNELS.daemonDisconnected, handler)
  }

  onActiveProcessesOpen(callback: () => void): () => void {
    const handler = () => { callback(); }
    ipcRenderer.on(CHANNELS.activeProcessesOpen, handler)
    return () => ipcRenderer.removeListener(CHANNELS.activeProcessesOpen, handler)
  }

  onSshConnectionStatus(callback: (...args: IpcEvents['sshConnectionStatus']['params']) => void): () => void {
    const handler = (_event: IpcRendererEvent, ...args: unknown[]) =>
      { callback(...(args as IpcEvents['sshConnectionStatus']['params'])); }
    ipcRenderer.on(CHANNELS.sshConnectionStatus, handler)
    return () => ipcRenderer.removeListener(CHANNELS.sshConnectionStatus, handler)
  }

  onSshBootstrapOutput(callback: (...args: IpcEvents['sshBootstrapOutput']['params']) => void): () => void {
    const handler = (_event: IpcRendererEvent, ...args: unknown[]) =>
      { callback(...(args as IpcEvents['sshBootstrapOutput']['params'])); }
    ipcRenderer.on(CHANNELS.sshBootstrapOutput, handler)
    return () => ipcRenderer.removeListener(CHANNELS.sshBootstrapOutput, handler)
  }

  onSshTunnelOutput(callback: (...args: IpcEvents['sshTunnelOutput']['params']) => void): () => void {
    const handler = (_event: IpcRendererEvent, ...args: unknown[]) =>
      { callback(...(args as IpcEvents['sshTunnelOutput']['params'])); }
    ipcRenderer.on(CHANNELS.sshTunnelOutput, handler)
    return () => ipcRenderer.removeListener(CHANNELS.sshTunnelOutput, handler)
  }

  onSshDaemonOutput(callback: (...args: IpcEvents['sshDaemonOutput']['params']) => void): () => void {
    const handler = (_event: IpcRendererEvent, ...args: unknown[]) =>
      { callback(...(args as IpcEvents['sshDaemonOutput']['params'])); }
    ipcRenderer.on(CHANNELS.sshDaemonOutput, handler)
    return () => ipcRenderer.removeListener(CHANNELS.sshDaemonOutput, handler)
  }

  onSshPortForwardStatus(callback: (...args: IpcEvents['sshPortForwardStatus']['params']) => void): () => void {
    const handler = (_event: IpcRendererEvent, ...args: unknown[]) =>
      { callback(...(args as IpcEvents['sshPortForwardStatus']['params'])); }
    ipcRenderer.on(CHANNELS.sshPortForwardStatus, handler)
    return () => ipcRenderer.removeListener(CHANNELS.sshPortForwardStatus, handler)
  }

  onSshPortForwardOutput(callback: (...args: IpcEvents['sshPortForwardOutput']['params']) => void): () => void {
    const handler = (_event: IpcRendererEvent, ...args: unknown[]) =>
      { callback(...(args as IpcEvents['sshPortForwardOutput']['params'])); }
    ipcRenderer.on(CHANNELS.sshPortForwardOutput, handler)
    return () => ipcRenderer.removeListener(CHANNELS.sshPortForwardOutput, handler)
  }

  onExecEvent(callback: (...args: IpcEvents['execEvent']['params']) => void): () => void {
    const handler = (_event: IpcRendererEvent, ...args: unknown[]) =>
      { callback(...(args as IpcEvents['execEvent']['params'])); }
    ipcRenderer.on(CHANNELS.execEvent, handler)
    return () => ipcRenderer.removeListener(CHANNELS.execEvent, handler)
  }
}
