# Electron to Rust parity audit: summary

> The Electron app was removed after the fix campaign (`STATUS.md`); its last version is commit `f05be955`. The `src/…` paths and line numbers below are at the audit's commit, 517086c (`git show 517086c:<path>`). Gaps listed here as open may since be fixed: `STATUS.md` and `status-by-row.md` give each row's outcome.

**Scope.** Every Electron source file that the cut-over will delete (`src/main`, `src/preload`, `src/renderer`, `src/applications`, `src/shared`: 199 files in 72 chunks) was mapped row by row to the native port in `app-rs/`. That gives 9,660 behaviour rows: **7,401 ported, 916 partial, 294 missing, 873 not applicable, 176 shared** (handled by the daemon or another shared component). The method had two passes. A first pass mapped each chunk. An adversarial second pass then re-read every chunk. It added 225 rows the first pass missed and changed 979 rows. It searched again for every MISSING row, re-checked every PARTIAL and NA row, and spot-checked the Rust references of PORTED rows. The per-chunk tables are in `chunks/*.md`. Ids below are `c<chunk>-<row>`. Severity is the auditor's: **high** means data loss, a wrong or unsafe git or PTY action, or a core workflow that is missing; **medium** means a visible feature or behaviour difference; **low** means cosmetic, text-only or an edge case.

For this summary I opened the Electron and Rust code behind the top gaps myself. 19 of the 20 groups below are confirmed. One group (sandbox, gap 7) is corrected: it is not a gap between the apps, because the daemon ignores the field.

---

## Top gaps (high severity, deduplicated)

### Session sync and connections

1. **Workspace body writes that were deferred or failed are lost on reconnect.** Confirmed. Electron flushes the old store's dirty bodies on the new transport before it replaces the store. Rust disposes the old store as soon as the connection leaves Connected: `dispose` clears `syncs`, and `flush_deferred_writes` and `set_connection_status` have no callers. Any edit whose write failed as the link died, or that was still waiting on a debounce, is gone when the new store rebuilds from disk.
   Ids: c08-81, c12-73, c55-95, c58-55, c58-19, c59-30, c59-31, c60-68, c61-97, c67-197.
   Electron: `src/renderer/store/app.ts:390-404`; `createSessionStore.ts:130, 650-653, 1363, 2069`; `src/preload/ipc-client.ts:434`; `src/main/ipc/ipc-server.ts:748`.
   Rust: `treeterm-model/src/app_store.rs:212-236` (`retire` on every status change); `session_store.rs:2186` (`flush_deferred_writes`, unused), `:2201` (`set_connection_status`, unused), `:2230` (`dispose`).

2. **During a reconnect the session's tree, views and selection are dropped instead of being dimmed.** Confirmed. Electron keeps the store alive while Reconnecting, Error or Disconnected, and shows the workspace rows at 60% opacity under the banner. Rust retires the store on every status change. `AppView::sync` keeps views for Connected slots only, so the sidebar shows just the banner. The open workspace, the rail entries, the fold state and the Upload/Monitor sub-tabs all disappear. The traffic counters and the activity history also restart after the reconnect.
   Ids: c12-75 (high); also c55-155, c37-12, c37-13, c37-117, c37-124, c37-131, c37-132, c67-219, c35-148, c35-149, c42-78, c42-79, c54-136.
   Electron: `app.ts:429`; `SessionPanel.tsx:529-535, 634`.
   Rust: `app_store.rs:218-236`; `app_view.rs:171-180`.

3. **After a connect or reconnect no workspace is selected, and a remote with no workspaces gets no home workspace.** Confirmed. Electron's `addRemoteSession` and `localConnect`, and the reconnect handler, show the first ref's workspace. For a remote with no refs, Electron resolves `$HOME` (falling back to `/home/<user>`), adds a workspace there and opens it. Rust starts every connection generation at `ActiveView::Nothing` and `Selection::Nothing`, so each launch and each reconnect lands on "No workspace selected". A new remote stays empty. `resolve_homedir` exists but is used only by the worktree registry.
   Ids: c12-72 (high); c12-27, c08-89, c08-90, c55-89, c55-100, c55-123, c55-124, c55-125, c55-126, c55-127, c55-128, c55-129, c61-103, c61-127, c67-191, c67-196.
   Electron: `app.ts:367-371, 415, 527-548`; `createSessionStore.ts:2085, 2203`.
   Rust: `app_view.rs:268`; `tree_pane.rs:111`; `treeterm-core/src/worktree_registry.rs:93`.

4. **Our own writes never commit their attention mutations.** Confirmed by reading. `register_write` and `discard_write` have no callers. The own-echo branch returns before any reconcile, and `reconcile_write` runs only for external bodies, with an empty sha. A pending Record therefore survives until some external body carries the same revision. If another window has meanwhile written a newer revision, `reapply_attention` puts the older one back, so the unread or acknowledged state can go backwards across windows.
   Ids: c58-132, c65-121 (high); c59-14, c58-99.
   Electron: `createSessionStore.ts:468, 583`; `store/workspaceAttention.ts:54`.
   Rust: `session_store.rs:708-715`; `workspace_store.rs:1606-1616`; `treeterm-core/src/attention.rs:158-191`.

### Terminal

5. **A key or pointer press does not re-claim the PTY's input.** Confirmed. Electron claims on focusin, on window focus, and on every keydown and pointerdown (capture phase). Rust claims only when the focus state changes: `set_focused` returns early when the terminal is already focused. If another machine or window claims the PTY, keys typed into an already-focused Rust terminal are dropped silently by the daemon (`pty_manager.rs:373-377`). They keep being dropped until focus leaves the terminal and comes back. The same applies to the Active Processes viewer, and to Refresh, which does not claim the new attachment.
   Ids: c15-58, c68-164 (high); c68-165, c29-125, c15-136.
   Electron: `src/renderer/utils/keyboardFocus.ts:22-28`; `BaseTerminal.tsx:253`; `PtyViewer.tsx:96`.
   Rust: `treeterm-model/src/terminal.rs:398-416`; `treeterm-app/src/terminal_view.rs:105-107, 178, 256`.
   Related (medium): harness prompts and auto-approve are written through the tab's focus-gated Terminal attachment, where Electron uses an ungated Background attachment. They are dropped when another client owns the input: c04-95, c06-63, c29-83, c29-93, c29-99, c62-70, c70-60.

6. **Resize: the view requests its own size on every repaint and overrides other attachments.** Confirmed by reading. When the daemon echoes a size, `requested` is cleared, and the next prepaint calls `request_size` again whenever the grid differs from the view. A resize made by another attachment is therefore undone at once, and never shows the size-mismatch badge. Two clients of different sizes on one PTY can resize each other back and forth. Electron refits only when its own container changes size, and flags a foreign size with the badge.
   Ids: c15-84 (high); c15-79.
   Electron: `BaseTerminal.tsx:341, 373`.
   Rust: `treeterm-app/src/terminal_element.rs:299-312`; `treeterm-model/src/terminal.rs:433-443, 643-648`.

