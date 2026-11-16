# Rows per Electron file

Counts after the second pass. NA means no Rust counterpart is needed by design (IPC or preload plumbing, React mechanics, library glue). SHARED means the daemon does it and both apps call the same RPC. The "high" column counts high-severity gaps whose Electron reference is in that file.

| Electron file | rows | ported | partial | missing | na | shared | high |
|---|---|---|---|---|---|---|---|
| src/applications/aiHarness/renderer.ts | 32 | 25 | 4 | 1 | 1 | 1 |  |
| src/applications/analyzerHistory/renderer.ts | 9 | 8 | 0 | 0 | 1 | 0 |  |
| src/applications/browser/renderer.ts | 30 | 27 | 2 | 0 | 1 | 0 |  |
| src/applications/browser/url.ts | 20 | 19 | 1 | 0 | 0 | 0 |  |
| src/applications/chat/renderer.ts | 9 | 8 | 0 | 0 | 1 | 0 |  |
| src/applications/comments/renderer.ts | 10 | 8 | 1 | 0 | 1 | 0 |  |
| src/applications/customRunner/renderer.ts | 23 | 19 | 2 | 0 | 1 | 1 |  |
| src/applications/editor/renderer.ts | 9 | 8 | 0 | 0 | 1 | 0 |  |
| src/applications/filesystem/renderer.ts | 9 | 8 | 0 | 0 | 1 | 0 |  |
| src/applications/ghosttyTerminal/renderer.ts | 11 | 7 | 3 | 0 | 1 | 0 |  |
| src/applications/github/renderer.ts | 9 | 8 | 0 | 0 | 1 | 0 |  |
| src/applications/index.ts | 1 | 0 | 0 | 0 | 1 | 0 |  |
| src/applications/review/renderer.ts | 12 | 7 | 4 | 0 | 1 | 0 |  |
| src/applications/sshUpload/renderer.tsx | 12 | 11 | 0 | 0 | 1 | 0 |  |
| src/applications/terminal/renderer.ts | 38 | 31 | 2 | 0 | 4 | 1 |  |
| src/applications/terminalAnalyzerDebugger/renderer.ts | 8 | 7 | 0 | 0 | 1 | 0 |  |
| src/applications/ttyList/renderer.ts | 9 | 8 | 0 | 0 | 1 | 0 |  |
| src/applications/webApp/forward.ts | 16 | 14 | 1 | 0 | 1 | 0 |  |
| src/applications/webApp/ports.ts | 23 | 18 | 3 | 0 | 2 | 0 |  |
| src/applications/webApp/renderer.ts | 56 | 45 | 8 | 0 | 3 | 0 |  |
| src/applications/webApp/runtime.ts | 12 | 8 | 3 | 0 | 1 | 0 |  |
| src/applications/workspaceSettings/renderer.ts | 9 | 8 | 0 | 0 | 1 | 0 |  |
| src/main/connectionManager.ts | 185 | 176 | 6 | 0 | 3 | 0 |  |
| src/main/execStreamRegistry.ts | 22 | 9 | 4 | 0 | 9 | 0 |  |
| src/main/grpcByteCounter.ts | 17 | 11 | 2 | 0 | 4 | 0 |  |
| src/main/grpcClient.ts | 193 | 152 | 13 | 0 | 23 | 5 |  |
| src/main/index.ts | 242 | 150 | 19 | 2 | 59 | 12 |  |
| src/main/ipc/ipc-server.ts | 90 | 46 | 4 | 2 | 9 | 29 | 1 |
| src/main/menu.ts | 49 | 10 | 14 | 24 | 1 | 0 | 1 |
| src/main/portForward.ts | 33 | 30 | 3 | 0 | 0 | 0 |  |
| src/main/settings.ts | 74 | 68 | 5 | 0 | 1 | 0 |  |
| src/main/socketPath.ts | 5 | 5 | 0 | 0 | 0 | 0 |  |
| src/main/ssh.ts | 127 | 124 | 3 | 0 | 0 | 0 |  |
| src/main/webviewGuard.ts | 14 | 5 | 4 | 0 | 5 | 0 |  |
| src/preload/eventDispatcher.ts | 20 | 8 | 0 | 0 | 12 | 0 |  |
| src/preload/index.ts | 122 | 67 | 3 | 1 | 29 | 22 |  |
| src/preload/ipc-client.ts | 83 | 42 | 7 | 0 | 6 | 28 | 3 |
| src/renderer/App.tsx | 42 | 30 | 6 | 3 | 3 | 0 |  |
| src/renderer/audio/ding.ts | 18 | 1 | 6 | 0 | 11 | 0 |  |
| src/renderer/components/ActiveProcessesDialog.tsx | 33 | 27 | 2 | 0 | 2 | 2 |  |
| src/renderer/components/ActivityIndicator.tsx | 13 | 12 | 0 | 1 | 0 | 0 |  |
| src/renderer/components/ActivityStateBadge.tsx | 19 | 18 | 0 | 0 | 1 | 0 |  |
| src/renderer/components/AiHarness.tsx | 36 | 21 | 4 | 2 | 9 | 0 | 1 |
| src/renderer/components/AnalyzerHistory.tsx | 57 | 52 | 3 | 0 | 2 | 0 |  |
| src/renderer/components/AppErrorFallback.tsx | 6 | 0 | 0 | 2 | 4 | 0 |  |
| src/renderer/components/AutoOpenWorktreesDialog.tsx | 61 | 47 | 13 | 0 | 1 | 0 |  |
| src/renderer/components/AutoScrollPre.tsx | 4 | 1 | 1 | 0 | 2 | 0 |  |
| src/renderer/components/BaseTerminal.tsx | 169 | 113 | 22 | 11 | 23 | 0 | 2 |
| src/renderer/components/BranchBadge.tsx | 25 | 13 | 4 | 5 | 3 | 0 |  |
| src/renderer/components/BrowserPane.tsx | 18 | 18 | 0 | 0 | 0 | 0 |  |
| src/renderer/components/BrowserSettingsSection.tsx | 19 | 15 | 3 | 0 | 1 | 0 |  |
| src/renderer/components/Chat.tsx | 45 | 40 | 3 | 0 | 2 | 0 |  |
| src/renderer/components/CloseConfirmDialog.tsx | 12 | 11 | 1 | 0 | 0 | 0 |  |
| src/renderer/components/CommentDisplay.tsx | 9 | 4 | 5 | 0 | 0 | 0 |  |
| src/renderer/components/CommentInput.tsx | 11 | 10 | 1 | 0 | 0 | 0 |  |
| src/renderer/components/CommentsList.tsx | 80 | 68 | 5 | 5 | 1 | 1 |  |
| src/renderer/components/ConnectionPicker.tsx | 70 | 55 | 9 | 1 | 5 | 0 |  |
| src/renderer/components/ContextMenu.tsx | 5 | 4 | 0 | 0 | 1 | 0 |  |
| src/renderer/components/CreateChildDialog.tsx | 294 | 230 | 56 | 4 | 4 | 0 |  |
| src/renderer/components/DiffFileTree.tsx | 71 | 57 | 8 | 2 | 4 | 0 |  |
| src/renderer/components/DiffToolbar.tsx | 34 | 22 | 3 | 9 | 0 | 0 |  |
| src/renderer/components/ErrorBoundary.tsx | 13 | 0 | 1 | 5 | 7 | 0 |  |
| src/renderer/components/FileChangeStats.tsx | 6 | 5 | 0 | 1 | 0 | 0 |  |
| src/renderer/components/FileDiffSection.tsx | 94 | 53 | 11 | 14 | 16 | 0 | 1 |
| src/renderer/components/FileDownloadButton.tsx | 22 | 19 | 3 | 0 | 0 | 0 |  |
| src/renderer/components/FileEditor.tsx | 84 | 50 | 21 | 5 | 6 | 2 |  |
| src/renderer/components/FileTree.tsx | 117 | 95 | 12 | 2 | 6 | 2 | 1 |
| src/renderer/components/FileViewer.tsx | 138 | 105 | 16 | 6 | 10 | 1 |  |
| src/renderer/components/FilesystemBrowser.tsx | 54 | 39 | 11 | 3 | 1 | 0 |  |
| src/renderer/components/FlexLayoutPane.tsx | 47 | 23 | 7 | 7 | 10 | 0 | 2 |
| src/renderer/components/GhosttyTerminal.tsx | 10 | 6 | 1 | 0 | 3 | 0 |  |
| src/renderer/components/GitHubBrowser.tsx | 45 | 38 | 2 | 1 | 4 | 0 |  |
| src/renderer/components/IdleDetectorToggle.tsx | 6 | 6 | 0 | 0 | 0 | 0 |  |
| src/renderer/components/ImageDiffViewer.tsx | 9 | 6 | 2 | 1 | 0 | 0 |  |
| src/renderer/components/JsonViewer.tsx | 22 | 19 | 2 | 0 | 1 | 0 |  |
| src/renderer/components/KeybindingOverlay.tsx | 29 | 23 | 4 | 0 | 2 | 0 |  |
| src/renderer/components/MarkdownPreview.tsx | 8 | 5 | 3 | 0 | 0 | 0 |  |
| src/renderer/components/ModelViewer.tsx | 42 | 27 | 8 | 0 | 7 | 0 |  |
| src/renderer/components/MonacoDiffViewer.tsx | 132 | 39 | 0 | 0 | 93 | 0 |  |
| src/renderer/components/OpenWorkspaceDialog.tsx | 55 | 43 | 2 | 9 | 0 | 1 | 1 |
| src/renderer/components/PageViews.tsx | 79 | 60 | 10 | 1 | 8 | 0 |  |
| src/renderer/components/ParentReviewCommentsButton.tsx | 14 | 13 | 0 | 0 | 1 | 0 |  |
| src/renderer/components/PdfViewer.tsx | 11 | 5 | 2 | 0 | 4 | 0 |  |
| src/renderer/components/PierreDiffViewer.tsx | 27 | 22 | 3 | 0 | 2 | 0 |  |
| src/renderer/components/PortForwardDialog.tsx | 34 | 30 | 2 | 0 | 2 | 0 |  |
| src/renderer/components/PrIndicators.tsx | 19 | 16 | 3 | 0 | 0 | 0 |  |
| src/renderer/components/PromptCommitButton.tsx | 6 | 5 | 1 | 0 | 0 | 0 |  |
| src/renderer/components/PromptDescriptionButton.tsx | 5 | 0 | 0 | 5 | 0 | 0 | 3 |
| src/renderer/components/PromptGitHubCommentsButton.tsx | 5 | 4 | 1 | 0 | 0 | 0 |  |
| src/renderer/components/PromptRebaseButton.tsx | 6 | 5 | 1 | 0 | 0 | 0 |  |
| src/renderer/components/PtyViewer.tsx | 32 | 16 | 11 | 0 | 4 | 1 |  |
| src/renderer/components/RefreshTitleButton.tsx | 10 | 2 | 4 | 2 | 2 | 0 |  |
| src/renderer/components/RemoteDirectoryBrowser.tsx | 62 | 50 | 4 | 3 | 3 | 2 |  |
| src/renderer/components/ReviewBrowser.tsx | 553 | 453 | 51 | 38 | 11 | 0 | 7 |
| src/renderer/components/ReviewCommentsButton.tsx | 13 | 10 | 2 | 0 | 1 | 0 |  |
| src/renderer/components/RunActionDropdown.tsx | 36 | 25 | 6 | 1 | 4 | 0 |  |
| src/renderer/components/SearchPanel.tsx | 64 | 57 | 3 | 0 | 4 | 0 |  |
| src/renderer/components/SessionInfoPane.tsx | 152 | 124 | 18 | 1 | 9 | 0 |  |
| src/renderer/components/SessionPanel.tsx | 237 | 163 | 49 | 10 | 15 | 0 | 2 |
| src/renderer/components/SessionTtyList.tsx | 70 | 57 | 5 | 0 | 6 | 2 |  |
| src/renderer/components/SettingsDialog.tsx | 268 | 220 | 37 | 6 | 5 | 0 |  |
| src/renderer/components/SoundToggle.tsx | 21 | 9 | 7 | 1 | 4 | 0 |  |
| src/renderer/components/SshUploadPane.tsx | 58 | 42 | 11 | 0 | 4 | 1 |  |
| src/renderer/components/StackedDiffList.tsx | 128 | 74 | 26 | 3 | 25 | 0 |  |
| src/renderer/components/SystemMonitor.tsx | 114 | 98 | 5 | 5 | 6 | 0 |  |
| src/renderer/components/SystemPromptDebugger.tsx | 59 | 55 | 4 | 0 | 0 | 0 |  |
| src/renderer/components/TabActivityIndicator.tsx | 3 | 2 | 0 | 1 | 0 | 0 |  |
| src/renderer/components/TabContentPortals.tsx | 19 | 6 | 0 | 3 | 10 | 0 |  |
| src/renderer/components/TabErrorFallback.tsx | 6 | 0 | 0 | 5 | 1 | 0 |  |
| src/renderer/components/Terminal.tsx | 12 | 7 | 2 | 0 | 3 | 0 |  |
| src/renderer/components/TerminalAnalyzerDebugger.tsx | 54 | 44 | 5 | 0 | 5 | 0 |  |
| src/renderer/components/TerminalStatusBar.tsx | 8 | 7 | 0 | 0 | 1 | 0 |  |
| src/renderer/components/ToggleSwitch.tsx | 7 | 5 | 1 | 0 | 1 | 0 |  |
| src/renderer/components/TrafficBadge.tsx | 10 | 6 | 1 | 0 | 3 | 0 |  |
| src/renderer/components/TreePane.tsx | 63 | 38 | 5 | 16 | 4 | 0 |  |
| src/renderer/components/TtyListBrowser.tsx | 55 | 49 | 0 | 0 | 4 | 2 |  |
| src/renderer/components/UpstreamWarningDialog.tsx | 11 | 11 | 0 | 0 | 0 | 0 |  |
| src/renderer/components/WebAppPane.tsx | 21 | 18 | 2 | 0 | 1 | 0 |  |
| src/renderer/components/WebAppSettingsSection.tsx | 20 | 19 | 0 | 0 | 1 | 0 |  |
| src/renderer/components/WidthLimitToggle.tsx | 7 | 6 | 1 | 0 | 0 | 0 |  |
| src/renderer/components/WorkspaceErrorFallback.tsx | 5 | 0 | 0 | 5 | 0 | 0 |  |
| src/renderer/components/WorkspacePane.tsx | 174 | 123 | 18 | 6 | 27 | 0 | 2 |
| src/renderer/components/WorkspaceSettings.tsx | 42 | 35 | 6 | 1 | 0 | 0 |  |
| src/renderer/components/terminalTheme.ts | 6 | 6 | 0 | 0 | 0 | 0 |  |
| src/renderer/contexts/SessionStoreContext.ts | 3 | 0 | 0 | 0 | 3 | 0 |  |
| src/renderer/hooks/useTtyConnection.ts | 14 | 7 | 0 | 0 | 6 | 1 |  |
| src/renderer/hooks/useWorkspaceApis.ts | 6 | 0 | 0 | 0 | 6 | 0 |  |
| src/renderer/lib/classificationProvider.ts | 28 | 28 | 0 | 0 | 0 | 0 |  |
| src/renderer/lib/concurrency.ts | 12 | 11 | 1 | 0 | 0 | 0 |  |
| src/renderer/lib/fileLimits.ts | 6 | 6 | 0 | 0 | 0 | 0 |  |
| src/renderer/lib/gitClient.ts | 259 | 252 | 4 | 0 | 3 | 0 |  |
| src/renderer/lib/githubClient.ts | 79 | 76 | 3 | 0 | 0 | 0 |  |
| src/renderer/lib/homedir.ts | 11 | 8 | 2 | 0 | 1 | 0 |  |
| src/renderer/lib/llmClient.ts | 58 | 47 | 3 | 0 | 8 | 0 |  |
| src/renderer/lib/modelLoader.ts | 34 | 34 | 0 | 0 | 0 | 0 |  |
| src/renderer/lib/modelStage.ts | 28 | 25 | 2 | 0 | 1 | 0 |  |
| src/renderer/lib/runActionsClient.ts | 68 | 63 | 4 | 0 | 0 | 1 |  |
| src/renderer/lib/sha256.ts | 2 | 2 | 0 | 0 | 0 | 0 |  |
| src/renderer/lib/withTimeout.ts | 5 | 5 | 0 | 0 | 0 | 0 |  |
| src/renderer/lib/worktreeHierarchy.ts | 18 | 18 | 0 | 0 | 0 | 0 |  |
| src/renderer/lib/worktreeRegistry.ts | 40 | 39 | 1 | 0 | 0 | 0 |  |
| src/renderer/main.tsx | 36 | 23 | 2 | 0 | 7 | 4 |  |
| src/renderer/monaco-config.ts | 20 | 7 | 2 | 6 | 5 | 0 |  |
| src/renderer/pierre-diffs-config.ts | 3 | 0 | 1 | 0 | 2 | 0 |  |
| src/renderer/store/activityState.ts | 21 | 16 | 3 | 0 | 2 | 0 |  |
| src/renderer/store/app.ts | 157 | 121 | 10 | 11 | 10 | 5 | 1 |
| src/renderer/store/contextMenu.ts | 14 | 10 | 0 | 0 | 4 | 0 |  |
| src/renderer/store/createAnalyzerStore.ts | 114 | 93 | 11 | 1 | 9 | 0 | 1 |
| src/renderer/store/createGitControllerStore.ts | 61 | 57 | 3 | 0 | 1 | 0 | 1 |
| src/renderer/store/createReviewCommentStore.ts | 32 | 30 | 2 | 0 | 0 | 0 |  |
| src/renderer/store/createReviewViewedFilesStore.ts | 27 | 25 | 1 | 0 | 1 | 0 |  |
| src/renderer/store/createRunActionsStore.ts | 10 | 10 | 0 | 0 | 0 | 0 |  |
| src/renderer/store/createSessionStore.ts | 607 | 492 | 71 | 10 | 24 | 10 | 8 |
| src/renderer/store/createTrafficMeterStore.ts | 50 | 38 | 7 | 0 | 5 | 0 |  |
| src/renderer/store/createTtyStore.ts | 21 | 15 | 1 | 0 | 2 | 3 |  |
| src/renderer/store/createWorkspaceStore.ts | 201 | 162 | 16 | 3 | 19 | 1 | 2 |
| src/renderer/store/keybinding.ts | 65 | 55 | 3 | 0 | 7 | 0 |  |
| src/renderer/store/navigation.ts | 5 | 5 | 0 | 0 | 0 | 0 |  |
| src/renderer/store/recentDirectories.ts | 19 | 15 | 2 | 0 | 2 | 0 |  |
| src/renderer/store/sessionNames.ts | 33 | 23 | 5 | 4 | 1 | 0 |  |
| src/renderer/store/settings.ts | 96 | 83 | 5 | 0 | 8 | 0 |  |
| src/renderer/store/startWorkspaceNotifications.ts | 5 | 2 | 0 | 0 | 3 | 0 |  |
| src/renderer/store/workspaceAttention.ts | 27 | 26 | 1 | 0 | 0 | 0 | 1 |
| src/renderer/store/workspaceAttentionCoordinator.ts | 42 | 35 | 1 | 0 | 6 | 0 |  |
| src/renderer/terminal/engine.ts | 26 | 22 | 1 | 0 | 3 | 0 |  |
| src/renderer/terminal/ghosttyEngine.ts | 41 | 31 | 4 | 1 | 5 | 0 |  |
| src/renderer/terminal/proposeDimensions.ts | 7 | 6 | 0 | 0 | 1 | 0 |  |
| src/renderer/terminal/xtermCellSize.ts | 2 | 1 | 0 | 0 | 1 | 0 |  |
| src/renderer/terminal/xtermEngine.ts | 65 | 42 | 7 | 7 | 9 | 0 |  |
| src/renderer/types/classification.ts | 9 | 9 | 0 | 0 | 0 | 0 |  |
| src/renderer/types/index.ts | 259 | 182 | 22 | 0 | 24 | 31 | 1 |
| src/renderer/utils/base64.ts | 5 | 3 | 0 | 0 | 2 | 0 |  |
| src/renderer/utils/definitionSearch.ts | 40 | 35 | 4 | 0 | 1 | 0 |  |
| src/renderer/utils/detectMonacoLanguage.ts | 10 | 10 | 0 | 0 | 0 | 0 |  |
| src/renderer/utils/diffSearch.ts | 33 | 30 | 1 | 0 | 2 | 0 |  |
| src/renderer/utils/downloadFile.ts | 4 | 3 | 0 | 0 | 1 | 0 |  |
| src/renderer/utils/favouriteFiles.ts | 24 | 22 | 2 | 0 | 0 | 0 |  |
| src/renderer/utils/findRunningHarnessPtyId.ts | 7 | 5 | 1 | 0 | 1 | 0 |  |
| src/renderer/utils/fitTerminal.ts | 5 | 5 | 0 | 0 | 0 | 0 |  |
| src/renderer/utils/formatBytes.ts | 6 | 5 | 1 | 0 | 0 | 0 |  |
| src/renderer/utils/idleDetector.ts | 22 | 22 | 0 | 0 | 0 | 0 |  |
| src/renderer/utils/keybindingConverter.ts | 4 | 3 | 0 | 0 | 1 | 0 |  |
| src/renderer/utils/keyboardFocus.ts | 10 | 6 | 0 | 2 | 2 | 0 | 1 |
| src/renderer/utils/keyboardHealthMonitor.ts | 11 | 0 | 0 | 0 | 11 | 0 |  |
| src/renderer/utils/layoutModel.ts | 20 | 12 | 2 | 4 | 2 | 0 | 1 |
| src/renderer/utils/loadWebglRenderer.ts | 6 | 1 | 0 | 0 | 5 | 0 |  |
| src/renderer/utils/logger.ts | 15 | 10 | 3 | 0 | 2 | 0 |  |
| src/renderer/utils/repoSearch.ts | 37 | 37 | 0 | 0 | 0 | 0 |  |
| src/renderer/utils/reviewPrompt.ts | 9 | 9 | 0 | 0 | 0 | 0 |  |
| src/renderer/utils/ttyDisplay.ts | 9 | 8 | 1 | 0 | 0 | 0 |  |
| src/shared/classifierSettings.ts | 27 | 26 | 1 | 0 | 0 | 0 |  |
| src/shared/fileContentKind.ts | 21 | 18 | 2 | 0 | 1 | 0 |  |
| src/shared/formatBytes.ts | 1 | 0 | 1 | 0 | 0 | 0 |  |
| src/shared/ipc-types.ts | 101 | 84 | 3 | 0 | 12 | 2 |  |
| src/shared/lifecycle.ts | 31 | 5 | 0 | 0 | 26 | 0 |  |
| src/shared/types.ts | 78 | 72 | 3 | 0 | 1 | 2 |  |
| src/shared/workspaceFavourites.ts | 12 | 9 | 0 | 3 | 0 | 0 | 1 |
| src/shared/workspaceFile.ts | 29 | 28 | 1 | 0 | 0 | 0 |  |
| src/shared/workspaceSettings.ts | 4 | 4 | 0 | 0 | 0 | 0 |  |
| **total (199 files)** | 9660 | 7401 | 916 | 294 | 873 | 176 | 45 |
