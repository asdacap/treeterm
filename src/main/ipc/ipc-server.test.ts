import { describe, it, expect, vi, beforeEach } from 'vitest'

const { mockHandle, mockOn } = vi.hoisted(() => ({
  mockHandle: vi.fn<(...args: any[]) => any>(),
  mockOn: vi.fn<(...args: any[]) => any>()
}))

vi.mock('electron', () => ({
  ipcMain: {
    handle: mockHandle,
    on: mockOn
  },
  BrowserWindow: vi.fn<(...args: any[]) => any>()
}))

import type { BrowserWindow } from 'electron'
import type { Session, ConnectionInfo, TTYSessionInfo } from '../../shared/types'
import { IpcServer } from './ipc-server'

describe('IpcServer', () => {
  let server: IpcServer

  beforeEach(() => {
    vi.clearAllMocks()
    server = new IpcServer()
  })

  describe('handle registration (invoke pattern)', () => {
    it('onPtyCreate registers handler on pty:create channel', () => {
      const handler = vi.fn<(...args: any[]) => any>()
      server.onPtyCreate(handler)
      expect(mockHandle).toHaveBeenCalledWith('pty:create', expect.any(Function))
    })

    it('onPtyCreate wrapper forwards event and args to handler', async () => {
      const handler = vi.fn<(...args: any[]) => Promise<string>>().mockResolvedValue('pty-123')
      // eslint-disable-next-line @typescript-eslint/no-unsafe-argument
      server.onPtyCreate(handler as any)
      const wrapper = mockHandle.mock.calls[0][1] as (...args: any[]) => Promise<string>
      const fakeEvent = { sender: {} }
      const result = await wrapper(fakeEvent, '/home/user', undefined, undefined)
      expect(handler).toHaveBeenCalledWith(fakeEvent, '/home/user', undefined, undefined)
      expect(result).toBe('pty-123')
    })

    it('onGitGetInfo registers handler on git:getInfo channel', () => {
      const handler = vi.fn<(...args: any[]) => any>()
      server.onGitGetInfo(handler)
      expect(mockHandle).toHaveBeenCalledWith('git:getInfo', expect.any(Function))
    })

    it('onSessionUpdate registers handler on session:update channel', () => {
      const handler = vi.fn<(...args: any[]) => any>()
      server.onSessionUpdate(handler)
      expect(mockHandle).toHaveBeenCalledWith('session:update', expect.any(Function))
    })

    it('onSettingsLoad registers handler on settings:load channel', () => {
      const handler = vi.fn<(...args: any[]) => any>()
      server.onSettingsLoad(handler)
      expect(mockHandle).toHaveBeenCalledWith('settings:load', expect.any(Function))
    })

    it('onFsReadFile registers handler on fs:readFile channel', () => {
      const handler = vi.fn<(...args: any[]) => any>()
      server.onFsReadFile(handler)
      expect(mockHandle).toHaveBeenCalledWith('fs:readFile', expect.any(Function))
    })

    it('onFsReadFile wrapper forwards args and returns result', async () => {
      const handler = vi.fn<(...args: any[]) => Promise<{ success: boolean }>>().mockResolvedValue({ success: true })
      // eslint-disable-next-line @typescript-eslint/no-unsafe-argument
      server.onFsReadFile(handler as any)
      const wrapper = mockHandle.mock.calls[0][1] as (...args: any[]) => Promise<{ success: boolean }>
      const result = await wrapper({}, '/ws', '/file.txt')
      expect(handler).toHaveBeenCalledWith('/ws', '/file.txt')
      expect(result).toEqual({ success: true })
    })

    it('onAppGetWindowUuid registers handler on app:getWindowUuid channel', () => {
      const handler = vi.fn<(...args: any[]) => any>()
      server.onAppGetWindowUuid(handler)
      expect(mockHandle).toHaveBeenCalledWith('app:getWindowUuid', expect.any(Function))
    })

    it.each([
      ['onPtyAttach', 'pty:attach'],
      ['onGitCreateWorktree', 'git:createWorktree'],
      ['onGitRemoveWorktree', 'git:removeWorktree'],
      ['onGitListWorktrees', 'git:listWorktrees'],
      ['onGitListLocalBranches', 'git:listLocalBranches'],
      ['onGitListRemoteBranches', 'git:listRemoteBranches'],
      ['onGitGetBranchesInWorktrees', 'git:getBranchesInWorktrees'],
      ['onGitCreateWorktreeFromBranch', 'git:createWorktreeFromBranch'],
      ['onGitCreateWorktreeFromRemote', 'git:createWorktreeFromRemote'],
      ['onGitGetDiff', 'git:getDiff'],
      ['onGitGetFileDiff', 'git:getFileDiff'],
      ['onGitMerge', 'git:merge'],
      ['onGitCheckMergeConflicts', 'git:checkMergeConflicts'],
      ['onGitHasUncommittedChanges', 'git:hasUncommittedChanges'],
      ['onGitCommitAll', 'git:commitAll'],
      ['onGitDeleteBranch', 'git:deleteBranch'],
      ['onGitRenameBranch', 'git:renameBranch'],
      ['onGitGetUncommittedChanges', 'git:getUncommittedChanges'],
      ['onGitGetUncommittedFileDiff', 'git:getUncommittedFileDiff'],
      ['onGitStageFile', 'git:stageFile'],
      ['onGitUnstageFile', 'git:unstageFile'],
      ['onGitStageAll', 'git:stageAll'],
      ['onGitUnstageAll', 'git:unstageAll'],
      ['onGitCommitStaged', 'git:commitStaged'],
      ['onGitGetFileContentsForDiff', 'git:getFileContentsForDiff'],
      ['onGitGetUncommittedFileContentsForDiff', 'git:getUncommittedFileContentsForDiff'],
      ['onGitGetHeadCommitHash', 'git:getHeadCommitHash'],
      ['onSettingsSave', 'settings:save'],
      ['onFsReadDirectory', 'fs:readDirectory'],
      ['onFsWriteFile', 'fs:writeFile'],
      ['onFsSearchFiles', 'fs:searchFiles'],
      ['onSessionUpdate', 'session:update'],
      ['onDaemonShutdown', 'daemon:shutdown'],
      ['onDialogSelectFolder', 'dialog:selectFolder'],
      ['onDialogGetRecentDirectories', 'dialog:getRecentDirectories'],
      ['onSandboxIsAvailable', 'sandbox:isAvailable'],
      ['onAppGetInitialWorkspace', 'app:getInitialWorkspace'],
      ['onRunActionsDetect', 'runActions:detect'],
      ['onRunActionsRun', 'runActions:run'],
      ['onSshConnect', 'ssh:connect'],
      ['onSshDisconnect', 'ssh:disconnect'],
      ['onSshListConnections', 'ssh:listConnections'],
      ['onSshSaveConnection', 'ssh:saveConnection'],
      ['onSshGetSavedConnections', 'ssh:getSavedConnections'],
      ['onSshRemoveSavedConnection', 'ssh:removeSavedConnection'],
      ['onSshGetOutput', 'ssh:getOutput'],
      ['onSshWatchOutput', 'ssh:watchOutput'],
      ['onSshUnwatchOutput', 'ssh:unwatchOutput'],
      ['onSshWatchConnectionStatus', 'ssh:watchConnectionStatus'],
      ['onSshUnwatchConnectionStatus', 'ssh:unwatchConnectionStatus'],
    ] as const)('%s registers handler on %s channel', (method, channel) => {
      const handler = vi.fn<(...args: any[]) => any>()
      // eslint-disable-next-line @typescript-eslint/no-unsafe-call, @typescript-eslint/no-unsafe-member-access
      ;(server as any)[method](handler)
      expect(mockHandle).toHaveBeenCalledWith(channel, expect.any(Function))
    })

    it.each([
      ['onGitCreateWorktree', 'git:createWorktree'],
      ['onSettingsSave', 'settings:save'],
      ['onFsReadDirectory', 'fs:readDirectory'],
      ['onSessionUpdate', 'session:update'],
      ['onDaemonShutdown', 'daemon:shutdown'],
      ['onDialogSelectFolder', 'dialog:selectFolder'],
      ['onSandboxIsAvailable', 'sandbox:isAvailable'],
      ['onAppGetInitialWorkspace', 'app:getInitialWorkspace'],
    // eslint-disable-next-line @typescript-eslint/no-unused-vars
    ] as const)('%s wrapper forwards args to handler and returns result', async (method, _channel) => {
      const handler = vi.fn<(...args: any[]) => Promise<string>>().mockResolvedValue('result')
      // eslint-disable-next-line @typescript-eslint/no-unsafe-call, @typescript-eslint/no-unsafe-member-access
      ;(server as any)[method](handler)
      const wrapper = mockHandle.mock.calls[0][1] as (...args: any[]) => Promise<string>
      const result = await wrapper({}, 'arg1', 'arg2')
      expect(handler).toHaveBeenCalledWith('arg1', 'arg2')
      expect(result).toBe('result')
    })

    it.each([
      ['onPtyCreate', 'pty:create'],
      ['onPtyAttach', 'pty:attach'],
      ['onLlmChatSend', 'llm:chat:send'],
    // eslint-disable-next-line @typescript-eslint/no-unused-vars
    ] as const)('%s wrapper forwards event and args to handler', async (method, _channel) => {
      const handler = vi.fn<(...args: any[]) => Promise<string>>().mockResolvedValue('result')
      // eslint-disable-next-line @typescript-eslint/no-unsafe-call, @typescript-eslint/no-unsafe-member-access
      ;(server as any)[method](handler)
      const wrapper = mockHandle.mock.calls[0][1] as (...args: any[]) => Promise<string>
      const mockEvent = { sender: {} }
      const result = await wrapper(mockEvent, 'arg1', 'arg2')
      expect(handler).toHaveBeenCalledWith(mockEvent, 'arg1', 'arg2')
      expect(result).toBe('result')
    })
  })

  describe('on registration (send pattern)', () => {
    it('onAppCloseConfirmed registers handler on app:close-confirmed channel', () => {
      const handler = vi.fn<(...args: any[]) => void>()
      server.onAppCloseConfirmed(handler)
      expect(mockOn).toHaveBeenCalledWith('app:close-confirmed', expect.any(Function))
    })

    it('onAppCloseCancelled registers handler on app:close-cancelled channel', () => {
      const handler = vi.fn<(...args: any[]) => void>()
      server.onAppCloseCancelled(handler)
      expect(mockOn).toHaveBeenCalledWith('app:close-cancelled', expect.any(Function))
    })

    it('onLlmChatCancel registers handler on llm:chat:cancel channel', () => {
      const handler = vi.fn<(...args: any[]) => void>()
      server.onLlmChatCancel(handler)
      expect(mockOn).toHaveBeenCalledWith('llm:chat:cancel', expect.any(Function))
    })

    it('onLlmChatCancel wrapper forwards args to handler', () => {
      const handler = vi.fn<(...args: any[]) => void>()
      server.onLlmChatCancel(handler)
      const wrapper = mockOn.mock.calls[0][1] as (...args: any[]) => void
      wrapper({}, 'request-123')
      expect(handler).toHaveBeenCalledWith('request-123')
    })
  })

  describe('event emitters (main → renderer)', () => {
    it('ptyEvent sends to window webContents with correct channel and args', () => {
      const mockSend = vi.fn<(...args: any[]) => void>()
      const mockWindow = { webContents: { send: mockSend } } as unknown as BrowserWindow
      server.setWindow(mockWindow)

      const dataBytes = new TextEncoder().encode('hello-data')
      server.ptyEvent('pty-1', { type: 'data', data: dataBytes })
      expect(mockSend).toHaveBeenCalledWith('pty:event', 'pty-1', { type: 'data', data: dataBytes })
    })

    it('ptyEvent sends exit to window webContents with correct channel and args', () => {
      const mockSend = vi.fn<(...args: any[]) => void>()
      const mockWindow = { webContents: { send: mockSend } } as unknown as BrowserWindow
      server.setWindow(mockWindow)

      server.ptyEvent('pty-1', { type: 'exit', exitCode: 0 })
      expect(mockSend).toHaveBeenCalledWith('pty:event', 'pty-1', { type: 'exit', exitCode: 0 })
    })

    it('settingsOpen sends to window webContents with correct channel', () => {
      const mockSend = vi.fn<(...args: any[]) => void>()
      const mockWindow = { webContents: { send: mockSend } } as unknown as BrowserWindow
      server.setWindow(mockWindow)

      server.settingsOpen()
      expect(mockSend).toHaveBeenCalledWith('settings:open')
    })

    it('appConfirmClose sends to window webContents', () => {
      const mockSend = vi.fn<(...args: any[]) => void>()
      const mockWindow = { webContents: { send: mockSend } } as unknown as BrowserWindow
      server.setWindow(mockWindow)

      server.appConfirmClose()
      expect(mockSend).toHaveBeenCalledWith('app:confirm-close')
    })

    it('sessionSync sends to window with correct args', () => {
      const mockSend = vi.fn<(...args: any[]) => void>()
      const mockWindow = { webContents: { send: mockSend } } as unknown as BrowserWindow
      server.setWindow(mockWindow)

      const sessionData = { id: 'session-1' } as unknown as Session
      server.sessionSync('local', sessionData)
      expect(mockSend).toHaveBeenCalledWith('session:sync', 'local', sessionData)
    })

    it('daemonDisconnected sends to window', () => {
      const mockSend = vi.fn<(...args: any[]) => void>()
      const mockWindow = { webContents: { send: mockSend } } as unknown as BrowserWindow
      server.setWindow(mockWindow)

      server.daemonDisconnected()
      expect(mockSend).toHaveBeenCalledWith('daemon:disconnected')
    })

    it('appReady sends to window webContents', () => {
      const mockSend = vi.fn<(...args: any[]) => void>()
      const mockWindow = { webContents: { send: mockSend } } as unknown as BrowserWindow
      server.setWindow(mockWindow)

      server.appReady('uuid-1' as unknown as Session | null)
      expect(mockSend).toHaveBeenCalledWith('app:ready', 'uuid-1')
    })

    it('capsLockEvent sends to window webContents', () => {
      const mockSend = vi.fn<(...args: any[]) => void>()
      const mockWindow = { webContents: { send: mockSend } } as unknown as BrowserWindow
      server.setWindow(mockWindow)

      server.capsLockEvent(true as unknown as { type: string; key: string; code: string })
      expect(mockSend).toHaveBeenCalledWith('capslock-event', true)
    })

    it('daemonSessions sends to window webContents', () => {
      const mockSend = vi.fn<(...args: any[]) => void>()
      const mockWindow = { webContents: { send: mockSend } } as unknown as BrowserWindow
      server.setWindow(mockWindow)

      const sessions = [{ id: 's1' }] as unknown as TTYSessionInfo[]
      server.daemonSessions(sessions)
      expect(mockSend).toHaveBeenCalledWith('daemon:sessions', sessions)
    })

    it('activeProcessesOpen sends to window webContents', () => {
      const mockSend = vi.fn<(...args: any[]) => void>()
      const mockWindow = { webContents: { send: mockSend } } as unknown as BrowserWindow
      server.setWindow(mockWindow)

      server.activeProcessesOpen()
      expect(mockSend).toHaveBeenCalledWith('active-processes:open')
    })

    it('sshConnectionStatus sends to window webContents', () => {
      const mockSend = vi.fn<(...args: any[]) => void>()
      const mockWindow = { webContents: { send: mockSend } } as unknown as BrowserWindow
      server.setWindow(mockWindow)

      const info = { id: 'conn-1', status: 'connected' } as unknown as ConnectionInfo
      server.sshConnectionStatus(info)
      expect(mockSend).toHaveBeenCalledWith('ssh:connectionStatus', info)
    })

    it('sshOutput sends to window webContents', () => {
      const mockSend = vi.fn<(...args: any[]) => void>()
      const mockWindow = { webContents: { send: mockSend } } as unknown as BrowserWindow
      server.setWindow(mockWindow)

      server.sshOutput('conn-1', 'log line')
      expect(mockSend).toHaveBeenCalledWith('ssh:output', 'conn-1', 'log line')
    })

    it('gitOutput sends to window webContents', () => {
      const mockSend = vi.fn<(...args: any[]) => void>()
      const mockWindow = { webContents: { send: mockSend } } as unknown as BrowserWindow
      server.setWindow(mockWindow)

      server.gitOutput('op-1', 'Preparing worktree')
      expect(mockSend).toHaveBeenCalledWith('git:output', 'op-1', 'Preparing worktree')
    })
  })

  describe('setWindow', () => {
    it('sets the window for event emission', () => {
      const mockSend = vi.fn<(...args: any[]) => void>()
      const mockWindow = { webContents: { send: mockSend } } as unknown as BrowserWindow
      server.setWindow(mockWindow)

      server.ptyEvent('pty-1', { type: 'data', data: new TextEncoder().encode('data') })
      expect(mockSend).toHaveBeenCalled()
    })

    it('null window does not throw on emit', () => {
      const dataBytes = new TextEncoder().encode('data')
      server.setWindow(null)
      expect(() => { server.ptyEvent('pty-1', { type: 'data', data: dataBytes }); }).not.toThrow()
      expect(() => { server.settingsOpen(); }).not.toThrow()
      expect(() => { server.daemonDisconnected(); }).not.toThrow()
    })

    it('no window set (default) does not throw on emit', () => {
      expect(() => { server.ptyEvent('pty-1', { type: 'data', data: new TextEncoder().encode('data') }); }).not.toThrow()
    })
  })
})