7. **CORRECTED: PTYs are created without a sandbox (c58-20).** The audit rated this high because Rust always sends `CreatePtyRequest.sandbox: None` (`treeterm-core/src/daemon.rs:158`). But daemon-rs never reads the field: `grpc_server.rs:63-75` passes only cwd, env, size, startup command and handle, and nothing in daemon-rs mentions sandbox. Neither app's PTYs are sandboxed, so there is no difference between them. It is reclassified as low and shared. It deserves a note for the project owner: the "Enable Sandbox" setting has no effect in either app, except that Rust uses the sandbox background colour.

### App shell

8. **No menu bar off macOS.** Confirmed. On Linux and Windows GPUI's `set_menus` only stores the menus, and the Kit's `AppMenuBar` is never rendered. So File, Edit, Window, Settings, Quit and Active Processes have no UI there (Ctrl+, still works). This is untested because the port has not yet run on Linux (README "Not yet").
   Ids: c09-161 (high); c09-13, c09-16, c09-40, c09-42.
   Electron: `src/main/menu.ts:161`.
   Rust: `treeterm-app/src/menus.rs:52-75`; `vendor/gpui-component/src/menu/app_menu_bar.rs` (unused).

### Workspace and tabs

9. **The tab strip has no drag to reorder, no split panes and no overflow handling.** Confirmed. In Electron, flex-layout lets tabs be dragged to reorder, docked to split the pane, resized with splitters and maximized. Tabs that do not fit go into a "Hidden tabs" menu, and "+" stays visible. Rust draws one `h_flex` of tabs with click handlers only. Saved splits are flattened into one strip. Tabs past the width, and the "+" button, are pushed off-screen, where only the prefix next/prev keys can reach them. Rust never writes `layoutModel`.
   Ids: c25-86, c25-89, c69-107 (high); c25-85, c25-87, c25-88, c25-108, c25-111, c69-18, c69-19, c69-20, c25-55, c25-66, c25-73.
   Electron: `FlexLayoutPane.tsx:200, 224`; `utils/layoutModel.ts:11-20`.
   Rust: `treeterm-app/src/app_view.rs:1793-1906`; `treeterm-core/src/layout.rs:10`.

10. **The Open Workspace dialog has no "Default Application" choice.** Confirmed. `OnPicked` carries only the path. `finish_open_workspace` always opens the global default as the first tab and saves empty `WorktreeSettings`, so children have nothing to inherit. The value can be set later in the workspace's Settings tab, but by then the first tab already exists. Medium would arguably be fairer, because that workaround exists.
    Ids: c28-18 (high); c28-9, c28-12, c28-47, c28-48, c28-49, c28-50, c28-51, c28-52, c36-29, c37-58, c60-105, c60-111.
    Electron: `OpenWorkspaceDialog.tsx:48, 147-161`; `createSessionStore.ts:1486, 1512`.
    Rust: `treeterm-app/src/open_workspace_dialog.rs:16`; `treeterm-model/src/session_store.rs:1407, 1448, 1477`.

### AI harness

11. **"Prompt Description" and "Skip" are missing.** Confirmed. Electron shows them on an AI-harness tab when the workspace has a description that has not been sent yet. "Prompt Description" sends it through `promptHarness`, and both buttons set `descriptionPrompted`. Rust has no button, and only the analyzer ever writes `descriptionPrompted`.
    Ids: c29-86, c29-87, c29-89, c48-56, c48-95 (high); c29-88, c29-90, c48-55, c48-57.
    Electron: `PromptDescriptionButton.tsx:5-27`; `WorkspacePane.tsx:289-299, 446`.
    Rust: `treeterm-core/src/analyzer/mod.rs:705, 951`; `treeterm-model/src/workspace_store.rs:400` (`prompt_harness` exists).

12. **"Auto-approve safe" is persisted and restored. In Electron it always starts off.** Confirmed. Electron writes `autoApprove: false` when it creates the tab and never saves the switch. Rust saves the switch in the synced tab state and restores it at analyzer start. After a restart, after a reconnect, or in a second Rust window, safe permission prompts are therefore auto-approved without the user turning it on again. This is safety-relevant and is not in the README.
    Ids: c13-116 (high); c56-94.
    Electron: `src/applications/aiHarness/renderer.ts:30`; `createAnalyzerStore.ts:471, 576`.
    Rust: `treeterm-model/src/workspace_store.rs:446, 983`.

13. **Git status is not refreshed when the harness goes Idle or Completed, or after other git changes.** Confirmed. Electron runs `refreshGitInfo` and `refreshGit` on each Idle or Completed transition. It also refreshes every workspace on load, after staging, commit and merge, and on every row click. In Rust, `AnalyzerEvent::RefreshGit` only re-reads the branch. `GitController::refresh` runs only for the viewed workspace on load and on a workspace switch, plus the status button, Pull and the GitHub tab. The controller does not poll. So the "Uncommitted", behind, conflict and PR indicators go stale while an agent works.
    Ids: c56-126, c63-71, c57-130 (high); also c31-68, c31-134, c31-141, c31-148, c31-155, c31-164, c32-39, c61-50, c61-57, c61-58, c61-59, c63-54, c61-129, c59-139, c59-161, c60-15, c61-36, c36-75.
    Electron: `createAnalyzerStore.ts:588-597`; `createWorkspaceStore.ts:360, 390`; `createGitControllerStore.ts:134`.
    Rust: `treeterm-model/src/workspace_store.rs:996, 615-627`; `session_store.rs:860, 931`; `git_controller.rs:201`.

### Files

14. **Favourites are not inherited from ancestor workspaces.** Confirmed. Electron's `getWorkspaceFavouritePaths` walks the parents, so a child worktree shows its parent's favourites in the Files tab and in Review, stars them as covered, and re-resolves them when an ancestor changes. Rust uses the workspace's own list only. `file_browser.rs:14-17` documents this, but the README's deliberate list does not. In Rust, children start with an empty list, and starring an inherited path copies it into the child's own list.
    Ids: c23-6, c63-117, c71-105 (high); c31-16, c23-35, c23-83, c23-84, c59-110, c63-26, c63-118, c63-119, c63-197, c63-198, c63-199, c71-106, c71-109.
    Electron: `src/shared/workspaceFavourites.ts:21-43`; `FileTree.tsx:45`; `ReviewBrowser.tsx:99`.
    Rust: `treeterm-model/src/workspace_store.rs:1547-1556`; `file_browser.rs:14-17`; `review.rs:1039`.

### Review

15. **Merges run without a fresh conflict dry-run, so a conflicting merge leaves the parent worktree mid-merge.** Confirmed. Before Merge, Squash Merge or Merge and Keep, Electron re-runs `checkMergeConflicts`. If it finds conflicts, Electron shows "Cannot merge: N conflict(s) detected" and stops. Rust's `merge_locked` checks only that the parent is clean, then runs `git merge` directly, and nothing in app-rs runs `merge --abort`. While a re-check is running, Rust also treats the conflict state as unknown: the banner and "(has conflicts)" disappear and Merge is enabled (c32-61, c32-137).
    Ids: c32-5, c32-6, c32-9, c32-25, c32-26, c32-29 (high); c32-7, c32-8, c32-27, c32-28, c32-61, c32-137, c32-150, c32-155, c32-160.
    Electron: `ReviewBrowser.tsx:589-601, 636-645`.
    Rust: `treeterm-model/src/session_store.rs:2366-2413`; `treeterm-core/src/git/mod.rs:963-984`.

