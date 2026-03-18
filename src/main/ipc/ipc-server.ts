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
  reviewsLoad: 'reviews:load',
  reviewsSave: 'reviews:save',
  reviewsAddComment: 'reviews:addComment',
  reviewsDeleteComment: 'reviews:deleteComment',
  reviewsUpdateOutdated: 'reviews:updateOutdated',
  reviewsToggleAddressed: 'reviews:toggleAddressed',
  reviewsGetFilePath: 'reviews:getFilePath',
  settingsLoad: 'settings:load',
  settingsSave: 'settings:save',
  fsReadDirectory: 'fs:readDirectory',
  fsReadFile: 'fs:readFile',
  fsWriteFile: 'fs:writeFile',
  fsSearchFiles: 'fs:searchFiles',
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
      ...args: IpcRequests['ptyCreate']['params']
    ) => IpcRequests['ptyCreate']['result'] | Promise<IpcRequests['ptyCreate']['result']>
  ): void {
    ipcMain.handle(CHANNELS.ptyCreate, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['ptyCreate']['params']))
    )
  }

  onPtyAttach(
    handler: (
      ...args: IpcRequests['ptyAttach']['params']
    ) => IpcRequests['ptyAttach']['result'] | Promise<IpcRequests['ptyAttach']['result']>
  ): void {
    ipcMain.handle(CHANNELS.ptyAttach, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['ptyAttach']['params']))
    )
  }

  onPtyDetach(
    handler: (
      ...args: IpcRequests['ptyDetach']['params']
    ) => IpcRequests['ptyDetach']['result'] | Promise<IpcRequests['ptyDetach']['result']>
  ): void {
    ipcMain.handle(CHANNELS.ptyDetach, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['ptyDetach']['params']))
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

  onPtyIsAlive(
    handler: (
      ...args: IpcRequests['ptyIsAlive']['params']
    ) => IpcRequests['ptyIsAlive']['result'] | Promise<IpcRequests['ptyIsAlive']['result']>
  ): void {
    ipcMain.handle(CHANNELS.ptyIsAlive, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['ptyIsAlive']['params']))
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

  onGitGetChildWorktrees(
    handler: (
      ...args: IpcRequests['gitGetChildWorktrees']['params']
    ) => IpcRequests['gitGetChildWorktrees']['result'] | Promise<IpcRequests['gitGetChildWorktrees']['result']>
  ): void {
    ipcMain.handle(CHANNELS.gitGetChildWorktrees, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['gitGetChildWorktrees']['params']))
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

  // Reviews request handlers
  onReviewsLoad(
    handler: (
      ...args: IpcRequests['reviewsLoad']['params']
    ) => IpcRequests['reviewsLoad']['result'] | Promise<IpcRequests['reviewsLoad']['result']>
  ): void {
    ipcMain.handle(CHANNELS.reviewsLoad, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['reviewsLoad']['params']))
    )
  }

  onReviewsSave(
    handler: (
      ...args: IpcRequests['reviewsSave']['params']
    ) => IpcRequests['reviewsSave']['result'] | Promise<IpcRequests['reviewsSave']['result']>
  ): void {
    ipcMain.handle(CHANNELS.reviewsSave, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['reviewsSave']['params']))
    )
  }

  onReviewsAddComment(
    handler: (
      ...args: IpcRequests['reviewsAddComment']['params']
    ) => IpcRequests['reviewsAddComment']['result'] | Promise<IpcRequests['reviewsAddComment']['result']>
  ): void {
    ipcMain.handle(CHANNELS.reviewsAddComment, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['reviewsAddComment']['params']))
    )
  }

  onReviewsDeleteComment(
    handler: (
      ...args: IpcRequests['reviewsDeleteComment']['params']
    ) => IpcRequests['reviewsDeleteComment']['result'] | Promise<IpcRequests['reviewsDeleteComment']['result']>
  ): void {
    ipcMain.handle(CHANNELS.reviewsDeleteComment, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['reviewsDeleteComment']['params']))
    )
  }

  onReviewsUpdateOutdated(
    handler: (
      ...args: IpcRequests['reviewsUpdateOutdated']['params']
    ) => IpcRequests['reviewsUpdateOutdated']['result'] | Promise<IpcRequests['reviewsUpdateOutdated']['result']>
  ): void {
    ipcMain.handle(CHANNELS.reviewsUpdateOutdated, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['reviewsUpdateOutdated']['params']))
    )
  }

  onReviewsToggleAddressed(
    handler: (
      ...args: IpcRequests['reviewsToggleAddressed']['params']
    ) => IpcRequests['reviewsToggleAddressed']['result'] | Promise<IpcRequests['reviewsToggleAddressed']['result']>
  ): void {
    ipcMain.handle(CHANNELS.reviewsToggleAddressed, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['reviewsToggleAddressed']['params']))
    )
  }

  onReviewsGetFilePath(
    handler: (
      ...args: IpcRequests['reviewsGetFilePath']['params']
    ) => IpcRequests['reviewsGetFilePath']['result'] | Promise<IpcRequests['reviewsGetFilePath']['result']>
  ): void {
    ipcMain.handle(CHANNELS.reviewsGetFilePath, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['reviewsGetFilePath']['params']))
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

  // STT request handlers
  onSttTranscribeOpenai(
    handler: (
      ...args: IpcRequests['sttTranscribeOpenai']['params']
    ) => IpcRequests['sttTranscribeOpenai']['result'] | Promise<IpcRequests['sttTranscribeOpenai']['result']>
  ): void {
    ipcMain.handle(CHANNELS.sttTranscribeOpenai, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['sttTranscribeOpenai']['params']))
    )
  }

  onSttTranscribeLocal(
    handler: (
      ...args: IpcRequests['sttTranscribeLocal']['params']
    ) => IpcRequests['sttTranscribeLocal']['result'] | Promise<IpcRequests['sttTranscribeLocal']['result']>
  ): void {
    ipcMain.handle(CHANNELS.sttTranscribeLocal, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['sttTranscribeLocal']['params']))
    )
  }

  onSttCheckMicPermission(
    handler: (
      ...args: IpcRequests['sttCheckMicPermission']['params']
    ) => IpcRequests['sttCheckMicPermission']['result'] | Promise<IpcRequests['sttCheckMicPermission']['result']>
  ): void {
    ipcMain.handle(CHANNELS.sttCheckMicPermission, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['sttCheckMicPermission']['params']))
    )
  }

  // Session request handlers
  onSessionCreate(
    handler: (
      ...args: IpcRequests['sessionCreate']['params']
    ) => IpcRequests['sessionCreate']['result'] | Promise<IpcRequests['sessionCreate']['result']>
  ): void {
    ipcMain.handle(CHANNELS.sessionCreate, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['sessionCreate']['params']))
    )
  }

  onSessionUpdate(
    handler: (
      ...args: IpcRequests['sessionUpdate']['params']
    ) => IpcRequests['sessionUpdate']['result'] | Promise<IpcRequests['sessionUpdate']['result']>
  ): void {
    ipcMain.handle(CHANNELS.sessionUpdate, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['sessionUpdate']['params']))
    )
  }

  onSessionList(
    handler: (
      ...args: IpcRequests['sessionList']['params']
    ) => IpcRequests['sessionList']['result'] | Promise<IpcRequests['sessionList']['result']>
  ): void {
    ipcMain.handle(CHANNELS.sessionList, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['sessionList']['params']))
    )
  }

  onSessionGet(
    handler: (
      ...args: IpcRequests['sessionGet']['params']
    ) => IpcRequests['sessionGet']['result'] | Promise<IpcRequests['sessionGet']['result']>
  ): void {
    ipcMain.handle(CHANNELS.sessionGet, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['sessionGet']['params']))
    )
  }

  onSessionDelete(
    handler: (
      ...args: IpcRequests['sessionDelete']['params']
    ) => IpcRequests['sessionDelete']['result'] | Promise<IpcRequests['sessionDelete']['result']>
  ): void {
    ipcMain.handle(CHANNELS.sessionDelete, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['sessionDelete']['params']))
    )
  }

  onSessionOpenInNewWindow(
    handler: (
      ...args: IpcRequests['sessionOpenInNewWindow']['params']
    ) => IpcRequests['sessionOpenInNewWindow']['result'] | Promise<IpcRequests['sessionOpenInNewWindow']['result']>
  ): void {
    ipcMain.handle(CHANNELS.sessionOpenInNewWindow, (_event: IpcMainInvokeEvent, ...args: unknown[]) =>
      handler(...(args as IpcRequests['sessionOpenInNewWindow']['params']))
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

  // ==================== Fire-and-Forget Handlers (send/on pattern) ====================

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

  onAppCloseConfirmed(handler: (event: IpcMainEvent) => void): void {
    ipcMain.on(CHANNELS.appCloseConfirmed, (event: IpcMainEvent) => handler(event))
  }

  onAppCloseCancelled(handler: (event: IpcMainEvent) => void): void {
    ipcMain.on(CHANNELS.appCloseCancelled, (event: IpcMainEvent) => handler(event))
  }

  // ==================== Event Emitters (main → renderer) ====================

  ptyData(...args: IpcEvents['ptyData']['params']): void {
    this.window?.webContents.send(CHANNELS.ptyData, ...args)
  }

  ptyExit(...args: IpcEvents['ptyExit']['params']): void {
    this.window?.webContents.send(CHANNELS.ptyExit, ...args)
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

  terminalNew(): void {
    this.window?.webContents.send(CHANNELS.terminalNew)
  }

  terminalShowSessions(): void {
    this.window?.webContents.send(CHANNELS.terminalShowSessions)
  }

  sessionShowSessions(): void {
    this.window?.webContents.send(CHANNELS.sessionShowSessions)
  }

  sessionSync(...args: IpcEvents['sessionSync']['params']): void {
    this.window?.webContents.send(CHANNELS.sessionSync, ...args)
  }

  daemonDisconnected(): void {
    this.window?.webContents.send(CHANNELS.daemonDisconnected)
  }

  activeProcessesOpen(): void {
    this.window?.webContents.send(CHANNELS.activeProcessesOpen)
  }
}
