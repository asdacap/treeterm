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
  ptyDetach: 'pty:detach',
  ptyList: 'pty:list',
  ptyIsAlive: 'pty:isAlive',
  gitGetInfo: 'git:getInfo',
  gitCreateWorktree: 'git:createWorktree',
  gitRemoveWorktree: 'git:removeWorktree',
  gitListWorktrees: 'git:listWorktrees',
  gitGetChildWorktrees: 'git:getChildWorktrees',
  gitListLocalBranches: 'git:listLocalBranches',
  gitListRemoteBranches: 'git:listRemoteBranches',
  gitGetBranchesInWorktrees: 'git:getBranchesInWorktrees',
  gitCreateWorktreeFromBranch: 'git:createWorktreeFromBranch',
  gitCreateWorktreeFromRemote: 'git:createWorktreeFromRemote',
  gitGetDiff: 'git:getDiff',
  gitGetFileDiff: 'git:getFileDiff',
  gitMerge: 'git:merge',
  gitCheckMergeConflicts: 'git:checkMergeConflicts',
  gitHasUncommittedChanges: 'git:hasUncommittedChanges',
  gitCommitAll: 'git:commitAll',
  gitDeleteBranch: 'git:deleteBranch',
  gitGetUncommittedChanges: 'git:getUncommittedChanges',
  gitGetUncommittedFileDiff: 'git:getUncommittedFileDiff',
  gitStageFile: 'git:stageFile',
  gitUnstageFile: 'git:unstageFile',
  gitStageAll: 'git:stageAll',
  gitUnstageAll: 'git:unstageAll',
  gitCommitStaged: 'git:commitStaged',
  gitGetFileContentsForDiff: 'git:getFileContentsForDiff',
  gitGetUncommittedFileContentsForDiff: 'git:getUncommittedFileContentsForDiff',
  gitGetHeadCommitHash: 'git:getHeadCommitHash',
  settingsLoad: 'settings:load',
  settingsSave: 'settings:save',
  fsReadDirectory: 'fs:readDirectory',
  fsReadFile: 'fs:readFile',
  fsWriteFile: 'fs:writeFile',
  fsSearchFiles: 'fs:searchFiles',
  runActionsDetect: 'runActions:detect',
  runActionsRun: 'runActions:run',
  sttTranscribeOpenai: 'stt:transcribe-openai',
  sttTranscribeLocal: 'stt:transcribe-local',
  sttCheckMicPermission: 'stt:check-mic-permission',
  sessionCreate: 'session:create',
  sessionUpdate: 'session:update',
  sessionList: 'session:list',
  sessionGet: 'session:get',
  sessionDelete: 'session:delete',
  sessionOpenInNewWindow: 'session:open-in-new-window',
  daemonShutdown: 'daemon:shutdown',
  dialogSelectFolder: 'dialog:selectFolder',
  dialogGetRecentDirectories: 'dialog:getRecentDirectories',
  sandboxIsAvailable: 'sandbox:isAvailable',
  appGetInitialWorkspace: 'app:getInitialWorkspace',
  appGetWindowUuid: 'app:getWindowUuid',

  // Send channels
  ptyWrite: 'pty:write',
  ptyResize: 'pty:resize',
  ptyKill: 'pty:kill',
  appCloseConfirmed: 'app:close-confirmed',
  appCloseCancelled: 'app:close-cancelled',

  // Event channels
  ptyData: 'pty:data',
  ptyExit: 'pty:exit',
  settingsOpen: 'settings:open',
  appConfirmClose: 'app:confirm-close',
  appReady: 'app:ready',
  capsLockEvent: 'capslock-event',
  daemonSessions: 'daemon:sessions',
  terminalNew: 'terminal:new',
  terminalShowSessions: 'terminal:show-sessions',
  sessionShowSessions: 'session:show-sessions',
  sessionSync: 'session:sync',
  daemonDisconnected: 'daemon:disconnected',
  activeProcessesOpen: 'active-processes:open'
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

  ptyDetach(...args: IpcRequests['ptyDetach']['params']): Promise<IpcRequests['ptyDetach']['result']> {
    return ipcRenderer.invoke(CHANNELS.ptyDetach, ...args)
  }

  ptyList(...args: IpcRequests['ptyList']['params']): Promise<IpcRequests['ptyList']['result']> {
    return ipcRenderer.invoke(CHANNELS.ptyList, ...args)
  }

  ptyIsAlive(...args: IpcRequests['ptyIsAlive']['params']): Promise<IpcRequests['ptyIsAlive']['result']> {
    return ipcRenderer.invoke(CHANNELS.ptyIsAlive, ...args)
  }

  // Git requests
  gitGetInfo(...args: IpcRequests['gitGetInfo']['params']): Promise<IpcRequests['gitGetInfo']['result']> {
    return ipcRenderer.invoke(CHANNELS.gitGetInfo, ...args)
  }

  gitCreateWorktree(
    ...args: IpcRequests['gitCreateWorktree']['params']
  ): Promise<IpcRequests['gitCreateWorktree']['result']> {
    return ipcRenderer.invoke(CHANNELS.gitCreateWorktree, ...args)
  }

  gitRemoveWorktree(
    ...args: IpcRequests['gitRemoveWorktree']['params']
  ): Promise<IpcRequests['gitRemoveWorktree']['result']> {
    return ipcRenderer.invoke(CHANNELS.gitRemoveWorktree, ...args)
  }

  gitListWorktrees(
    ...args: IpcRequests['gitListWorktrees']['params']
  ): Promise<IpcRequests['gitListWorktrees']['result']> {
    return ipcRenderer.invoke(CHANNELS.gitListWorktrees, ...args)
  }

  gitGetChildWorktrees(
    ...args: IpcRequests['gitGetChildWorktrees']['params']
  ): Promise<IpcRequests['gitGetChildWorktrees']['result']> {
    return ipcRenderer.invoke(CHANNELS.gitGetChildWorktrees, ...args)
  }

  gitListLocalBranches(
    ...args: IpcRequests['gitListLocalBranches']['params']
  ): Promise<IpcRequests['gitListLocalBranches']['result']> {
    return ipcRenderer.invoke(CHANNELS.gitListLocalBranches, ...args)
  }

  gitListRemoteBranches(
    ...args: IpcRequests['gitListRemoteBranches']['params']
  ): Promise<IpcRequests['gitListRemoteBranches']['result']> {
    return ipcRenderer.invoke(CHANNELS.gitListRemoteBranches, ...args)
  }

  gitGetBranchesInWorktrees(
    ...args: IpcRequests['gitGetBranchesInWorktrees']['params']
  ): Promise<IpcRequests['gitGetBranchesInWorktrees']['result']> {
    return ipcRenderer.invoke(CHANNELS.gitGetBranchesInWorktrees, ...args)
  }

  gitCreateWorktreeFromBranch(
    ...args: IpcRequests['gitCreateWorktreeFromBranch']['params']
  ): Promise<IpcRequests['gitCreateWorktreeFromBranch']['result']> {
    return ipcRenderer.invoke(CHANNELS.gitCreateWorktreeFromBranch, ...args)
  }

  gitCreateWorktreeFromRemote(
    ...args: IpcRequests['gitCreateWorktreeFromRemote']['params']
  ): Promise<IpcRequests['gitCreateWorktreeFromRemote']['result']> {
    return ipcRenderer.invoke(CHANNELS.gitCreateWorktreeFromRemote, ...args)
  }

  gitGetDiff(...args: IpcRequests['gitGetDiff']['params']): Promise<IpcRequests['gitGetDiff']['result']> {
    return ipcRenderer.invoke(CHANNELS.gitGetDiff, ...args)
  }

  gitGetFileDiff(
    ...args: IpcRequests['gitGetFileDiff']['params']
  ): Promise<IpcRequests['gitGetFileDiff']['result']> {
    return ipcRenderer.invoke(CHANNELS.gitGetFileDiff, ...args)
  }

  gitMerge(...args: IpcRequests['gitMerge']['params']): Promise<IpcRequests['gitMerge']['result']> {
    return ipcRenderer.invoke(CHANNELS.gitMerge, ...args)
  }

  gitCheckMergeConflicts(
    ...args: IpcRequests['gitCheckMergeConflicts']['params']
  ): Promise<IpcRequests['gitCheckMergeConflicts']['result']> {
    return ipcRenderer.invoke(CHANNELS.gitCheckMergeConflicts, ...args)
  }

  gitHasUncommittedChanges(
    ...args: IpcRequests['gitHasUncommittedChanges']['params']
  ): Promise<IpcRequests['gitHasUncommittedChanges']['result']> {
    return ipcRenderer.invoke(CHANNELS.gitHasUncommittedChanges, ...args)
  }

  gitCommitAll(...args: IpcRequests['gitCommitAll']['params']): Promise<IpcRequests['gitCommitAll']['result']> {
    return ipcRenderer.invoke(CHANNELS.gitCommitAll, ...args)
  }

  gitDeleteBranch(
    ...args: IpcRequests['gitDeleteBranch']['params']
  ): Promise<IpcRequests['gitDeleteBranch']['result']> {
    return ipcRenderer.invoke(CHANNELS.gitDeleteBranch, ...args)
  }

  gitGetUncommittedChanges(
    ...args: IpcRequests['gitGetUncommittedChanges']['params']
  ): Promise<IpcRequests['gitGetUncommittedChanges']['result']> {
    return ipcRenderer.invoke(CHANNELS.gitGetUncommittedChanges, ...args)
  }

  gitGetUncommittedFileDiff(
    ...args: IpcRequests['gitGetUncommittedFileDiff']['params']
  ): Promise<IpcRequests['gitGetUncommittedFileDiff']['result']> {
    return ipcRenderer.invoke(CHANNELS.gitGetUncommittedFileDiff, ...args)
  }

  gitStageFile(...args: IpcRequests['gitStageFile']['params']): Promise<IpcRequests['gitStageFile']['result']> {
    return ipcRenderer.invoke(CHANNELS.gitStageFile, ...args)
  }

  gitUnstageFile(
    ...args: IpcRequests['gitUnstageFile']['params']
  ): Promise<IpcRequests['gitUnstageFile']['result']> {
    return ipcRenderer.invoke(CHANNELS.gitUnstageFile, ...args)
  }

  gitStageAll(...args: IpcRequests['gitStageAll']['params']): Promise<IpcRequests['gitStageAll']['result']> {
    return ipcRenderer.invoke(CHANNELS.gitStageAll, ...args)
  }

  gitUnstageAll(...args: IpcRequests['gitUnstageAll']['params']): Promise<IpcRequests['gitUnstageAll']['result']> {
    return ipcRenderer.invoke(CHANNELS.gitUnstageAll, ...args)
  }

  gitCommitStaged(
    ...args: IpcRequests['gitCommitStaged']['params']
  ): Promise<IpcRequests['gitCommitStaged']['result']> {
    return ipcRenderer.invoke(CHANNELS.gitCommitStaged, ...args)
  }

  gitGetFileContentsForDiff(
    ...args: IpcRequests['gitGetFileContentsForDiff']['params']
  ): Promise<IpcRequests['gitGetFileContentsForDiff']['result']> {
    return ipcRenderer.invoke(CHANNELS.gitGetFileContentsForDiff, ...args)
  }

  gitGetUncommittedFileContentsForDiff(
    ...args: IpcRequests['gitGetUncommittedFileContentsForDiff']['params']
  ): Promise<IpcRequests['gitGetUncommittedFileContentsForDiff']['result']> {
    return ipcRenderer.invoke(CHANNELS.gitGetUncommittedFileContentsForDiff, ...args)
  }

  gitGetHeadCommitHash(
    ...args: IpcRequests['gitGetHeadCommitHash']['params']
  ): Promise<IpcRequests['gitGetHeadCommitHash']['result']> {
    return ipcRenderer.invoke(CHANNELS.gitGetHeadCommitHash, ...args)
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

  // Run Actions requests
  runActionsDetect(
    ...args: IpcRequests['runActionsDetect']['params']
  ): Promise<IpcRequests['runActionsDetect']['result']> {
    return ipcRenderer.invoke(CHANNELS.runActionsDetect, ...args)
  }

  runActionsRun(
    ...args: IpcRequests['runActionsRun']['params']
  ): Promise<IpcRequests['runActionsRun']['result']> {
    return ipcRenderer.invoke(CHANNELS.runActionsRun, ...args)
  }

  // STT requests
  sttTranscribeOpenai(
    ...args: IpcRequests['sttTranscribeOpenai']['params']
  ): Promise<IpcRequests['sttTranscribeOpenai']['result']> {
    return ipcRenderer.invoke(CHANNELS.sttTranscribeOpenai, ...args)
  }

  sttTranscribeLocal(
    ...args: IpcRequests['sttTranscribeLocal']['params']
  ): Promise<IpcRequests['sttTranscribeLocal']['result']> {
    return ipcRenderer.invoke(CHANNELS.sttTranscribeLocal, ...args)
  }

  sttCheckMicPermission(
    ...args: IpcRequests['sttCheckMicPermission']['params']
  ): Promise<IpcRequests['sttCheckMicPermission']['result']> {
    return ipcRenderer.invoke(CHANNELS.sttCheckMicPermission, ...args)
  }

  // Session requests
  sessionCreate(...args: IpcRequests['sessionCreate']['params']): Promise<IpcRequests['sessionCreate']['result']> {
    return ipcRenderer.invoke(CHANNELS.sessionCreate, ...args)
  }

  sessionUpdate(...args: IpcRequests['sessionUpdate']['params']): Promise<IpcRequests['sessionUpdate']['result']> {
    return ipcRenderer.invoke(CHANNELS.sessionUpdate, ...args)
  }

  sessionList(...args: IpcRequests['sessionList']['params']): Promise<IpcRequests['sessionList']['result']> {
    return ipcRenderer.invoke(CHANNELS.sessionList, ...args)
  }

  sessionGet(...args: IpcRequests['sessionGet']['params']): Promise<IpcRequests['sessionGet']['result']> {
    return ipcRenderer.invoke(CHANNELS.sessionGet, ...args)
  }

  sessionDelete(...args: IpcRequests['sessionDelete']['params']): Promise<IpcRequests['sessionDelete']['result']> {
    return ipcRenderer.invoke(CHANNELS.sessionDelete, ...args)
  }

  sessionOpenInNewWindow(...args: IpcRequests['sessionOpenInNewWindow']['params']): Promise<IpcRequests['sessionOpenInNewWindow']['result']> {
    return ipcRenderer.invoke(CHANNELS.sessionOpenInNewWindow, ...args)
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

  // ==================== Event Listeners (on pattern, returns unsubscribe function) ====================

  onPtyData(callback: (...args: IpcEvents['ptyData']['params']) => void): () => void {
    const handler = (_event: IpcRendererEvent, ...args: unknown[]) =>
      callback(...(args as IpcEvents['ptyData']['params']))
    ipcRenderer.on(CHANNELS.ptyData, handler)
    return () => ipcRenderer.removeListener(CHANNELS.ptyData, handler)
  }

  onPtyExit(callback: (...args: IpcEvents['ptyExit']['params']) => void): () => void {
    const handler = (_event: IpcRendererEvent, ...args: unknown[]) =>
      callback(...(args as IpcEvents['ptyExit']['params']))
    ipcRenderer.on(CHANNELS.ptyExit, handler)
    return () => ipcRenderer.removeListener(CHANNELS.ptyExit, handler)
  }

  onSettingsOpen(callback: () => void): () => void {
    const handler = () => callback()
    ipcRenderer.on(CHANNELS.settingsOpen, handler)
    return () => ipcRenderer.removeListener(CHANNELS.settingsOpen, handler)
  }

  onAppConfirmClose(callback: () => void): () => void {
    const handler = () => callback()
    ipcRenderer.on(CHANNELS.appConfirmClose, handler)
    return () => ipcRenderer.removeListener(CHANNELS.appConfirmClose, handler)
  }

  onAppReady(callback: (...args: IpcEvents['appReady']['params']) => void): () => void {
    const handler = (_event: IpcRendererEvent, ...args: unknown[]) =>
      callback(...(args as IpcEvents['appReady']['params']))
    ipcRenderer.on(CHANNELS.appReady, handler)
    return () => ipcRenderer.removeListener(CHANNELS.appReady, handler)
  }

  onCapsLockEvent(callback: (...args: IpcEvents['capsLockEvent']['params']) => void): () => void {
    const handler = (_event: IpcRendererEvent, ...args: unknown[]) =>
      callback(...(args as IpcEvents['capsLockEvent']['params']))
    ipcRenderer.on(CHANNELS.capsLockEvent, handler)
    return () => ipcRenderer.removeListener(CHANNELS.capsLockEvent, handler)
  }

  onDaemonSessions(callback: (...args: IpcEvents['daemonSessions']['params']) => void): () => void {
    const handler = (_event: IpcRendererEvent, ...args: unknown[]) =>
      callback(...(args as IpcEvents['daemonSessions']['params']))
    ipcRenderer.on(CHANNELS.daemonSessions, handler)
    return () => ipcRenderer.removeListener(CHANNELS.daemonSessions, handler)
  }

  onTerminalNew(callback: () => void): () => void {
    const handler = () => callback()
    ipcRenderer.on(CHANNELS.terminalNew, handler)
    return () => ipcRenderer.removeListener(CHANNELS.terminalNew, handler)
  }

  onTerminalShowSessions(callback: () => void): () => void {
    const handler = () => callback()
    ipcRenderer.on(CHANNELS.terminalShowSessions, handler)
    return () => ipcRenderer.removeListener(CHANNELS.terminalShowSessions, handler)
  }

  onSessionShowSessions(callback: () => void): () => void {
    const handler = () => callback()
    ipcRenderer.on(CHANNELS.sessionShowSessions, handler)
    return () => ipcRenderer.removeListener(CHANNELS.sessionShowSessions, handler)
  }

  onSessionSync(callback: (...args: IpcEvents['sessionSync']['params']) => void): () => void {
    const handler = (_event: IpcRendererEvent, ...args: unknown[]) =>
      callback(...(args as IpcEvents['sessionSync']['params']))
    ipcRenderer.on(CHANNELS.sessionSync, handler)
    return () => ipcRenderer.removeListener(CHANNELS.sessionSync, handler)
  }

  onDaemonDisconnected(callback: () => void): () => void {
    const handler = () => callback()
    ipcRenderer.on(CHANNELS.daemonDisconnected, handler)
    return () => ipcRenderer.removeListener(CHANNELS.daemonDisconnected, handler)
  }

  onActiveProcessesOpen(callback: () => void): () => void {
    const handler = () => callback()
    ipcRenderer.on(CHANNELS.activeProcessesOpen, handler)
    return () => ipcRenderer.removeListener(CHANNELS.activeProcessesOpen, handler)
  }
}