16. **Each diff rebuild resets collapse state and scroll.** Confirmed. The `DiffState` cache key is the patch text plus the file list, so it changes on every stage or unstage from a file header, on any refresh that changed something, on the `-w` toggle, and when a filter changes the listed files. The rebuild collapses binary files only and scrolls to the first file. Files folded by hand open again, "Collapse all" is not re-applied (the button still says "Expand all"), and the reader loses their place.
    Ids: c21-61 (high); c43-125, c43-126, c21-23.
    Electron: `FileDiffSection.tsx:112, 142`; `StackedDiffList.tsx:465, 489`.
    Rust: `treeterm-app/src/review_view/stacked.rs:201-261`.

17. **Selecting a commit reconciles the branch's viewed marks against that commit's files.** Confirmed. `select_commit` calls `reconcile_viewed`, which checks the shown files, here the commit's. A file that the commit changed differently from the whole branch loses its viewed mark in the shared `reviewViewedFiles` metadata, in both apps. Electron's `loadCommitDiff` does not reconcile.
    Ids: c31-117.
    Electron: `ReviewBrowser.tsx:418-430`.
    Rust: `treeterm-model/src/review.rs:647, 1252-1265`.

### Tree and git

18. **"Remove" on a root workspace keeps every descendant worktree and branch.** Confirmed. Electron's `remove()` calls `removeWorkspace` with `keepBranch: false, keepWorktree: false`. That runs `git worktree remove` on each descendant worktree, deletes its branch (unless detached) and drops its worktree-registry entry. Rust uses `RemoveMode::KeepWorktreeAndBranch`, both from the tree's Remove and from the header close (x). The descendants leave the session but stay on disk and in the registry, untracked. The Rust behaviour is the safer of the two, but it is not documented as deliberate.
    Ids: c36-68 (high); c48-41, c61-65.
    Electron: `SessionPanel.tsx:336-338`; `createSessionStore.ts:1155-1199, 1758`.
    Rust: `treeterm-app/src/tree_pane.rs:605-609`; `app_view.rs:1665`; `treeterm-model/src/session_store.rs:177-190`.

19. **"Dismiss" on a Loading or Error row does nothing.** Confirmed. The menu of a row that is not loaded asks `Remove workspace "<label>"?` and then calls `remove_workspace`, which returns at once when the entry has no store. `dismiss_workspace` exists but has no caller. A failed creation, a refused lock or an adopt error therefore leaves a row that cannot be removed. Async adopt and auto-open failures land in such rows (see the Dialogs section).
    Ids: c37-104, c60-89 (high); c36-98, c58-33, c48-101, c60-90, c60-129.
    Electron: `SessionPanel.tsx:495, 869`; `createSessionStore.ts:1429`.
    Rust: `treeterm-app/src/tree_pane.rs:543-550`; `treeterm-model/src/session_store.rs:1525, 2046-2049`.

A bug found while confirming (medium, filed under Files): the 3D model viewer removes `#status` before it builds the stage, so `showError` dereferences null whenever WebGL is unavailable. The user then sees a blank canvas with no message (c26-128, c26-132; `treeterm-app/assets/model-viewer/viewer.src.js:231, 248`).

---

## By area (medium and low)

[M] = medium, [L] = low. Duplicates reported from a component, its store and its type are merged.

### Session sync
- [M] Live terminal ignores a width-limit flip made in another window until it re-attaches (c15-46, c47-119).
- [M] Editor tab: tab-state writes are debounced 800 ms, and the pending write is dropped on close or quit (c22-47). An open editor never follows external tab-state changes and writes its stale content and dirty flag over another window's edits (c22-66).
- [M] Files tab reads selectedPath, expandedDirs and scrollToLine only when the tab is built, then writes stale values back over another window's (c23-77, c25-6, c24-138).
- [M] Force Unlock is not offered next to an operation error, only in the session header while the lock is held (c48-99). [L] The button is hidden once `expires_at` passes, even though the daemon may still report the lock (c37-37).
- [M] A malformed `workspaceAttention` is accepted and treated as read (Electron rejects the body) (c58-131).
- [M] Auto Open takes the session lock once per child instead of once for the batch (c60-137, c61-2).
- [L] `connectionId` is never written into terminal, harness, runner or Web App tab state (no visible effect) (c00-18, c00-122, c01-11, c01-112).
- [L] Error text has a doubled prefix: "Invalid workspace file: invalid workspace file: …" (c58-140).
- [L] Attention is applied optimistically instead of after the echo (c58-133).
- [L] `dispose` keeps the git controllers (a cooldown refresh can still run) and Web App start-up tasks (c61-115, c61-116).
- [L] The coordinator never pauses while restoring (c58-26, c59-54).
- [L] Unknown workspace-file keys are kept (a superset) (c71-128).

### Connections / SSH
- [M] Saved connections are deduplicated by id, and the form creates a new id on every connect, so reconnecting from the form appends duplicates and older entries keep stale forwards. `Settings::save_ssh_connection` (host, user and port match) is used only by its test (c07-53, c07-54, c08-49, c11-118, c12-36, c17-106, c67-216, c70-100).
- [M] When the session fails to load after connect, there is no Error state and no "Remote daemon is outdated. Retry with 'Refresh remote daemon' checked." The port force-reconnects with backoff indefinitely and shows "Reconnecting…" (c07-40 MISSING, c07-41, c07-42, c08-88, c12-28, c67-208).
- [M] Starting a connect does not switch the main pane to the new session's pane (c17-90, c55-133).
- [M] The picker resets its fields on each open, so "Refresh remote daemon" is not kept (c17-85). It refuses ports that Electron coerces to 22 (c17-100). The forward host field is not reset to "localhost" after Add (c17-114, MISSING).
- [M] The Bootstrap, Tunnel and Daemon logs in the session pane freeze on the old stream after Retry, Refresh daemon or Ignore mismatch (c35-36, c35-41, c35-46).
- [M] Sessions that are not connected have no header context menu, so a remote stuck in Connecting cannot be disconnected (c37-26).
- [M] Session pane Monitor and Upload are blank while not connected and lose their state (c35-148, c35-149). The upload notice is never reached (c42-60).
- [L] `--ssh` with a target that cannot be parsed exits 1 instead of opening locally. The grammar differs: '@' in the host, port 0 or above 65535, a leading '+' (c06-3, c07-102, c07-112).
- [L] Wording and casing of connect errors (c04-48, c04-53). Disconnected carries no error (c02-17). A tunnel exit does not overwrite the Reconnecting error (c02-83).
- [L] Session pane: Retry on a failed local connection retries (Electron does nothing) (c35-61). A disconnect removes the session from every window (c35-95). Session ID is shown only while connected (c35-109). The Error line also shows while Reconnecting (c35-116). The TTYs tab shows "Not connected" (c35-150).

