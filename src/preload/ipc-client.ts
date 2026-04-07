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
  gitGetInfo: 'git:getInfo',
  gitCreateWorktree: 'git:createWorktree',
  gitRemoveWorktree: 'git:removeWorktree',
  gitListWorktrees: 'git:listWorktrees',
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
  gitRenameBranch: 'git:renameBranch',
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
  gitGetLog: 'git:getLog',
  gitGetCommitDiff: 'git:getCommitDiff',
  gitGetCommitFileDiff: 'git:getCommitFileDiff',
  gitFetch: 'git:fetch',
  gitPull: 'git:pull',
  gitGetBehindCount: 'git:getBehindCount',
  gitGetRemoteUrl: 'git:getRemoteUrl',
  githubGetPrInfo: 'github:getPrInfo',
  settingsLoad: 'settings:load',
  settingsSave: 'settings:save',
  fsReadDirectory: 'fs:readDirectory',
  fsReadFile: 'fs:readFile',
  fsWriteFile: 'fs:writeFile',
  fsSearchFiles: 'fs:searchFiles',
  runActionsDetect: 'runActions:detect',
  runActionsRun: 'runActions:run',
  sessionUpdate: 'session:update',
  daemonShutdown: 'daemon:shutdown',
  dialogSelectFolder: 'dialog:selectFolder',
  dialogGetRecentDirectories: 'dialog:getRecentDirectories',
  sandboxIsAvailable: 'sandbox:isAvailable',
  appGetInitialWorkspace: 'app:getInitialWorkspace',
  appGetWindowUuid: 'app:getWindowUuid',
  sshConnect: 'ssh:connect',
  sshDisconnect: 'ssh:disconnect',
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
  llmChatSend: 'llm:chat:send',
  llmAnalyzeTerminal: 'llm:analyzeTerminal',
  llmClearAnalyzerCache: 'llm:clearAnalyzerCache',
  llmGenerateTitle: 'llm:generateTitle',

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
  llmChatCancel: 'llm:chat:cancel',

  // Event channels
  ptyEvent: 'pty:event',
  settingsOpen: 'settings:open',
  appConfirmClose: 'app:confirm-close',
  appReady: 'app:ready',
  capsLockEvent: 'capslock-event',
  daemonSessions: 'daemon:sessions',
  sessionSync: 'session:sync',
  sshAutoConnected: 'ssh:autoConnected',
  daemonDisconnected: 'daemon:disconnected',
  activeProcessesOpen: 'active-processes:open',
  sshConnectionStatus: 'ssh:connectionStatus',
  sshBootstrapOutput: 'ssh:bootstrapOutput',
  sshTunnelOutput: 'ssh:tunnelOutput',
  sshDaemonOutput: 'ssh:daemonOutput',
  sshPortForwardStatus: 'ssh:portForwardStatus',
  sshPortForwardOutput: 'ssh:portForwardOutput',
  llmChatDelta: 'llm:chat:delta',
  llmChatDone: 'llm:chat:done',
  llmChatError: 'llm:chat:error',
  gitOutput: 'git:output'
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

  gitRenameBranch(
    ...args: IpcRequests['gitRenameBranch']['params']
  ): Promise<IpcRequests['gitRenameBranch']['result']> {
    return ipcRenderer.invoke(CHANNELS.gitRenameBranch, ...args)
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

  gitGetLog(
    ...args: IpcRequests['gitGetLog']['params']
  ): Promise<IpcRequests['gitGetLog']['result']> {
    return ipcRenderer.invoke(CHANNELS.gitGetLog, ...args)
  }

  gitGetCommitDiff(
    ...args: IpcRequests['gitGetCommitDiff']['params']
  ): Promise<IpcRequests['gitGetCommitDiff']['result']> {
    return ipcRenderer.invoke(CHANNELS.gitGetCommitDiff, ...args)
  }

  gitGetCommitFileDiff(
    ...args: IpcRequests['gitGetCommitFileDiff']['params']
  ): Promise<IpcRequests['gitGetCommitFileDiff']['result']> {
    return ipcRenderer.invoke(CHANNELS.gitGetCommitFileDiff, ...args)
  }

  // Git fetch/pull requests
  gitFetch(...args: IpcRequests['gitFetch']['params']): Promise<IpcRequests['gitFetch']['result']> {
    return ipcRenderer.invoke(CHANNELS.gitFetch, ...args)
  }

  gitPull(...args: IpcRequests['gitPull']['params']): Promise<IpcRequests['gitPull']['result']> {
    return ipcRenderer.invoke(CHANNELS.gitPull, ...args)
  }

  gitGetBehindCount(...args: IpcRequests['gitGetBehindCount']['params']): Promise<IpcRequests['gitGetBehindCount']['result']> {
    return ipcRenderer.invoke(CHANNELS.gitGetBehindCount, ...args)
  }

  // GitHub requests
  gitGetRemoteUrl(
    ...args: IpcRequests['gitGetRemoteUrl']['params']
  ): Promise<IpcRequests['gitGetRemoteUrl']['result']> {
    return ipcRenderer.invoke(CHANNELS.gitGetRemoteUrl, ...args)
  }

  githubGetPrInfo(
    ...args: IpcRequests['githubGetPrInfo']['params']
  ): Promise<IpcRequests['githubGetPrInfo']['result']> {
    return ipcRenderer.invoke(CHANNELS.githubGetPrInfo, ...args)
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

  // Session requests
  sessionUpdate(...args: IpcRequests['sessionUpdate']['params']): Promise<IpcRequests['sessionUpdate']['result']> {
    return ipcRenderer.invoke(CHANNELS.sessionUpdate, ...args)
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

  // LLM requests
  llmChatSend(
    ...args: IpcRequests['llmChatSend']['params']
  ): Promise<IpcRequests['llmChatSend']['result']> {
    return ipcRenderer.invoke(CHANNELS.llmChatSend, ...args)
  }

  llmAnalyzeTerminal(
    ...args: IpcRequests['llmAnalyzeTerminal']['params']
  ): Promise<IpcRequests['llmAnalyzeTerminal']['result']> {
    return ipcRenderer.invoke(CHANNELS.llmAnalyzeTerminal, ...args)
  }

  llmClearAnalyzerCache(): Promise<void> {
    return ipcRenderer.invoke(CHANNELS.llmClearAnalyzerCache)
  }

  llmGenerateTitle(
    ...args: IpcRequests['llmGenerateTitle']['params']
  ): Promise<IpcRequests['llmGenerateTitle']['result']> {
    return ipcRenderer.invoke(CHANNELS.llmGenerateTitle, ...args)
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

  llmChatCancel(...args: IpcSends['llmChatCancel']['params']): void {
    ipcRenderer.send(CHANNELS.llmChatCancel, ...args)
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

  onLlmChatDelta(callback: (...args: IpcEvents['llmChatDelta']['params']) => void): () => void {
    const handler = (_event: IpcRendererEvent, ...args: unknown[]) =>
      { callback(...(args as IpcEvents['llmChatDelta']['params'])); }
    ipcRenderer.on(CHANNELS.llmChatDelta, handler)
    return () => ipcRenderer.removeListener(CHANNELS.llmChatDelta, handler)
  }

  onLlmChatDone(callback: (...args: IpcEvents['llmChatDone']['params']) => void): () => void {
    const handler = (_event: IpcRendererEvent, ...args: unknown[]) =>
      { callback(...(args as IpcEvents['llmChatDone']['params'])); }
    ipcRenderer.on(CHANNELS.llmChatDone, handler)
    return () => ipcRenderer.removeListener(CHANNELS.llmChatDone, handler)
  }

  onLlmChatError(callback: (...args: IpcEvents['llmChatError']['params']) => void): () => void {
    const handler = (_event: IpcRendererEvent, ...args: unknown[]) =>
      { callback(...(args as IpcEvents['llmChatError']['params'])); }
    ipcRenderer.on(CHANNELS.llmChatError, handler)
    return () => ipcRenderer.removeListener(CHANNELS.llmChatError, handler)
  }

  onGitOutput(callback: (...args: IpcEvents['gitOutput']['params']) => void): () => void {
    const handler = (_event: IpcRendererEvent, ...args: unknown[]) =>
      { callback(...(args as IpcEvents['gitOutput']['params'])); }
    ipcRenderer.on(CHANNELS.gitOutput, handler)
    return () => ipcRenderer.removeListener(CHANNELS.gitOutput, handler)
  }

  onExecEvent(callback: (...args: IpcEvents['execEvent']['params']) => void): () => void {
    const handler = (_event: IpcRendererEvent, ...args: unknown[]) =>
      { callback(...(args as IpcEvents['execEvent']['params'])); }
    ipcRenderer.on(CHANNELS.execEvent, handler)
    return () => ipcRenderer.removeListener(CHANNELS.execEvent, handler)
  }
}
