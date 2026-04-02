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
  sshGetOutput: 'ssh:getOutput',
  sshWatchOutput: 'ssh:watchOutput',
  sshUnwatchOutput: 'ssh:unwatchOutput',
  sshWatchConnectionStatus: 'ssh:watchConnectionStatus',
  sshUnwatchConnectionStatus: 'ssh:unwatchConnectionStatus',
  sshAddPortForward: 'ssh:addPortForward',
  sshRemovePortForward: 'ssh:removePortForward',
  sshListPortForwards: 'ssh:listPortForwards',
  sshWatchPortForwardOutput: 'ssh:watchPortForwardOutput',
  sshUnwatchPortForwardOutput: 'ssh:unwatchPortForwardOutput',

  // LLM operations
  llmChatSend: 'llm:chat:send',
  llmAnalyzeTerminal: 'llm:analyzeTerminal',
  llmClearAnalyzerCache: 'llm:clearAnalyzerCache',
  llmGenerateTitle: 'llm:generateTitle',

  // Clipboard operations
  clipboardReadText: 'clipboard:readText',
  clipboardWriteText: 'clipboard:writeText',

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
  sshOutput: 'ssh:output',
  sshPortForwardStatus: 'ssh:portForwardStatus',
  sshPortForwardOutput: 'ssh:portForwardOutput',
  llmChatDelta: 'llm:chat:delta',
  llmChatDone: 'llm:chat:done',
  llmChatError: 'llm:chat:error',
  gitOutput: 'git:output'
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
      handler(...(args as IpcSends['ptyWrite']['params']))
    )
  }

  onPtyResize(handler: (...args: IpcSends['ptyResize']['params']) => void): void {
    ipcMain.on(CHANNELS.ptyResize, (_event: IpcMainEvent, ...args: unknown[]) =>
      handler(...(args as IpcSends['ptyResize']['params']))
    )
  }

  onPtyKill(handler: (...args: IpcSends['ptyKill']['params']) => void): void {
    ipcMain.on(CHANNELS.ptyKill, (_event: IpcMainEvent, ...args: unknown[]) =>
      handler(...(args as IpcSends['ptyKill']['params']))
    )
  }

  // Git request handlers
  onGitGetInfo(
    handler: (
      ...args: IpcRequests['gitGetInfo']['params']
    ) => IpcRequests['gitGetInfo']['result'] | Promise<IpcRequests['gitGetInfo']['result']>
  ): void {
    ipcMain.handle(CHANNELS.gitGetInfo, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['gitGetInfo']['params']))
    )
  }

  onGitCreateWorktree(
    handler: (
      ...args: IpcRequests['gitCreateWorktree']['params']
    ) => IpcRequests['gitCreateWorktree']['result'] | Promise<IpcRequests['gitCreateWorktree']['result']>
  ): void {
    ipcMain.handle(CHANNELS.gitCreateWorktree, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['gitCreateWorktree']['params']))
    )
  }

  onGitRemoveWorktree(
    handler: (
      ...args: IpcRequests['gitRemoveWorktree']['params']
    ) => IpcRequests['gitRemoveWorktree']['result'] | Promise<IpcRequests['gitRemoveWorktree']['result']>
  ): void {
    ipcMain.handle(CHANNELS.gitRemoveWorktree, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['gitRemoveWorktree']['params']))
    )
  }

  onGitListWorktrees(
    handler: (
      ...args: IpcRequests['gitListWorktrees']['params']
    ) => IpcRequests['gitListWorktrees']['result'] | Promise<IpcRequests['gitListWorktrees']['result']>
  ): void {
    ipcMain.handle(CHANNELS.gitListWorktrees, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['gitListWorktrees']['params']))
    )
  }


  onGitListLocalBranches(
    handler: (
      ...args: IpcRequests['gitListLocalBranches']['params']
    ) => IpcRequests['gitListLocalBranches']['result'] | Promise<IpcRequests['gitListLocalBranches']['result']>
  ): void {
    ipcMain.handle(CHANNELS.gitListLocalBranches, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['gitListLocalBranches']['params']))
    )
  }

  onGitListRemoteBranches(
    handler: (
      ...args: IpcRequests['gitListRemoteBranches']['params']
    ) => IpcRequests['gitListRemoteBranches']['result'] | Promise<IpcRequests['gitListRemoteBranches']['result']>
  ): void {
    ipcMain.handle(CHANNELS.gitListRemoteBranches, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['gitListRemoteBranches']['params']))
    )
  }

  onGitGetBranchesInWorktrees(
    handler: (
      ...args: IpcRequests['gitGetBranchesInWorktrees']['params']
    ) => IpcRequests['gitGetBranchesInWorktrees']['result'] | Promise<IpcRequests['gitGetBranchesInWorktrees']['result']>
  ): void {
    ipcMain.handle(CHANNELS.gitGetBranchesInWorktrees, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['gitGetBranchesInWorktrees']['params']))
    )
  }

  onGitCreateWorktreeFromBranch(
    handler: (
      ...args: IpcRequests['gitCreateWorktreeFromBranch']['params']
    ) => IpcRequests['gitCreateWorktreeFromBranch']['result'] | Promise<IpcRequests['gitCreateWorktreeFromBranch']['result']>
  ): void {
    ipcMain.handle(CHANNELS.gitCreateWorktreeFromBranch, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['gitCreateWorktreeFromBranch']['params']))
    )
  }

  onGitCreateWorktreeFromRemote(
    handler: (
      ...args: IpcRequests['gitCreateWorktreeFromRemote']['params']
    ) => IpcRequests['gitCreateWorktreeFromRemote']['result'] | Promise<IpcRequests['gitCreateWorktreeFromRemote']['result']>
  ): void {
    ipcMain.handle(CHANNELS.gitCreateWorktreeFromRemote, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['gitCreateWorktreeFromRemote']['params']))
    )
  }

  onGitGetDiff(
    handler: (
      ...args: IpcRequests['gitGetDiff']['params']
    ) => IpcRequests['gitGetDiff']['result'] | Promise<IpcRequests['gitGetDiff']['result']>
  ): void {
    ipcMain.handle(CHANNELS.gitGetDiff, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['gitGetDiff']['params']))
    )
  }

  onGitGetFileDiff(
    handler: (
      ...args: IpcRequests['gitGetFileDiff']['params']
    ) => IpcRequests['gitGetFileDiff']['result'] | Promise<IpcRequests['gitGetFileDiff']['result']>
  ): void {
    ipcMain.handle(CHANNELS.gitGetFileDiff, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['gitGetFileDiff']['params']))
    )
  }

  onGitMerge(
    handler: (
      ...args: IpcRequests['gitMerge']['params']
    ) => IpcRequests['gitMerge']['result'] | Promise<IpcRequests['gitMerge']['result']>
  ): void {
    ipcMain.handle(CHANNELS.gitMerge, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['gitMerge']['params']))
    )
  }

  onGitCheckMergeConflicts(
    handler: (
      ...args: IpcRequests['gitCheckMergeConflicts']['params']
    ) => IpcRequests['gitCheckMergeConflicts']['result'] | Promise<IpcRequests['gitCheckMergeConflicts']['result']>
  ): void {
    ipcMain.handle(CHANNELS.gitCheckMergeConflicts, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['gitCheckMergeConflicts']['params']))
    )
  }

  onGitHasUncommittedChanges(
    handler: (
      ...args: IpcRequests['gitHasUncommittedChanges']['params']
    ) => IpcRequests['gitHasUncommittedChanges']['result'] | Promise<IpcRequests['gitHasUncommittedChanges']['result']>
  ): void {
    ipcMain.handle(CHANNELS.gitHasUncommittedChanges, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['gitHasUncommittedChanges']['params']))
    )
  }

  onGitCommitAll(
    handler: (
      ...args: IpcRequests['gitCommitAll']['params']
    ) => IpcRequests['gitCommitAll']['result'] | Promise<IpcRequests['gitCommitAll']['result']>
  ): void {
    ipcMain.handle(CHANNELS.gitCommitAll, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['gitCommitAll']['params']))
    )
  }

  onGitDeleteBranch(
    handler: (
      ...args: IpcRequests['gitDeleteBranch']['params']
    ) => IpcRequests['gitDeleteBranch']['result'] | Promise<IpcRequests['gitDeleteBranch']['result']>
  ): void {
    ipcMain.handle(CHANNELS.gitDeleteBranch, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['gitDeleteBranch']['params']))
    )
  }

  onGitRenameBranch(
    handler: (
      ...args: IpcRequests['gitRenameBranch']['params']
    ) => IpcRequests['gitRenameBranch']['result'] | Promise<IpcRequests['gitRenameBranch']['result']>
  ): void {
    ipcMain.handle(CHANNELS.gitRenameBranch, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['gitRenameBranch']['params']))
    )
  }

  onGitGetUncommittedChanges(
    handler: (
      ...args: IpcRequests['gitGetUncommittedChanges']['params']
    ) => IpcRequests['gitGetUncommittedChanges']['result'] | Promise<IpcRequests['gitGetUncommittedChanges']['result']>
  ): void {
    ipcMain.handle(CHANNELS.gitGetUncommittedChanges, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['gitGetUncommittedChanges']['params']))
    )
  }

  onGitGetUncommittedFileDiff(
    handler: (
      ...args: IpcRequests['gitGetUncommittedFileDiff']['params']
    ) => IpcRequests['gitGetUncommittedFileDiff']['result'] | Promise<IpcRequests['gitGetUncommittedFileDiff']['result']>
  ): void {
    ipcMain.handle(CHANNELS.gitGetUncommittedFileDiff, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['gitGetUncommittedFileDiff']['params']))
    )
  }

  onGitStageFile(
    handler: (
      ...args: IpcRequests['gitStageFile']['params']
    ) => IpcRequests['gitStageFile']['result'] | Promise<IpcRequests['gitStageFile']['result']>
  ): void {
    ipcMain.handle(CHANNELS.gitStageFile, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['gitStageFile']['params']))
    )
  }

  onGitUnstageFile(
    handler: (
      ...args: IpcRequests['gitUnstageFile']['params']
    ) => IpcRequests['gitUnstageFile']['result'] | Promise<IpcRequests['gitUnstageFile']['result']>
  ): void {
    ipcMain.handle(CHANNELS.gitUnstageFile, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['gitUnstageFile']['params']))
    )
  }

  onGitStageAll(
    handler: (
      ...args: IpcRequests['gitStageAll']['params']
    ) => IpcRequests['gitStageAll']['result'] | Promise<IpcRequests['gitStageAll']['result']>
  ): void {
    ipcMain.handle(CHANNELS.gitStageAll, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['gitStageAll']['params']))
    )
  }

  onGitUnstageAll(
    handler: (
      ...args: IpcRequests['gitUnstageAll']['params']
    ) => IpcRequests['gitUnstageAll']['result'] | Promise<IpcRequests['gitUnstageAll']['result']>
  ): void {
    ipcMain.handle(CHANNELS.gitUnstageAll, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['gitUnstageAll']['params']))
    )
  }

  onGitCommitStaged(
    handler: (
      ...args: IpcRequests['gitCommitStaged']['params']
    ) => IpcRequests['gitCommitStaged']['result'] | Promise<IpcRequests['gitCommitStaged']['result']>
  ): void {
    ipcMain.handle(CHANNELS.gitCommitStaged, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['gitCommitStaged']['params']))
    )
  }

  onGitGetFileContentsForDiff(
    handler: (
      ...args: IpcRequests['gitGetFileContentsForDiff']['params']
    ) => IpcRequests['gitGetFileContentsForDiff']['result'] | Promise<IpcRequests['gitGetFileContentsForDiff']['result']>
  ): void {
    ipcMain.handle(CHANNELS.gitGetFileContentsForDiff, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['gitGetFileContentsForDiff']['params']))
    )
  }

  onGitGetUncommittedFileContentsForDiff(
    handler: (
      ...args: IpcRequests['gitGetUncommittedFileContentsForDiff']['params']
    ) => IpcRequests['gitGetUncommittedFileContentsForDiff']['result'] | Promise<IpcRequests['gitGetUncommittedFileContentsForDiff']['result']>
  ): void {
    ipcMain.handle(CHANNELS.gitGetUncommittedFileContentsForDiff, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['gitGetUncommittedFileContentsForDiff']['params']))
    )
  }

  onGitGetHeadCommitHash(
    handler: (
      ...args: IpcRequests['gitGetHeadCommitHash']['params']
    ) => IpcRequests['gitGetHeadCommitHash']['result'] | Promise<IpcRequests['gitGetHeadCommitHash']['result']>
  ): void {
    ipcMain.handle(CHANNELS.gitGetHeadCommitHash, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['gitGetHeadCommitHash']['params']))
    )
  }

  onGitGetLog(
    handler: (
      ...args: IpcRequests['gitGetLog']['params']
    ) => IpcRequests['gitGetLog']['result'] | Promise<IpcRequests['gitGetLog']['result']>
  ): void {
    ipcMain.handle(CHANNELS.gitGetLog, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['gitGetLog']['params']))
    )
  }

  onGitGetCommitDiff(
    handler: (
      ...args: IpcRequests['gitGetCommitDiff']['params']
    ) => IpcRequests['gitGetCommitDiff']['result'] | Promise<IpcRequests['gitGetCommitDiff']['result']>
  ): void {
    ipcMain.handle(CHANNELS.gitGetCommitDiff, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['gitGetCommitDiff']['params']))
    )
  }

  onGitGetCommitFileDiff(
    handler: (
      ...args: IpcRequests['gitGetCommitFileDiff']['params']
    ) => IpcRequests['gitGetCommitFileDiff']['result'] | Promise<IpcRequests['gitGetCommitFileDiff']['result']>
  ): void {
    ipcMain.handle(CHANNELS.gitGetCommitFileDiff, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['gitGetCommitFileDiff']['params']))
    )
  }

  // Git fetch/pull request handlers
  onGitFetch(
    handler: (
      ...args: IpcRequests['gitFetch']['params']
    ) => IpcRequests['gitFetch']['result'] | Promise<IpcRequests['gitFetch']['result']>
  ): void {
    ipcMain.handle(CHANNELS.gitFetch, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['gitFetch']['params']))
    )
  }

  onGitPull(
    handler: (
      ...args: IpcRequests['gitPull']['params']
    ) => IpcRequests['gitPull']['result'] | Promise<IpcRequests['gitPull']['result']>
  ): void {
    ipcMain.handle(CHANNELS.gitPull, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['gitPull']['params']))
    )
  }

  onGitGetBehindCount(
    handler: (
      ...args: IpcRequests['gitGetBehindCount']['params']
    ) => IpcRequests['gitGetBehindCount']['result'] | Promise<IpcRequests['gitGetBehindCount']['result']>
  ): void {
    ipcMain.handle(CHANNELS.gitGetBehindCount, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['gitGetBehindCount']['params']))
    )
  }

  // GitHub request handlers
  onGitGetRemoteUrl(
    handler: (
      ...args: IpcRequests['gitGetRemoteUrl']['params']
    ) => IpcRequests['gitGetRemoteUrl']['result'] | Promise<IpcRequests['gitGetRemoteUrl']['result']>
  ): void {
    ipcMain.handle(CHANNELS.gitGetRemoteUrl, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['gitGetRemoteUrl']['params']))
    )
  }

  onGithubGetPrInfo(
    handler: (
      ...args: IpcRequests['githubGetPrInfo']['params']
    ) => IpcRequests['githubGetPrInfo']['result'] | Promise<IpcRequests['githubGetPrInfo']['result']>
  ): void {
    ipcMain.handle(CHANNELS.githubGetPrInfo, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['githubGetPrInfo']['params']))
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

  // LLM request handlers
  onLlmChatSend(
    handler: (
      event: IpcMainInvokeEvent,
      ...args: IpcRequests['llmChatSend']['params']
    ) => IpcRequests['llmChatSend']['result'] | Promise<IpcRequests['llmChatSend']['result']>
  ): void {
    ipcMain.handle(CHANNELS.llmChatSend, (event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(event, ...(args as IpcRequests['llmChatSend']['params']))
    )
  }

  onLlmAnalyzeTerminal(
    handler: (
      ...args: IpcRequests['llmAnalyzeTerminal']['params']
    ) => IpcRequests['llmAnalyzeTerminal']['result'] | Promise<IpcRequests['llmAnalyzeTerminal']['result']>
  ): void {
    ipcMain.handle(CHANNELS.llmAnalyzeTerminal, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['llmAnalyzeTerminal']['params']))
    )
  }

  onLlmClearAnalyzerCache(
    handler: (
      ...args: IpcRequests['llmClearAnalyzerCache']['params']
    ) => IpcRequests['llmClearAnalyzerCache']['result'] | Promise<IpcRequests['llmClearAnalyzerCache']['result']>
  ): void {
    ipcMain.handle(CHANNELS.llmClearAnalyzerCache, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['llmClearAnalyzerCache']['params']))
    )
  }

  onLlmGenerateTitle(
    handler: (
      ...args: IpcRequests['llmGenerateTitle']['params']
    ) => IpcRequests['llmGenerateTitle']['result'] | Promise<IpcRequests['llmGenerateTitle']['result']>
  ): void {
    ipcMain.handle(CHANNELS.llmGenerateTitle, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['llmGenerateTitle']['params']))
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

  // Run Actions request handlers
  onRunActionsDetect(
    handler: (
      ...args: IpcRequests['runActionsDetect']['params']
    ) => IpcRequests['runActionsDetect']['result'] | Promise<IpcRequests['runActionsDetect']['result']>
  ): void {
    ipcMain.handle(CHANNELS.runActionsDetect, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['runActionsDetect']['params']))
    )
  }

  onRunActionsRun(
    handler: (
      ...args: IpcRequests['runActionsRun']['params']
    ) => IpcRequests['runActionsRun']['result'] | Promise<IpcRequests['runActionsRun']['result']>
  ): void {
    ipcMain.handle(CHANNELS.runActionsRun, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['runActionsRun']['params']))
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

  onSshGetOutput(
    handler: (
      ...args: IpcRequests['sshGetOutput']['params']
    ) => IpcRequests['sshGetOutput']['result'] | Promise<IpcRequests['sshGetOutput']['result']>
  ): void {
    ipcMain.handle(CHANNELS.sshGetOutput, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['sshGetOutput']['params']))
    )
  }

  onSshWatchOutput(
    handler: (
      event: IpcMainInvokeEvent,
      ...args: IpcRequests['sshWatchOutput']['params']
    ) => IpcRequests['sshWatchOutput']['result'] | Promise<IpcRequests['sshWatchOutput']['result']>
  ): void {
    ipcMain.handle(CHANNELS.sshWatchOutput, (event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(event, ...(args as IpcRequests['sshWatchOutput']['params']))
    )
  }

  onSshUnwatchOutput(
    handler: (
      event: IpcMainInvokeEvent,
      ...args: IpcRequests['sshUnwatchOutput']['params']
    ) => IpcRequests['sshUnwatchOutput']['result'] | Promise<IpcRequests['sshUnwatchOutput']['result']>
  ): void {
    ipcMain.handle(CHANNELS.sshUnwatchOutput, (event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(event, ...(args as IpcRequests['sshUnwatchOutput']['params']))
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
    ipcMain.on(CHANNELS.appCloseConfirmed, (event: IpcMainEvent) => handler(event))
  }

  onAppCloseCancelled(handler: (event: IpcMainEvent) => void): void {
    ipcMain.on(CHANNELS.appCloseCancelled, (event: IpcMainEvent) => handler(event))
  }

  onLlmChatCancel(handler: (...args: IpcSends['llmChatCancel']['params']) => void): void {
    ipcMain.on(CHANNELS.llmChatCancel, (_event: IpcMainEvent, ...args: unknown[]) =>
      handler(...(args as IpcSends['llmChatCancel']['params']))
    )
  }

  onClipboardWriteText(handler: (...args: IpcSends['clipboardWriteText']['params']) => void): void {
    ipcMain.on(CHANNELS.clipboardWriteText, (_event: IpcMainEvent, ...args: unknown[]) =>
      handler(...(args as IpcSends['clipboardWriteText']['params']))
    )
  }

  onClipboardReadText(
    handler: () => IpcRequests['clipboardReadText']['result'] | Promise<IpcRequests['clipboardReadText']['result']>
  ): void {
    ipcMain.handle(CHANNELS.clipboardReadText, () => handler())
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

  daemonDisconnected(): void {
    this.window?.webContents.send(CHANNELS.daemonDisconnected)
  }

  activeProcessesOpen(): void {
    this.window?.webContents.send(CHANNELS.activeProcessesOpen)
  }

  sshConnectionStatus(...args: IpcEvents['sshConnectionStatus']['params']): void {
    this.window?.webContents.send(CHANNELS.sshConnectionStatus, ...args)
  }

  sshOutput(...args: IpcEvents['sshOutput']['params']): void {
    this.window?.webContents.send(CHANNELS.sshOutput, ...args)
  }

  sshPortForwardStatus(...args: IpcEvents['sshPortForwardStatus']['params']): void {
    this.window?.webContents.send(CHANNELS.sshPortForwardStatus, ...args)
  }

  sshPortForwardOutput(...args: IpcEvents['sshPortForwardOutput']['params']): void {
    this.window?.webContents.send(CHANNELS.sshPortForwardOutput, ...args)
  }

  gitOutput(...args: IpcEvents['gitOutput']['params']): void {
    this.window?.webContents.send(CHANNELS.gitOutput, ...args)
  }
}