### Daemon client
- [M] Nothing fails in-flight execs when a connection enters Reconnecting. They end when the socket closes, are cancelled silently with their owner, or hit the backstop. There is no "connection <id> lost while command was running" (c03-12, c03-17, c03-18, c07-96). [L] Execs are not cancelled on a still-open local socket (c03-15).
- [M] User-visible gRPC errors show tonic's `Status` dump ('status: Internal, message: "…", details: [], metadata: …') instead of '13 INTERNAL: spawn failed' (c04-83).
- [L] Connect is tried once instead of retrying until the 5 s deadline (c04-52). The exec backstop margin is 10 s instead of 5 s, so messages say 40000ms (c50-3, c52-80, c52-90).

### Traffic
- [M] Byte counters and meters exist per connection generation and per workspace store, so every reconnect resets the totals and the rate window and shows "↓ …". There is no lookup by connection id, so the "↓ ?" error state cannot happen. A replaced remote's counter is not carried over (c02-9, c02-113, c02-126, c03-23, c04-44, c11-117, c55-32, c62-11, c62-14, c62-40).
- [L] Raw socket bytes are counted (HTTP/2 framing included) instead of message payloads (c03-22, c04-51, c62-12). Every workspace is sampled from creation, so a late first burst shows as a spike (c62-30). Removed workspaces' badges are not pruned (c62-47). The loading badge has a "0 B" tooltip (c46-74). The badge shows a fixed "↓ …" for sessions that are not connected (c37-36). Exact `.x5` sizes round half to even (c68-169, c70-49).

### Terminal
- [M] The AI-harness `disableScrollbar` setting is stored and editable but nothing reads it: the scrollbar is always drawn and the wheel scrolls the scrollback (c00-28, c13-97, c15-24, c15-150, c71-59).
- [M] Mouse features missing: copy-on-select in Ghostty tabs (c66-76), hovered-link underline and pointer (c66-127), right-click selects the word on macOS (c66-143), Alt+click moves the cursor (c66-144), Shift+click extends the selection (c66-146), drag past the edge auto-scrolls (c66-148). [L] Alt+drag block selection (c66-145), Alt+wheel fast scroll (c66-147).
- [M] Double-click word boundaries use alacritty's separators, which include ':' and '|', so `file.rs:42` and URLs are split (c66-142).
- [M] Links open only on Cmd/Ctrl+click and OSC 8 accepts any scheme (c66-126). [L] Ghostty tabs open links on a plain click in Electron (c66-37). Rust detects fewer URL schemes and finds different extents (c66-44).
- [M] The tab shown after closing the active tab, or after a program exits, does not get focus (c15-62).
- [M] "Bottom" has no one-line tolerance: the badge reads MIDDLE, output is not followed, and a resize keeps the ratio (c15-65, c15-76, c66-62, c66-114).
- [M] AI-harness background colour accepts only #rrggbb (c00-27, c13-95). The placeholder reads "Starting terminal…" (c13-88, c25-104, c45-95).
- [M] Active Processes viewer: exit text, attach-error heading, loading text (c29-117, c29-130, c29-131, c67-67). [L] Font, palette, cursor and scrollback come from settings (c29-106, c29-107, c29-108, c29-109). Banner instead of an error view (c29-122, c29-127). Adds a "disconnected" banner (c29-144).
- [L] The pin snaps back only on output and resize, and the wheel unpins only when it scrolls the scrollback (c15-57, c15-59). Badge: no 250 ms delay, double-click behaviour, "120 x 40" label (c15-159, c15-169, c15-165, c63-14).
- [L] Overlays: stream errors and write errors carry generic or different text, a failed keystroke write is only logged, and attach failures have no "Failed to reattach terminal:" prefix (c04-15, c04-18, c04-21, c04-38, c11-58, c15-82, c15-91, c15-119). The overlay is kept while hidden (c15-17). The 1 s exit rule applies to hidden tabs (c15-116). Banners are in the UI font and can be clicked away (c15-152, c15-153).
- [L] Context-menu paste is bracketed (c15-146). The width-strip look and the switch animation differ (c15-151, c46-66).
- [L] OSC 52 edge cases: target byte, an empty payload clears the clipboard, base64 strictness (c66-80, c66-82, c66-84). Ghostty honours OSC 52 (a superset) (c66-47). The activity snapshot trims trailing spaces (c66-24).

### Workspace / tabs
- [M] Closing a terminal, harness, runner or Web App tab while its PTY is being created drops the task. `on_pty_created`'s kill never runs, so a PTY the daemon already started is orphaned (c00-23, c00-127, c01-18, c01-137).
- [M] An adopted worktree (Existing or Recent, in the dialog) ignores "Use custom default application": `finish_child` gets `own_default: None`, so the first tab, the saved setting and inheritance are all lost (c18-50, c18-59, c19-150, c36-49, c58-37, c58-65, c59-149, c59-156, c60-2, c60-10).
- [M] No hover tooltip names the activity state on tab glyphs or tree glyphs. Three states share ● (c13-61, c45-62).
- [M] Branch badge: no "Renaming…" spinner, "Auto-rename branch" is never hidden, and repeated clicks queue more LLM renames (c16-6, c16-8, c16-12, c16-13, c16-23). [L] Label hyphenation (c16-24).
- [M] Run menu: a menu opened during detection stays on "Loading…" and hides custom runners; there is no maximum height or scrolling; action descriptions are not shown as tooltips; a second run cancels the first (c34-19, c34-34, c34-42, c34-44, c34-113). [L] Tooltip text, and group headers are clickable (c34-31, c34-112).
- [M] tasks.json and launch.json are parsed strictly, so one bad entry hides the whole file (c53-86). [L] (c53-81, c53-120, c53-121).
- [M] Removing the shown workspace leaves it selected, with "Workspace was removed" (c48-62, c60-99).
- [M] An OperationError cannot be cleared from the UI: there is no Cancel, only a "⚠" line (c48-100, c58-32, c60-87). [L] No panel or heading, the tabs stay visible, the old error stays during removal, and merge pre-check errors persist (c48-98, c48-102, c60-37, c60-43, c60-44).
- [M] Workspace loading output is not auto-scrolled (c14-127, c48-60).
- [M] `ghostty-terminal` is registered as a hidden "Terminal" with ">": Settings lists two "Terminal" entries (c00-154, c39-53). [L] Icon and new-tab menu (c00-155, c00-160, c55-46, c67-22).
- [M] A stale activeTabId shows "No tabs" although tabs exist (c69-14). [L] (c25-109). Empty workspace placeholder (c25-110).
- [L] The "+" menu is always left-aligned (c25-95). Labels follow live titles (an improvement) (c25-78, c69-27). The header edit survives a workspace switch (c48-20). Focus order (c48-43). An Error entry shows its reason (c48-63). Name is not selected on focus (c48-66). Action order (c48-73). The description box does not auto-grow (c48-88). The "More options" tooltip is missing (c48-127). Settings focuses the existing tab (c36-69). Path naming with a trailing slash (c60-101). Adopt has no name parameter (c59-146). A dismissed placeholder is not restored (c59-166).
- [L] Shape guards: malformed tab state renders anyway (benign) (c00-25, c00-66, c00-105, c00-178, c01-141, c13-87, c22-49, c25-4, c67-253, c67-254, c67-255, c67-258, c67-259).

### Tree / sidebar
- [M] Quick fork neither selects nor expands the new child (c36-37, c36-38, c36-41, c36-42, c59-119). Created and adopted children do not expand a collapsed parent (c36-46, c36-50, c36-54, c36-58, c36-62). Auto Open selects nothing (c59-159, c60-13).
- [M] Adopted and auto-opened workspaces are named after the directory, not the branch or display name. The name is persisted and shown when there is no displayName (c14-88, c18-53, c18-62, c58-15, c59-151, c60-133, c60-146).
- [M] There is no attention flash on tree rows, favourite rows or rail icons (c37-86, c37-128, c46-84, c46-85, c46-87, c46-88, c46-89, c46-100; c46-86 low). There is no "Saving workspace attention" spinner (c37-93, c46-90, c58-129, c58-135, c59-90, c63-25).
- [M] Row tooltip missing: no path and description (c37-80). [L] Unread-marker tooltip (c37-92, c46-103).
- [M] Loading and Error rows say "Loading…" or the error instead of the name (c37-115), and are absent from the rail (c37-124).
- [M] The toggle slot on a row with no children swallows the click and records a collapse (c37-88).
- [M] The session-rename input does not select its text (c37-29).
- [M] Favourites panel: no PR indicators (c29-79, c46-102). No context menu with Settings and Unmark (c46-97, c46-105, c46-106). No path tooltip (c46-99). Stays highlighted while the session pane is shown (c46-95). [L] Margin (c46-141).
- [M] The conflict triangle is not shown on workspaces without a PR (c29-78).
- [M] Sessions cannot be reordered in the collapsed rail (c46-123) [L] (c46-124). [M] No "No sessions. Click + to browse." empty state (c46-137).
- [M] TTY list: keeps the retired connection after a reconnect, so Refresh fails (c38-33). Its grouping is a snapshot (c38-34). [L] Sticky headers, text centring, stale list (c38-18, c38-66, c38-70).
- [M] Sound-toggle tooltip text (c42-5, c42-6). There is no tooltip on the rail copy (c42-14, c46-120).
- [L] Drag: rows that are not loaded or are descendants show hints, the source row is not dimmed, indicator styling differs, and drags from other sessions are accepted (c36-79, c36-82, c36-100, c36-101, c36-103, c37-69, c37-72, c37-81, c46-135).
- [L] The row and rail icon stay highlighted while the session pane is shown (c36-25, c37-74, c37-118). Re-expanding on change (c36-26). Centring and fold behaviour of session_panel (c37-10, c37-27, c37-34, c37-42). Unread rule versus focus outline (c37-76). Subtree hover tint (c37-106). Sidebar resize and accent (c12-104, c12-114). After a disconnect, the next session shows no workspace or info pane (c55-116, c55-117).
- [L] Session names: the "Local" default (c64-90). Stable keys (an improvement) (c64-93). A rename does not move the session (c64-95, c64-101). No 7-day expiry, which is not wanted with stable keys (c64-94, c64-111, c64-112, c64-120). [M] Corrupt ui-state.json is never repaired, so names, order and recents stop persisting (c64-116, c64-84).
- [L] JSON viewer whitespace and fold memory (c26-63, c26-69). Session order is kept across reconnects (an improvement) (c55-156).

### Dialogs
- [M] The child dialog has no processing phase: it closes as soon as an adopt starts, and progress shows as a tree row. A lock failure after that becomes an Error row that cannot be dismissed (gap 19) (c18-19 root, c19-21; [L] c18-20, c18-48, c18-54, c18-57, c18-63, c18-105, c18-110, c18-119, c18-128, c18-133, c18-137, c19-12, c19-17, c19-23, c19-26, c19-29, c19-32, c19-35, c19-38, c19-41, c19-92, c19-115, c19-138).
- [M] The child dialog loads each list once per dialog: no refetch or retry, and a late error shows on whichever tab is open. A failed PR fetch looks like "No open pull requests" (c18-98, c19-126; [L] c19-44, c19-48, c19-65, c19-67, c19-84, c19-89, c19-104, c19-112, c19-131).
- [M] Auto Open closes at once and reports neither progress nor errors in the dialog (c14-91, c14-92, c37-55, c58-38, c61-1; [L] c14-100, c14-102, c14-115, c14-118, c14-120, c14-122). [L] Exclusion filter, re-detection, stale error, Ok count (c14-68, c14-69, c14-71, c14-106). It stays open after its root goes away (c36-74).
- [M] Fork PR row has no "open it manually" tooltip (c19-145). [L] Tooltips on branches already in a worktree (c19-99, c19-121).
- [M] Active Processes lists PTYs only on open and after a stop (c13-27). [L] Names are a snapshot (c13-20, c12-85).
- [M] Remote directory browser: a typed path is not committed on blur, so Select or ".." use the old path (c30-38). [L] Re-submit, tooltips, keyboard access, Enter bubbling (c30-12, c30-34, c30-52, c30-59, c30-60).
- [M] Recent directories are not migrated from Electron's localStorage (`treeterm-recent-directories`), so the list starts empty after cut-over (c64-81).
- [L] Open paths hide the main checkout from Existing and Recent (c18-10, c19-54, c19-74, c37-51, c48-24, c14-68). The PR tab saves a hidden description (c18-74, c61-26). Enter rules (c18-83, c18-84, c28-20). Live parent name (c18-13). Re-clicking a tab clears the search (c18-139). Registry error dropped (c19-52). Empty description row (c19-81). Trimmed search (c19-90, c19-113). Keyboard access to controls (c19-151). Truncation (c19-154). Recent-row tooltip (c28-32). Close-confirm "()" for an empty branch (c16-112). The dialog stays open after its parent goes away (c36-73).
- [L] Native panels: the confirm text reads "Open Workspace" or "Upload", and they are not sheets (c06-86, c06-93, c42-41).

### Review / diff
- [M] The highlighted "active file" and the start point of next/previous unviewed are the last clicked file, not the file at the top of the scroll. The Kit diff does not report the top file. The README states this limit only for find (c20-29, c20-81, c20-85, c31-29, c32-77, c33-29, c33-44, c33-56, c33-57, c33-63, c33-80, c33-96, c33-97, c33-119, c43-34; c43-35 low).
- [M] Tooltips missing on icon-only toolbar buttons (prev/next unviewed, split/unified, unchanged regions, whitespace), file paths in the tree and headers, "new" stats, the base button and the right-click hint (c20-33, c20-58, c20-83, c20-87, c20-90, c20-94, c20-98, c20-118, c21-65, c21-86, c33-136, c33-167, c68-95). [L] Other tooltips (c16-120, c16-121, c16-123, c20-73, c20-76, c20-79, c20-101, c21-74, c32-105, c32-119, c32-123, c32-129, c32-136, c32-142, c32-147, c32-151, c32-156, c32-161, c33-141, c33-168, c33-181, c43-79).
- [M] `selectedFilePath` is never saved, so a restored Review opens at the first file (c31-28, c33-65, c67-58).
- [M] There is no large-diff guard ("This diff is large… Load anyway") (c21-42, c21-43, c21-44; [L] c21-1, c21-2, c21-4, c21-5, c21-6, c21-7, c21-8, c21-14). There is no lazy loading per file, and there are no per-file errors: one failing file fails the whole view (c21-30, c21-89, c21-90, c43-40, c43-94; [L] c43-1, c43-37, c43-38). The per-file refresh reloads everything (c21-72, c21-73, c21-75, c43-95).
- [M] A keyboard selection (Shift+Up/Down, Cmd/Ctrl+A) or a line-number drag opens a comment box (c21-51, c29-5, c43-98).
- [M] Find: the match row lands under the sticky header (c43-66). The selection stays after find closes or the query changes, and feeds the context menu (c43-62; [L] c43-108, c29-20). Cmd+F opens an invisible find bar while a favourite is shown (c43-43). [L] Bindings and nothing focused (c43-22, c43-46).
- [M] Review refreshes only when its tab label is clicked: not on keyboard switches, the header Review button, or returning to the workspace (c00-179, c31-71, c48-172). A switch between Committed, Uncommitted and Commits reloads everything and shows "Loading…". Commits reloads page 1 and drops later pages and the selection (c31-94, c32-183, c33-1, c33-4; [L] c32-174, c32-178). The spinner runs on every reload (c31-53, c31-63). A change to the workspace's own branch does not reload the review (c31-88).
- [M] Viewed marks are reconciled only for the view that is shown (c31-97, c31-102). The metadata parse is strict: bad JSON blocks every comment or viewed edit (c57-65, c57-66, c57-105).
- [M] Close and Clean has no "Closing…" and does not disable the header (c31-42, c32-45, c32-130). [M] A failed conflict check hides "Prompt Update from Parent" (c31-122).
- [M] hasParent is decided by the parent's branch, not by parentWorkspaceId: a child whose parent has no branch gets "Close" (c31-19; [L] c32-120, c00-176, c00-180, c67-54, c67-256).
- [M] Merge error prefix (c32-17, c32-34). [L] No alerts, shown in the panel or the header instead (c32-18, c32-22, c32-35, c32-41, c32-47, c32-48). Missing pre-check guards (c61-62, c61-63, c61-64).
- [M] SVG changes show a text diff instead of images (c21-55). AVIF is not an image (c26-56). [L] Alt text and fallback, checkerboard (c26-57, c26-60).
- [M] `-w` ignores whitespace inside lines, while Electron trims only the ends (c29-23).
- [M] Clicking a disabled Stage button collapses the file (c21-69). Menu items ignore conflicts that appear while the menu is open (c32-150, c32-155, c32-160). [L] Busy Stage selects the row (c20-123). Refresh stays clickable (c32-118). Check-all and Collapse-all are never disabled (c43-81, c43-84).
- [M] Sort order by ICU versus code point (c20-11; [L] c68-114). Folded directories are shared across trees and views (c20-52; [L] c20-25).
- [M] Files-tab comments: refused without a HEAD (unborn repo), and HEAD is read once per model (c25-22, c25-23). [L] "Still reading HEAD", inline error instead of an alert (c25-31, c25-35).
- [M] The banner text for a failed state load (c31-92). [M] Cmd+Enter versus Ctrl+Enter submit per platform (c16-129).
- [M] Removal output after merge-and-remove is not streamed to the merge panel (c50-125, c51-13; [L] c60-28, c61-47, c61-49).
- [L] Viewed hover opacity (c20-30). A click on a stale favourite opens the rebuilt diff at the top (c20-32, c20-57, c43-127). Active toggle hover (c20-124). Header shows the displayName (c32-110). Empty-state alignment, locale dates, prefetch, base filter memory, blank pane while loading (c33-6, c33-12, c33-144, c33-160, c33-194). Menu over comments and headers (c43-12, c43-13). A partially staged file shows only staged hunks (c43-128). Dark+ token colours (c54-114). Comment text cannot be selected, en-US dates (c16-122, c16-124). No logging (c31-172, c32-186).

### Files / editor
- [M] Scroll offset is not persisted for the Files viewer or the Editor tab. It is kept only in the in-run view cache (c22-7, c22-27, c24-55, c24-59, c25-15, c25-53, c67-51, c67-69; [L] c22-28, c22-37, c22-40, c24-24, c24-38, c24-60, c24-136, c25-14).
- [M] `scrollToLine` is never cleared from tab state, so every rebuild jumps to that line again, even on a different file (c24-58, c25-16, c67-50, c67-70; [L] c22-39).
- [M] Editor tab language features are missing: no go to definition, completion, hover, diagnostics, JSON/CSS/HTML/TS services or link detection (c22-42, c22-83, c54-96, c54-97, c54-98, c54-99, c54-100, c54-109, c68-170; [L] c22-43, c22-82, c54-138).
- [M] The editor always inserts 2-space soft tabs and draws tabs 2 columns wide. There is no indentation detection, so editing Makefiles and Go files changes their content (c22-84).
- [M] `.ico` and `.avif` show as binary instead of images (c05-49, c24-31, c70-35, c70-43).
- [M] Open in Tab re-reads the file instead of using the viewer's content, and refuses non-UTF-8 files (safer) (c24-71, c05-48; [L] c22-13, c22-17).
- [M] Re-clicking the file already selected re-reads it, drops "Open anyway", and clears the reveal and the active match (c23-114, c24-135).
- [M] Highlighting: .xml, .less, .r, .vue and dockerfiles are shown plain (c24-99).
- [M] Error prefixes: "Error loading file:" and "Error saving file:" (c22-21, c22-34, c24-36). [L] (c22-20, c23-18, c23-19, c23-26, c23-27, c24-35).
- [M] Model and PDF webview pages are cached by path, so a file changed on disk shows its old bytes (c26-151, c28-155). [L] Camera and WebGL state are kept while hidden (c53-59, c53-60).
- [M] Model viewer bug: no error is shown when WebGL fails (see the end of Top gaps) (c26-128, c26-132). [L] Viewer styling and states (c24-94, c26-144, c26-146, c26-157, c26-158, c26-159).
- [M] PDFs have no toolbar: no page box, zoom, print or download (c28-148).
- [M] Download state is shared by the whole tab, so a failure shows on the next file (c21-116). [L] Save dialog first, no size budget (c21-107, c21-108).
- [M] A failed directory keeps its error until the new answer arrives (c23-22).
- [L] Markdown: raw HTML is rendered, `$$` blocks, image alt text (c26-114, c26-115, c26-161). Editor details: scroll past the end, line highlight, scrollbar, cursor, replace key, retry loop (c22-23, c22-71, c22-75, c22-76, c22-80, c22-81). Message-row hover (c23-115). Star and clear-search have no keyboard or a11y access (c23-43, c23-116). Broken image fallback (c24-137). Tree resize ends only on mouse-up (c25-42). A same-file definition scrolls the viewer (c54-137). Definition errors are shown (c68-38, c68-41, c68-42).

### Search
- [M] Result file headings truncate at the end and lose the file name. `text_ellipsis_start` would match (c34-83).
- [L] No scrollbar, tab width (c34-79, c34-86). Seeding behaviour (c24-42, c24-46, c24-47).

### Git
- [M] `isGitRepo` and `gitRootPath` are never re-read after creation, and NotRepo is ignored. A folder that becomes a repo never gets git UI, and a stale branch stays (c56-125, c57-129, c58-49, c61-38, c63-187; [L] c61-40).
- [M] A GitController exists only when the GitHub HTTP client builds. Without it there is no git status, Pull, PR or behind count (c57-41).
- [M] Pull failure shows only in the tooltip of a button that may be hidden (c48-165). [L] The status button is not disabled while refreshing (c48-140).
- [M] A failed `git worktree remove` keeps the workspace with an error, while Electron drops it anyway (c60-27).
- [M] The worktree registry is parsed strictly: one bad entry breaks list, upsert and remove (c54-33).
- [L] Concurrency and coalescing differences (c49-99, c50-103). A branchless merge is refused up front (c60-51).

### GitHub / PR
- [M] When GraphQL details fail, Rust shows `PrDetails::Unavailable` with a grey badge and the reason, where Electron shows an OPEN PR without checks. This is deliberate per `treeterm-core/src/github/mod.rs:7-10`, but not in the README (c52-39, c52-41, c52-55; [L] c26-16, c29-63).
- [M] The GitHub header buttons have no loading state, so each click opens again (c48-144).
- [L] The refresh button has no tooltip and the wrong cursor (c26-9, c26-13).

### AI analyzer / harness
- [M] Failures of title, description and branch refresh are never shown. `last_refresh` is recorded but unread; only the no-harness case shows, as a toast with different text (c16-10, c16-11, c29-136, c36-2, c56-114, c56-118, c56-119, c63-123, c63-124; [L] c36-3, c63-2).
- [M] The refresh-title button has no in-flight state and stays clickable, so clicks start parallel LLM calls. The tooltip and spinner are missing (c29-134, c29-138, c29-141, c29-142).
- [M] Labelling and renaming use the first harness tab, not the active one (c16-9, c29-135, c63-35).
- [M] Transitions to the same state are dropped from Analyzer History (c14-128, c54-126). [L] State and log are dropped when a tab closes (c54-127).
- [M] A paste emits no Input, so a pasted command does not start title labelling (c13-92, c56-113).
- [M] `prompt_harness` pastes (bracketed, newlines turned into CR) instead of writing raw text + CR (c63-132; [L] c34-7). A failed write is not detected, so comments are marked addressed anyway (c63-133).
- [M] Prompt-debugger mode switch cancels the run and leaves the tab's activity at Working (c45-43).
- [L] 24-hour time (c14-8). Live call list (c14-31). Connection error text (c52-95). Non-string title fields (c52-140). Buffer after exit (c56-35, c56-122). Config-change reclassification (c56-104). Resize-only batches (c56-108). Logging (c56-128). Debugger buffer and result handling (c46-10, c46-13, c45-20). Cursor (c46-48). Button opacity (c34-10). Harness found by registry kind (c68-118).

### Web / Browser
- [M] Web App tabs have no page memory: `path` is never written or restored (c01-130, c01-155, c28-78, c47-82, c67-43). Browser `page_url` concatenates instead of resolving, so a start page with a path reopens at `/app/app/` (c01-156, c28-71).
- [M] If the server PTY fails to start, the tab waits for its port forever with no Error and no Retry, and Retry never re-creates the PTY (c01-110, c01-131, c45-98; [L] c01-116).
- [M] Once a Web App has been Ready, its webview is shown whatever the phase (c47-78).
- [M] In-page navigation (pushState, hash) is not tracked. Back and Forward use the app's own stacks (c28-80 MISSING, c28-79). Stop and progress appear only on commit (c28-81). A failed load shows no error page on macOS (c28-159). [L] A failure after commit leaves Stop up (c28-82).
- [M] URL validation splits strings instead of parsing a URL: upper-case schemes and `127.1` are refused, invalid ports accepted. A local Browser goes Ready without validation (c00-51, c10-127, c10-128, c16-46; [L] c00-72, c10-129). Settings refuses to save an invalid URL, where Electron only warns (c16-54).
- [M] A navigation guard cancels any non-local navigation (a superset of Electron) (c10-141).
- [M] Forward failure text (c01-70).
- [L] Path read once at init, localPort not re-read, probe timeout and "curl: " prefix (c01-171, c01-122, c01-82, c01-83, c01-85). An unparsable probe drops the markdown view (c28-84). Plain reload (c28-97). Escape handling (c28-106).

### Port forwarding
- [M] Saved forwards are not restarted after Retry or Refresh daemon in the session pane, because `forwards_started` stays true while the manager entry is replaced (c07-39, c35-68).
- [L] Exit and spawn error text (c09-61, c09-62). Output kept to 1,000 lines (c28-127). Number fields and the "Starting…" state (c29-53, c29-60). An unknown forward shows an error (c35-59). The dialog closes on a drop (c35-97). Missing tooltips (c35-135).

### Settings
- [M] A failed save keeps the unsaved change in memory, so Cancel keeps it. A save with no changes neither writes nor clears an old error (c39-24, c65-90, c65-170). The sound toggle flips even when the save fails and shows no error (c42-77, c65-68, c42-9 MISSING; [L] c42-3, c42-8, c42-18, c65-63).
- [M] Number fields are plain text, checked only on Save, with no fallbacks or bounds (c39-58, c39-59, c39-61, c39-62, c41-15, c41-19, c41-23, c41-40; [L] c41-14, c41-18, c41-22, c41-39).
- [M] Escape closes Settings and discards the edits (c39-98). A second open stacks a second dialog (c39-99).
- [M] Tab buttons, checkboxes, selects and buttons cannot be reached from the keyboard (c39-48). [L] Labels are not linked, and there are no accessible names (c40-46, c40-47, c40-52, c40-63, c40-65, c40-67, c40-73).
- [L] A pasted icon longer than the limit is rejected instead of truncated (c16-50, c40-27, c41-53). The last tab is not remembered (c39-11). Hover borders, disabled cursor, monospace hint (c40-21, c40-37, c40-77, c41-69, c41-89). `settings.json` `recentDirectories` is no longer updated; nothing reads it (c06-89, c08-34, c11-66, c12-21, c67-241, c70-84).

### Keybindings
- [M] One prefix or action key that cannot be parsed resets the whole keybinding config to the defaults, where Electron disables only that key (c64-22). Space as an action key is refused when saved (c41-90).
- [L] Multi-chord or empty prefix key (c64-121, c64-122). The recorder also triggers prefix mode and listens app-wide (c39-26, c39-42). Overlay labels, sorting and placement (c26-87, c26-90, c26-92, c26-160).

### App shell / menu
- [M] Menu items missing: About (c09-4), Hide Cmd+H (c09-8), Close Window Cmd+W (c09-15), View → Zoom in, out and reset (c09-33, c09-34, c09-35), Toggle Full Screen (c09-36). [L] Services, Hide Others, Show All, Zoom, Bring All to Front, Close off macOS, Help and Learn More, Reload, Force Reload, DevTools, Reset Input, the whole View menu, Edit Undo/Redo/Cut/Delete/Paste and Match Style (c09-7, c09-9, c09-10, c09-21, c09-22, c09-23, c09-26, c09-27, c09-29, c09-30, c09-31, c09-32, c09-37, c09-38, c09-41, c09-43, c09-45, c09-46, c09-47). Copy, Paste and Select All apply to terminals only (c09-24, c09-25, c09-28).
- [M] While a Kit dialog has focus, Preferences, Minimize and Active Processes are greyed out, and Cmd+Q quits without the unmerged-workspace confirmation (c09-6, c09-11, c09-42; [L] c09-14, c09-40).
- [M] Exit and Kill Daemon does nothing to the daemon when local is not connected (there is no temporary client), and skips the close confirmation (c07-120 MISSING, c09-18; [L] c07-122).
- [M] There is no error boundary: a panic on the UI thread ends the process, with no app, workspace or tab fallback and no Reload (c12-105, c12-106, c12-107, c14-58, c20-107, c20-112, c45-73, c47-114; [L] c14-62, c20-108, c20-110, c20-111, c20-113, c45-74, c45-75, c45-83, c45-84, c45-85, c45-86, c45-87, c47-115, c47-116, c47-117, c47-118).
- [M] The close confirmation ignores unmerged worktrees of sessions that are not connected (c55-82).
- [M] `treeterm [dir]` canonicalises the directory, so symlinks resolve (`/tmp/x` becomes `/private/tmp/x`) (c06-2).
- [M] Different empty-state text (c12-118). [L] No splash window, different start-up error handling, title "TreeTerm" (c06-20, c07-90, c07-110, c06-113). With no session left, the "Starting TreeTerm daemon…" spinner shows (c55-118). Ctrl+Q and Ctrl+M are not bound off macOS (c09-16).

### Tool tabs
- [M] Comments tab: nothing can be selected or copied (c17-154). The path has no tooltip (c17-58). Code has no horizontal scroll (c17-65). en-US dates only (c17-67). [L] Tooltips, empty file, parse error, hover (c17-61, c17-69, c17-71, c17-54, c17-152, c17-153).
- [M] Chat scrolls only when the text length changes (c16-96). [L] Selection and whitespace (c16-94, c16-139).
- [M] SSH Upload "Browse…" opens the whole Open Workspace modal, which starts at `/` (c30-7, c42-69, c42-72). State and the result are lost on a reconnect, and a result is lost if the connection drops mid-scp (c42-78, c42-79). [L] (c42-37, c42-39, c42-62, c42-70, c42-73).
- [M] Workspace settings: the JSON cannot be selected and its long lines overflow (c49-116, c49-117). [L] Enter keeps focus, Shift+Enter saves, unchanged blur writes nothing (c49-18, c49-22, c49-23, c49-114).
- [M] Prompt debugger result is not selectable (c45-59).

### System monitor
- [M] Histories are lost when the sub-tab is hidden (c44-42). A run that exits non-zero shows partial metrics instead of an error (c44-99). Stop and Kill have no SIGTERM/SIGKILL tooltips (c44-77, c44-79). Disk used and total bytes are never shown (c44-83).
- [L] 24-hour time, refresh tooltip, 15 s timeout, 4 s after each run (c44-80, c44-82, c44-92, c44-103). macOS load average is fixed in Rust (c44-16).

### Logging / diagnostics
- [L] The "Show Debug Badge" and "Show Raw Characters" settings do nothing, and their hint still mentions DevTools (c15-154, c15-70, c15-3, c15-4, c15-5, c15-6, c15-7, c15-8, c15-9, c39-70, c41-77, c71-81, c71-98). The log level comes only from RUST_LOG, the default is info, and an unknown level silences the output (c69-39, c69-40, c69-44). Debug-only lines are hidden at info (c09-71, c10-34, c10-37). Missing log lines: reconnect scheduling, tunnel teardown, child create, session restore (c02-66, c10-27, c18-42, c55-157). The session JSON differs (c35-77, c35-78, c35-79, c35-82, c35-152). There is no DevTools (c09-32).

### Notifications
- [L] A failed or muted system bell is not detected (c13-9). The rest is deliberate (below).

---

## Deliberate differences

### Explained in app-rs/README.md
- **The attention ding is the system bell.** There is no 880 Hz tone, gain or envelope, and no "notification sound failed" banner (c12-108, c12-109, c13-4, c13-10, c13-11, c13-12, c13-14, c54-63, c54-69, c55-24, c65-152; the missing failure detection, c13-9, follows from this).
- **The analyzer reads the tab's own terminal** instead of a headless Background attachment (c63-64). The workspace traffic is lower for harness tabs as a result (c62-46). For analyzer reads, c06-63 is deliberate. Harness prompt writes are not: see Top gap 5.
- **A corrupt settings.json is logged and not overwritten** (c09-111).
- **Replayed OSC 52 copies are ignored** (c66-92).
- **Files-tab review comments are saved with relative paths, and absolute ones are still shown** (c25-28, c25-37, c32-96).
- **Merge-and-remove stages untracked files before its WIP commit** (c60-48, c61-44).
- **Appearance (GPUI Kit limits):** textareas have no resize grip (c18-134, c40-55, c40-66, c41-2, c45-50, c46-40, c46-44). There is no minimap (c22-69). Comments are in a panel instead of Monaco view zones (c24-67, c24-69, c24-113, c25-50). Selects open a Kit menu (c40-46).
- **Review's find:** there is no word under the pointer and no top-of-scroll file for the right-click menu and "In this file" (c43-9, c43-15, c43-17, c43-18, c43-71, c68-87). The README limits this to find. The same Kit limit also drives the active-file gaps in Review, which the README does not mention.
- **Session names and order are keyed by connection target** (README Status) (c64-93).

### Deliberate only in code comments or tests (README is silent; consider documenting)
- `settings.json` is not created until the first save (c09-112). A saved `workspaceFocus` and unknown keybinding keys are kept (c09-119). Every key is type-checked per key (c09-153). Unknown keys and classifier criteria are kept (c09-154, c70-19). Rust writes only the fields that Settings edits (c39-23). Unsaved instances appear in the default-app list (c39-8). Unknown ids show as "(not available)" (c39-54, c49-27).
- GitHub `PrDetails::Unavailable` instead of a fake OPEN PR (c52-39, c52-41, c52-55, c26-16, c29-63).
- The exec backstop margin is 10 s (c50-3, c52-80, c52-90). The Analyzer History call list is live (c14-31). The macOS load average is fixed (c44-16). Unknown workspace-file keys are kept (c71-128).
- Favourites are not inherited (`file_browser.rs:16`): this one is a real gap, so it is listed in Top gaps (gap 14), not here.
- The editor refuses to open non-UTF-8 files instead of corrupting them on save (c22-17, c05-48).

---

Files in this folder: `gaps.md` (every MISSING/PARTIAL row, by severity), `per-file.md` (row counts per Electron file), `chunks/*.md` (the full row-by-row map: each Electron closure, branch and UI element with its Rust `file:line`, or a note that it has none). Audited at commit 517086c on 2026-11-15/16.
