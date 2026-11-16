# c14-renderer_components_AnalyzerHistory

Verified: +1 rows, 9 rows changed

Rust shorthand used below:
- `M` = app-rs/crates/treeterm-model/src/tool_tabs/analyzer_history.rs
- `V` = app-rs/crates/treeterm-app/src/tool_views/analyzer_history_view.rs
- `D` = app-rs/crates/treeterm-app/src/auto_open_dialog.rs
- `S` = app-rs/crates/treeterm-model/src/session_store.rs

## src/renderer/components/AnalyzerHistory.tsx

| # | Electron | Behaviour | Status | Rust parallel | Notes |
|---|---|---|---|---|---|
| c14-1 | src/renderer/components/AnalyzerHistory.tsx:14 | `KIND_COLORS`: analyzer pill `#1a5276`, title pill `#6a0dad` | PORTED | app-rs/crates/treeterm-model/src/tool_tabs/analyzer_history.rs:142 (kind_pill) | Same colours and texts. |
| c14-2 | src/renderer/components/AnalyzerHistory.tsx:273 | `KIND_COLORS[entry.kind] ?? '#555'` fallback for an unknown kind | NA | app-rs/crates/treeterm-core/src/analyzer/mod.rs:156 (HistoryKind) | `kind` is the union `'analyzer' \| 'title'` (createAnalyzerStore.ts:39), so the fallback is unreachable; Rust's `HistoryKind` enum is exhaustive. |
| c14-3 | src/renderer/components/AnalyzerHistory.tsx:19 | `TRANSITION_KIND_LABELS`: "viewport changed" / "viewport idle" / "classification" / "debugger" | PORTED | M:109 (transition_kind) | Same four labels; tested (M:200). |
| c14-4 | src/renderer/components/AnalyzerHistory.tsx:29 | `transitionExtra` ViewportIdle -> `after {idleTimeoutMs}ms` | PORTED | M:121 (transition_extra) | Test M:203 ("after 1500ms"). |
| c14-5 | src/renderer/components/AnalyzerHistory.tsx:30 | `transitionExtra` Classification -> the classification `reason` | PORTED | M:122 (transition_extra) | Test M:205. |
| c14-6 | src/renderer/components/AnalyzerHistory.tsx:31 | `transitionExtra` ViewportChanged / Debugger -> empty string | PORTED | M:123 (transition_extra) | Test M:206. |
| c14-7 | src/renderer/components/AnalyzerHistory.tsx:37 | `AnalyzerHistoryState { sourceTabId }` type + `isAnalyzerHistoryState` guard (object, non-null, has key `sourceTabId`) | PORTED | M:19 (HistorySource), M:41 (AnalyzerHistory::new) | Rust also requires the value to be a string (Electron accepts any value under the key; a malformed non-string id would show empty lists in Electron and the error text in Rust). The app always writes a string (workspace_store.rs:1300). |
| c14-8 | src/renderer/components/AnalyzerHistory.tsx:45 | `formatTime`: `toLocaleTimeString([], {hour,minute,second: '2-digit'})`, in the system locale | PARTIAL | M:181 (clock) -> app-rs/crates/treeterm-core/src/clock.rs:45 (time_of_day) | Rust always prints 24-hour `HH:MM:SS`. Electron follows the OS locale: on a 12-hour locale such as en-US it shows `02:15:30 PM`, Rust `14:15:30`. Used for every transition and call row. |
| c14-9 | src/renderer/components/AnalyzerHistory.tsx:50 | `bufferPreview`: first 3 lines, plus `\n...` when there are more than 3 | PORTED | M:128 (buffer_preview) | Test M:190. |
| c14-10 | src/renderer/components/AnalyzerHistory.tsx:56 | `pillStyle`: white 11px weight-500 text, padding 1px 6px, radius 3 | PORTED | V:35 (pill) | |
| c14-11 | src/renderer/components/AnalyzerHistory.tsx:69 | `AnalyzerHistory` tab app: reads tab state and the workspace's `getTabRef` | PORTED | app-rs/crates/treeterm-model/src/tool_tabs/mod.rs:26 (ToolTab::AnalyzerHistory), app-rs/crates/treeterm-app/src/tool_views/mod.rs:121, app-rs/crates/treeterm-model/src/apps.rs:44 | Opened by app-rs/crates/treeterm-model/src/workspace_store.rs:1299 (open_activity_history), state `{ sourceTabId }`. |
| c14-12 | src/renderer/components/AnalyzerHistory.tsx:72 | Invalid state -> "Invalid state: missing sourceTabId" in `#f14c4c`, padding 16 | PORTED | V:224 (render) | Same text and colour. Test app-rs/crates/treeterm-model/tests/tool_tabs.rs:385. |
| c14-13 | src/renderer/components/AnalyzerHistory.tsx:76 | Source tab's analyzer = ref has `analyzer` (AI harness tab), else null | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:367 (analyzer), M:90 (calls) | `None` for non-harness tabs, as Electron's null. |
| c14-14 | src/renderer/components/AnalyzerHistory.tsx:80 | Page container: padding 16, column, gap 24, full height, scrolls | PORTED | V:230 (render) | `size_full().p(16).gap(24).overflow_y_scroll()`. |
| c14-15 | src/renderer/components/AnalyzerHistory.tsx:81 | Transition list always shown (any tab kind) | PORTED | V:228 (render), V:121 (transitions) | |
| c14-16 | src/renderer/components/AnalyzerHistory.tsx:82 | Analyzer section shown only when the source tab has an analyzer | PORTED | V:152 (calls returns Option), V:237 | |
| c14-17 | src/renderer/components/AnalyzerHistory.tsx:88 | Transitions read live from the activity store, `?? NO_TRANSITIONS` empty fallback | PORTED | M:82 (transitions), app-rs/crates/treeterm-core/src/activity.rs:128 (transitions, unwrap_or_default), M:45 (observe workspace) | (v) Note corrected: live, but app-rs/crates/treeterm-model/src/workspace_store.rs:495 (set_activity) notifies only when the state changes, because same-state updates are not logged at all. What the list contains therefore differs; see c14-128. |
| c14-18 | src/renderer/components/AnalyzerHistory.tsx:89 | Per-transition expanded set; `toggle` adds/removes the index | PORTED | V:17 (Fold::Snapshot), V:105 (on_click toggles) | |
| c14-19 | src/renderer/components/AnalyzerHistory.tsx:100 | `handleDebug(transition)`: opens a `system-prompt-debugger` tab with `{ bufferText: snapshot }` | PORTED | M:97 (debug), app-rs/crates/treeterm-model/src/workspace_store.rs:1284 (open_prompt_debugger) | Same app id and state key. Rust logs an error if the debugger app is unregistered. |
| c14-20 | src/renderer/components/AnalyzerHistory.tsx:104 | Transitions listed newest first | PORTED | M:86 (`.rev()`) | |
| c14-21 | src/renderer/components/AnalyzerHistory.tsx:109 | Heading "Activity Transitions" + "{n} entries" | PORTED | V:149, V:54 (heading) | |
| c14-22 | src/renderer/components/AnalyzerHistory.tsx:112 | Empty state "No transitions yet." | PORTED | V:125 | Same text, 13px #888 padding 8 (V:76). |
| c14-23 | src/renderer/components/AnalyzerHistory.tsx:117 | Row index counted from the oldest (`length - 1 - i`), so expansion stays on its row as new ones arrive | PORTED | V:129 | |
| c14-24 | src/renderer/components/AnalyzerHistory.tsx:120 | Row card: #1e1e1e, 1px #333 border, radius 4, padding 8/12, gap 12, Debug button at the right | PORTED | V:62 (entry) | |
| c14-25 | src/renderer/components/AnalyzerHistory.tsx:134 | Transition time (#888, mono, 11px) | PORTED | V:141 | Time format differs, see c14-8. |
| c14-26 | src/renderer/components/AnalyzerHistory.tsx:137 | `{from} → {to}` (#ccc mono) | PORTED | V:142, app-rs/crates/treeterm-core/src/activity.rs:43 (as_str) | State strings match `ActivityState` values (src/shared/types.ts:210). |
| c14-27 | src/renderer/components/AnalyzerHistory.tsx:140 | Kind pill on #555 | PORTED | V:143 | |
| c14-28 | src/renderer/components/AnalyzerHistory.tsx:141 | Extra text shown only when non-empty (#888) | PORTED | V:144 | |
| c14-29 | src/renderer/components/AnalyzerHistory.tsx:143 | Snapshot `pre`: click toggles full snapshot vs `bufferPreview`; #666 mono 11px, wraps, pointer cursor | PORTED | V:87 (fold), V:146 | |
| c14-30 | src/renderer/components/AnalyzerHistory.tsx:159 | "Debug" button per transition | PORTED | V:114 (debug_button), V:147 | Same 11px, padding 4/8, #333/#555 styling. |
| c14-31 | src/renderer/components/AnalyzerHistory.tsx:185 | Analyzer calls are a snapshot taken when the tab mounts (only Refresh re-reads them) | PARTIAL | M:90 (calls), M:67 (observe analyzer) | Rust's list is live (updates on every new call). Deliberate per the module doc (M:4), not listed in README. |
| c14-32 | src/renderer/components/AnalyzerHistory.tsx:186 | Three independent expansion sets: buffer, response, prompt | PORTED | V:17 (Fold::Buffer/Response/Prompt), V:30 (open set) | |
| c14-33 | src/renderer/components/AnalyzerHistory.tsx:190 | `handleRefresh`: re-read `getHistory()` | PORTED | V:211-213 | Rust Refresh just redraws, since the list is already live (c14-31). |
| c14-34 | src/renderer/components/AnalyzerHistory.tsx:194 | `handleDebug(entry)`: debugger tab with the call's `bufferText` | PORTED | V:204 (debug_button with buffer_text), M:97 | |
| c14-35 | src/renderer/components/AnalyzerHistory.tsx:198 | `toggleExpand`: buffer full vs preview | PORTED | V:182 (Fold::Buffer) | |
| c14-36 | src/renderer/components/AnalyzerHistory.tsx:207 | `toggleResponse`: response full vs preview | PORTED | V:192 (Fold::Response) | |
| c14-37 | src/renderer/components/AnalyzerHistory.tsx:216 | `togglePrompt`: system prompt full vs placeholder | PORTED | V:195 (Fold::Prompt) | |
| c14-38 | src/renderer/components/AnalyzerHistory.tsx:225 | Calls listed newest first | PORTED | M:93 (`.rev()`) | |
| c14-39 | src/renderer/components/AnalyzerHistory.tsx:230 | Heading "Analyzer History" + "{n} entries" | PORTED | V:210 | |
| c14-40 | src/renderer/components/AnalyzerHistory.tsx:232 | "Refresh" button (12px, padding 4/12, #333 bg, #555 border) | PORTED | V:211 (text_button "analyzer-refresh") | |
| c14-41 | src/renderer/components/AnalyzerHistory.tsx:249 | Empty state "No history entries yet." | PORTED | V:156 | |
| c14-42 | src/renderer/components/AnalyzerHistory.tsx:255 | React `key` `${timestamp}-${kind}` | NA | | React reconciliation only. |
| c14-43 | src/renderer/components/AnalyzerHistory.tsx:268 | Call time (#888 mono) | PORTED | V:168 | Time format differs, see c14-8. |
| c14-44 | src/renderer/components/AnalyzerHistory.tsx:271 | Kind pill (text = kind, colour from KIND_COLORS) | PORTED | V:162, V:169 | |
| c14-45 | src/renderer/components/AnalyzerHistory.tsx:283 | Model name (#888 mono) | PORTED | V:170 | |
| c14-46 | src/renderer/components/AnalyzerHistory.tsx:286 | "cached" pill (#2e7d32) when `entry.cached` | PORTED | M:163 (summarize), V:171 | Test M:210. |
| c14-47 | src/renderer/components/AnalyzerHistory.tsx:300 | "{durationMs}ms" pill (#555 bg, #ddd text, mono) when `durationMs !== undefined` | PORTED | M:167 (duration Some for a response, None for CallFailed), V:172 | Matches Electron: failed calls (createAnalyzerStore.ts:321/392) have no duration. |
| c14-48 | src/renderer/components/AnalyzerHistory.tsx:315 | "error" pill (#f44747) when `entry.error` is truthy | PORTED | M:174, V:175 | Rust would show the pill for an empty error string (Electron hides it); providers do not return empty errors. |
| c14-49 | src/renderer/components/AnalyzerHistory.tsx:330 | Error message line (#f44747, 12px) when `entry.error` | PORTED | V:177, app-rs/crates/treeterm-core/src/analyzer/mod.rs:196 (HistoryEntry::error, incl. "[discarded]") | |
| c14-50 | src/renderer/components/AnalyzerHistory.tsx:333 | Buffer `pre` (#666): expanded full text, else `bufferPreview` | PORTED | V:182 | |
| c14-51 | src/renderer/components/AnalyzerHistory.tsx:348 | Response `pre` shown only when `entry.response` is non-empty | PORTED | V:190 | |
| c14-52 | src/renderer/components/AnalyzerHistory.tsx:365 | Collapsed response = first 80 chars + "..." when longer than 80 | PORTED | M:135 (response_preview) | Rust counts chars, JS UTF-16 units (differs only for astral characters). Test M:193. |
| c14-53 | src/renderer/components/AnalyzerHistory.tsx:349 | Response styling: #23d18b on #1a1a1a, padding 4, radius 3 | PORTED | V:192 (`boxed`), V:103 | |
| c14-54 | src/renderer/components/AnalyzerHistory.tsx:368 | System prompt `pre` shown only when `systemPrompt` is truthy | PORTED | M:167 (non-empty -> Some), V:194 | |
| c14-55 | src/renderer/components/AnalyzerHistory.tsx:385 | Collapsed prompt text "system prompt (click to expand)"; expanded shows the prompt (#b5cea8, boxed) | PORTED | V:195-202 | Same text. |
| c14-56 | src/renderer/components/AnalyzerHistory.tsx:389 | "Debug" button per call | PORTED | V:204 | |
| c14-128 | src/renderer/components/AnalyzerHistory.tsx:88 | The transition list shows every `setTabState` call, same-state ones included (activityState.ts:57 always appends): for an AI-harness tab (idle detector off, AiHarness.tsx:100), each classification adds a row, e.g. `working → working` + "classification" + its reason, and counts toward "{n} entries" | PARTIAL | app-rs/crates/treeterm-model/src/workspace_store.rs:495 (set_activity) | (v) Added. The core logs same-state changes (activity.rs:135 set_tab_state, and analyzer/mod.rs:853 emits SetActivity on every classification), but WorkspaceStore::set_activity drops the update when the state is unchanged (`if self.activity.get(tab_id) != Some(state)`). Rust's Activity Transitions list therefore shows only state changes. Repeated classifications and their reasons, plus the prompt debugger's same-state results, are missing, and the entry count is lower. Not in README. |

## src/renderer/components/AppErrorFallback.tsx

| # | Electron | Behaviour | Status | Rust parallel | Notes |
|---|---|---|---|---|---|
| c14-57 | src/renderer/components/AppErrorFallback.tsx:3 | `AppErrorFallbackProps { error?, onReload }` | NA | | React props. |
| c14-58 | src/renderer/components/AppErrorFallback.tsx:12 | Full-window fallback shown when the renderer throws during render (App.tsx:103 ErrorBoundary): "Application Error" + "The application encountered an unexpected error and needs to reload." | MISSING | | (v) Re-searched app-rs/crates for "Application Error", "needs to reload", "unexpected error", `set_hook`, `take_hook`, `catch_unwind`, `panic_hook`, crash, and checked app-rs/Cargo.toml for a `panic =` profile: nothing. A panic in a GPUI render ends the process and the window disappears with no message. The daemon keeps the PTYs, so relaunching recovers the session. No in-app error screen exists. |
| c14-59 | src/renderer/components/AppErrorFallback.tsx:17 | Error details block "Error: {message}" when `error` is given | NA | | Unreachable in the app: App.tsx:103 passes a static `fallback` element without `error`, and ErrorBoundary.tsx:50 renders it as is (only tests pass `error`). |
| c14-60 | src/renderer/components/AppErrorFallback.tsx:23 | "Show Stack Trace" / "Hide Stack Trace" toggle when `error.stack` | NA | | Unreachable, as c14-59. |
| c14-61 | src/renderer/components/AppErrorFallback.tsx:31 | Stack `pre` shown while `showStack` | NA | | Unreachable, as c14-59. |
| c14-62 | src/renderer/components/AppErrorFallback.tsx:39 | "Reload Application" button -> `location.reload()` (reloads the UI; main process and daemon keep running) | MISSING | | (v) Re-searched "Reload Application", "relaunch", "restart_app", "reload_window", "fn reload" in app-rs/crates: only the Browser tab's page reload (browser_view.rs:525), settings/review/process reloads. The equivalent recovery is relaunching the app by hand, which reconnects to the still-running daemon. This only matters together with c14-58. |

## src/renderer/components/AutoOpenWorktreesDialog.tsx

| # | Electron | Behaviour | Status | Rust parallel | Notes |
|---|---|---|---|---|---|
| c14-63 | src/renderer/components/AutoOpenWorktreesDialog.tsx:9 | Props: root workspace, open worktree paths, `onConfirm`, `onCancel` | PORTED | D:391 (open), app-rs/crates/treeterm-app/src/tree_pane.rs:590 (menu item) | Rust takes session + root id; the menu item is shown for non-worktree git workspaces. |
| c14-64 | src/renderer/components/AutoOpenWorktreesDialog.tsx:18 | `DetectedRow { node, entry \| null }` | PORTED | D:17 (Row) | Row flattens the node and adds `depth`. |
| c14-65 | src/renderer/components/AutoOpenWorktreesDialog.tsx:40 | Root anchor = root path + `gitBranch ?? ''` | PORTED | D:83-84 | `unwrap_or_default()`. |
| c14-66 | src/renderer/components/AutoOpenWorktreesDialog.tsx:33 | "Include unknown" starts unchecked | PORTED | D:66 | |
| c14-67 | src/renderer/components/AutoOpenWorktreesDialog.tsx:36 | Starts in the loading state | PORTED | D:67 (Detection::Loading) | |
| c14-68 | src/renderer/components/AutoOpenWorktreesDialog.tsx:46 | Excluded "already open" paths = open workspaces with `isWorktree` (SessionPanel.tsx:165) | PARTIAL | D:86-90 | Rust excludes the path of every open workspace, worktree or not. E.g. an open main checkout (non-worktree workspace) is listed by Electron (then skipped by its store) and hidden by Rust. Benign. |
| c14-69 | src/renderer/components/AutoOpenWorktreesDialog.tsx:93 | Detection re-runs when the include toggle, root path, root branch or set of open worktree paths change | PARTIAL | D:72 (on open), D:354 (on toggle) | Rust re-detects only on the toggle; a workspace opened/closed (or a root branch change) while the dialog is up does not refresh the list. |
| c14-70 | src/renderer/components/AutoOpenWorktreesDialog.tsx:49 | `cancelled` flag set on cleanup (:92): a superseded or unmounted load is ignored | PORTED | D:92 (`_load` replaced, dropping the old task), D:134 | |
| c14-71 | src/renderer/components/AutoOpenWorktreesDialog.tsx:51 | Each load sets loading and clears the error | PARTIAL | D:77 (load) | Rust resets `detection` (so a detection error clears) but not `self.error`, which takes precedence (D:295-299). (v) Note rewritten: `self.error` is only ever "Root workspace not found" (the sole synchronous Err of S:1957 auto_open_worktrees), and in that state `load` fails with the same text. The stale error is only visible if the root store returns while the dialog is open (e.g. a reconnect rebuild) and the user then toggles "Include unknown". |
| c14-72 | src/renderer/components/AutoOpenWorktreesDialog.tsx:54 | List worktrees and registry entries in parallel; either failing fails the load | PORTED | D:94-98, app-rs/crates/treeterm-core/src/worktree_registry.rs:287 (list) | Sequential in Rust. Rust uses an empty registry when the session has none (S:1919). Rust lists from `git_root_path ?? path`, Electron from the root path: same worktree set. |
| c14-73 | src/renderer/components/AutoOpenWorktreesDialog.tsx:59 | Registry entries keyed by path | PORTED | D:99 | |
| c14-74 | src/renderer/components/AutoOpenWorktreesDialog.tsx:65 | Filter: drop the root and already-open paths | PORTED | D:102 | Set of open paths differs, see c14-68. |
| c14-75 | src/renderer/components/AutoOpenWorktreesDialog.tsx:66 | Filter: worktrees in the registry are kept | PORTED | D:103 | |
| c14-76 | src/renderer/components/AutoOpenWorktreesDialog.tsx:67 | Filter: unknown worktrees kept only with "Include unknown" | PORTED | D:103 | |
| c14-77 | src/renderer/components/AutoOpenWorktreesDialog.tsx:70 | Place candidates in a hierarchy with `detectWorktreeHierarchy` (git `isAncestor`) | PORTED | D:106-110, app-rs/crates/treeterm-core/src/worktree_hierarchy.rs (detect_worktree_hierarchy) | |
| c14-78 | src/renderer/components/AutoOpenWorktreesDialog.tsx:75 | Stale result after detection is dropped (`if (cancelled) return`) | PORTED | D:92 | The dropped task never reaches D:124. |
| c14-79 | src/renderer/components/AutoOpenWorktreesDialog.tsx:77 | Each node paired with its registry entry `?? null` | PORTED | D:116 | |
| c14-80 | src/renderer/components/AutoOpenWorktreesDialog.tsx:82 | Every detected row checked by default | PORTED | D:127 | |
| c14-81 | src/renderer/components/AutoOpenWorktreesDialog.tsx:83 | On failure: "Failed to detect worktrees: {message}", rows cleared (ignored if cancelled) | PORTED | D:130 (Detection::Failed), D:297 | Same text. |
| c14-82 | src/renderer/components/AutoOpenWorktreesDialog.tsx:87 | `finally`: loading off (unless cancelled) | PORTED | D:125 | Detection replaced by Loaded/Failed. |
| c14-83 | src/renderer/components/AutoOpenWorktreesDialog.tsx:95 | `toggleChecked(path)` | PORTED | D:284-289 | |
| c14-84 | src/renderer/components/AutoOpenWorktreesDialog.tsx:105 | Confirm: node and entry maps by path | PORTED | D:145 | |
| c14-85 | src/renderer/components/AutoOpenWorktreesDialog.tsx:110 | `resolveParent`: walk up to the nearest checked ancestor, else null (root) | PORTED | D:149-163 | Rust adds a hop cap. No Rust test for this (Electron's test file covers it at AutoOpenWorktreesDialog.test.tsx:115). |
| c14-86 | src/renderer/components/AutoOpenWorktreesDialog.tsx:119 | Only checked rows become items | PORTED | D:164-166 | |
| c14-87 | src/renderer/components/AutoOpenWorktreesDialog.tsx:124 | Item path and branch | PORTED | D:168-169 | |
| c14-88 | src/renderer/components/AutoOpenWorktreesDialog.tsx:126 | Item `name` = registry displayName `??` branch; it becomes the new workspace's `name` (createSessionStore.ts:1660) | PARTIAL | S:2258 (AutoOpenItem, no name), S:1787 (adopt_worktree_with: name_from_path) | Rust names the workspace after the last path component. A worktree without a registry displayName is labelled by its directory (e.g. "fancy-moons-jump") in Rust's tree, by its branch in Electron's; the workspace file's `name` differs too. |
| c14-89 | src/renderer/components/AutoOpenWorktreesDialog.tsx:127 | Item `parentPath` = `resolveParent` | PORTED | D:170 | |
| c14-90 | src/renderer/components/AutoOpenWorktreesDialog.tsx:128 | Item displayName / description from the registry entry (absent -> undefined) | PORTED | D:171-172, S:1981 | Metadata gets only non-empty values, as Electron's truthiness spread. |
| c14-91 | src/renderer/components/AutoOpenWorktreesDialog.tsx:133 | Processing phase: `isProcessing` on, error cleared, dialog stays open until the whole batch is created | PARTIAL | D:141 (confirm), D:380-383, S:1957 (auto_open_worktrees spawns) | Rust has no processing state: confirm schedules the batch and the dialog closes at once. Progress shows as "Opening worktree..." entries in the tree (S:1793). (v) Also: Electron holds the session lock for the whole batch (createSessionStore.ts:1610). Rust takes it once per child inside adopt_worktree_with (S:1768), so another window can write between children. |
| c14-92 | src/renderer/components/AutoOpenWorktreesDialog.tsx:135 | Failure: show `result.error` in the dialog and keep it open (root missing, or session lock not acquired, e.g. "Session is locked by another window") | PARTIAL | D:181-185, S:1957 | Rust can only fail synchronously with "Root workspace not found". Lock and adopt failures happen per child after the dialog has closed. A lock failure turns that child into an error entry in the tree (S:1803 -> S:1827 fail_child). An adopt error (e.g. parent not found) is only logged (S:1993). (v) Refs fixed: S:1991 -> S:1993. Lock example added. |
| c14-93 | src/renderer/components/AutoOpenWorktreesDialog.tsx:137 | Fallback message "Failed to open worktrees" when the failure has no message | NA | | (v) Justification tightened: every failure of `autoOpenWorktrees` carries a string (`acquireLock` is typed `{acquired:false; error: string}`, createSessionStore.ts:220, and "Root workspace not found"). `??` does not fall back for an empty string, so the fallback is dead code. Rust's `Result<(), String>` likewise always has a message. |
| c14-94 | src/renderer/components/AutoOpenWorktreesDialog.tsx:136 | Success closes the dialog (SessionPanel.tsx:680) | PORTED | D:381-382 | Closes immediately in Rust (see c14-91). |
| c14-95 | src/renderer/components/AutoOpenWorktreesDialog.tsx:142 | Escape cancels | PORTED | app-rs/vendor/gpui-component/src/dialog/dialog.rs:226 (`keyboard: true`), :737 (close_on_escape) | Kit default, not overridden by ui::dialog_frame. |
| c14-96 | src/renderer/components/AutoOpenWorktreesDialog.tsx:148 | Group rows by `parentPath` (null = under the root) | PORTED | D:190 (ordered) | |
| c14-97 | src/renderer/components/AutoOpenWorktreesDialog.tsx:156 | Render depth-first: each row then its children, recursively | PORTED | D:191-199 (walk), D:43 (depths) | Rust also appends rows whose parent is not listed (D:203); Electron would drop them (unreachable in practice). |
| c14-98 | src/renderer/components/AutoOpenWorktreesDialog.tsx:165 | Indent `12 + depth * 20` px | PORTED | D:250 | Test D:401 (depths). |
| c14-99 | src/renderer/components/AutoOpenWorktreesDialog.tsx:166 | Clicking a row toggles it | PORTED | D:284 | |
| c14-100 | src/renderer/components/AutoOpenWorktreesDialog.tsx:166 | Row clicks ignored while processing | PARTIAL | D:284 | No processing phase in Rust (c14-91); the dialog is closed by then. |
| c14-101 | src/renderer/components/AutoOpenWorktreesDialog.tsx:168 | Row checkbox shows the checked state; its own click toggles without double-toggling (`stopPropagation`) | PORTED | D:256 (ui::checkbox_box, drawn), D:284 | The whole row is the hit target in Rust. |
| c14-102 | src/renderer/components/AutoOpenWorktreesDialog.tsx:173 | Row checkboxes disabled while processing | PARTIAL | D:256 (`disabled` always false) | See c14-91. |
| c14-103 | src/renderer/components/AutoOpenWorktreesDialog.tsx:160 | Row name = registry displayName `??` branch | PORTED | D:238-242 | |
| c14-104 | src/renderer/components/AutoOpenWorktreesDialog.tsx:177 | Branch label | PORTED | D:268 | |
| c14-105 | src/renderer/components/AutoOpenWorktreesDialog.tsx:178 | "unknown" badge when the worktree is not in the registry | PORTED | D:269-283 | |
| c14-106 | src/renderer/components/AutoOpenWorktreesDialog.tsx:185 | Checked count = size of the checked set | PARTIAL | D:301-304 | (v) PORTED -> PARTIAL. Rust counts the checked rows of the current load. While re-detecting after toggling "Include unknown", the label reads "Ok (0)"; Electron keeps the previous count, disabled. After a failed load, Rust shows "Ok (0)" disabled. Electron keeps the stale count with Ok enabled, and clicking it confirms an empty batch that succeeds and closes the dialog. |
| c14-107 | src/renderer/components/AutoOpenWorktreesDialog.tsx:188 | Clicking the overlay cancels; clicks inside do not | PORTED | app-rs/vendor/gpui-component/src/dialog/dialog.rs:229 (`overlay_closable: true`), :738 | |
| c14-108 | src/renderer/components/AutoOpenWorktreesDialog.tsx:191 | Title "Auto Open All Recent Worktrees" | PORTED | D:308 | 400px wide (D:393), as `.create-child-dialog`. |
| c14-109 | src/renderer/components/AutoOpenWorktreesDialog.tsx:192 | Close button "x" cancels | PORTED | D:308, app-rs/crates/treeterm-app/src/ui.rs:299 (dialog_header) | |
| c14-110 | src/renderer/components/AutoOpenWorktreesDialog.tsx:196 | "Under:" + the root workspace's name | PORTED | D:217-222, D:323-324 | Uses `file().name`, as Electron's `rootWs.name`. |
| c14-111 | src/renderer/components/AutoOpenWorktreesDialog.tsx:202 | Loading: "Detecting worktrees..." | PORTED | D:227 | |
| c14-112 | src/renderer/components/AutoOpenWorktreesDialog.tsx:204 | Empty (or failed): "No recent worktrees found" | PORTED | D:228-229 | |
| c14-113 | src/renderer/components/AutoOpenWorktreesDialog.tsx:206 | Row list (max height 200, scrolls) | PORTED | D:326-338 | |
| c14-114 | src/renderer/components/AutoOpenWorktreesDialog.tsx:211 | "Include unknown worktrees (not in recent history)" checkbox toggles and re-detects | PORTED | D:339-355 | Same text. |
| c14-115 | src/renderer/components/AutoOpenWorktreesDialog.tsx:217 | Include-unknown checkbox disabled while processing | PARTIAL | D:350 | See c14-91. |
| c14-116 | src/renderer/components/AutoOpenWorktreesDialog.tsx:223 | Error box when there is an error | PORTED | D:295-299, D:357-369 | |
| c14-117 | src/renderer/components/AutoOpenWorktreesDialog.tsx:227 | "Cancel" button cancels | PORTED | D:374-375 | |
| c14-118 | src/renderer/components/AutoOpenWorktreesDialog.tsx:227 | Cancel disabled while processing | PARTIAL | D:374 | See c14-91. |
| c14-119 | src/renderer/components/AutoOpenWorktreesDialog.tsx:233 | Ok disabled while loading or with nothing checked | PORTED | D:306 | |
| c14-120 | src/renderer/components/AutoOpenWorktreesDialog.tsx:233 | Ok disabled while processing | PARTIAL | D:306 | See c14-91. |
| c14-121 | src/renderer/components/AutoOpenWorktreesDialog.tsx:235 | Ok label "Ok ({n})" | PORTED | D:378 | |
| c14-122 | src/renderer/components/AutoOpenWorktreesDialog.tsx:235 | Ok label "Opening... Please wait" while processing | PARTIAL | app-rs/crates/treeterm-model/src/session_store.rs:1793 (adopt_worktree_with "Opening worktree..."), app-rs/crates/treeterm-app/src/tree_pane.rs:654 (Label::Loading) | (v) MISSING -> PARTIAL. Searched "Please wait", "Opening", "processing"/"submitting" in the dialogs. The label does not exist, but progress feedback does: each queued worktree appears at once as a loading entry in the tree ("Opening worktree..." in its pane) after the dialog closes. Follows from c14-91. |
| c14-123 | src/renderer/components/AutoOpenWorktreesDialog.tsx:232 | Ok click -> `handleConfirm` | PORTED | D:380-384 | |

## src/renderer/components/AutoScrollPre.tsx

| # | Electron | Behaviour | Status | Rust parallel | Notes |
|---|---|---|---|---|---|
| c14-124 | src/renderer/components/AutoScrollPre.tsx:4 | `AutoScrollPre` component props (`className`, children in a `pre`) | NA | | React component plumbing. |
| c14-125 | src/renderer/components/AutoScrollPre.tsx:7 | `if (ref.current)` guard | NA | | React ref mechanics. |
| c14-126 | src/renderer/components/AutoScrollPre.tsx:8 | After every render, scroll to the bottom: Review merge output (ReviewBrowser.tsx:1745) | PORTED | app-rs/crates/treeterm-app/src/review_view.rs:681 (merge_scroll.scroll_to_bottom) | Rust scrolls when the output grows, not on every render (a reader who scrolled up keeps the position until new output). |
| c14-127 | src/renderer/components/AutoScrollPre.tsx:8 | After every render, scroll to the bottom: workspace loading output while a worktree is created (WorkspacePane.tsx:314) | PARTIAL | app-rs/crates/treeterm-app/src/app_view.rs:1764 ("workspace-loading-output") | Rust shows the last 200 lines in a scroll box with no scroll handle and no scroll-to-bottom, so once the output is taller than the box the newest lines are below the fold. |

## Gaps

- c14-8, PARTIAL, src/renderer/components/AnalyzerHistory.tsx:45: times follow the OS locale in Electron (12-hour "02:15:30 PM" on en-US); Rust always prints 24-hour `HH:MM:SS`. Nearest: M:181 (clock) -> app-rs/crates/treeterm-core/src/clock.rs:45 (time_of_day).
- c14-31, PARTIAL, src/renderer/components/AnalyzerHistory.tsx:185: Electron's analyzer call list is a snapshot until Refresh; Rust's is live (a superset). Deliberate per module doc (M:4), not in README. Nearest: M:90 (calls), M:67 (observe analyzer).
- c14-58, MISSING, src/renderer/components/AppErrorFallback.tsx:12: no "Application Error" crash screen; a GPUI panic ends the process (daemon survives; relaunch recovers). Nearest: none.
- c14-62, MISSING, src/renderer/components/AppErrorFallback.tsx:39: no "Reload Application" action; recovery is relaunching the app (follows c14-58). Nearest: none.
- c14-68, PARTIAL, src/renderer/components/AutoOpenWorktreesDialog.tsx:46: Rust excludes every open workspace path, Electron only open worktree workspaces (a path open as a plain workspace is listed by Electron, then skipped on confirm). Nearest: D:86-90.
- c14-69, PARTIAL, src/renderer/components/AutoOpenWorktreesDialog.tsx:93: no re-detection when workspaces open/close or the root's path/branch change while the dialog is up; only the include-unknown toggle re-detects. Nearest: D:72 (on open), D:354 (on toggle).
- c14-71, PARTIAL, src/renderer/components/AutoOpenWorktreesDialog.tsx:51: re-detecting does not clear a previous confirm error (only "Root workspace not found", visible only if the root store comes back while the dialog is open). Nearest: D:77 (load).
- c14-88, PARTIAL, src/renderer/components/AutoOpenWorktreesDialog.tsx:126: auto-opened workspaces without a registry displayName are named after their directory (name_from_path), not their branch: different tree label and workspace-file `name`. Nearest: S:2258 (AutoOpenItem, no name), S:1787 (adopt_worktree_with: name_from_path).
- c14-91, PARTIAL, src/renderer/components/AutoOpenWorktreesDialog.tsx:133: no processing phase: the dialog closes at once and the batch runs in the background (session lock per child instead of one lock for the batch). Nearest: D:141 (confirm), D:380-383, S:1957 (auto_open_worktrees spawns).
- c14-92, PARTIAL, src/renderer/components/AutoOpenWorktreesDialog.tsx:135: lock/adopt failures are not shown in the dialog (lock failure becomes a tree error entry, adopt failure is only logged); only "Root workspace not found" is. Nearest: D:181-185, S:1957.
- c14-100, PARTIAL, src/renderer/components/AutoOpenWorktreesDialog.tsx:166: row clicks not blocked while processing; there is no processing phase (c14-91). Nearest: D:284.
- c14-102, PARTIAL, src/renderer/components/AutoOpenWorktreesDialog.tsx:173: row checkboxes never disabled (c14-91). Nearest: D:256 (`disabled` always false).
- c14-106, PARTIAL, src/renderer/components/AutoOpenWorktreesDialog.tsx:185: Ok count reads 0 while re-detecting and after a failed load (Electron keeps the previous count; after a failure its Ok stays enabled and confirms an empty batch). Nearest: D:301-304.
- c14-115, PARTIAL, src/renderer/components/AutoOpenWorktreesDialog.tsx:217: include-unknown never disabled (c14-91). Nearest: D:350.
- c14-118, PARTIAL, src/renderer/components/AutoOpenWorktreesDialog.tsx:227: Cancel never disabled (c14-91). Nearest: D:374.
- c14-120, PARTIAL, src/renderer/components/AutoOpenWorktreesDialog.tsx:233: Ok not disabled for processing (c14-91). Nearest: D:306.
- c14-122, PARTIAL, src/renderer/components/AutoOpenWorktreesDialog.tsx:235: no "Opening... Please wait" label; progress shows as "Opening worktree..." loading entries in the tree after the dialog closes (c14-91). Nearest: app-rs/crates/treeterm-model/src/session_store.rs:1793 (adopt_worktree_with "Opening worktree..."), app-rs/crates/treeterm-app/src/tree_pane.rs:654 (Label::Loading).
- c14-127, PARTIAL, src/renderer/components/AutoScrollPre.tsx:8: the workspace loading output (git output while a worktree is created) does not stay scrolled to the newest line. Nearest: app-rs/crates/treeterm-app/src/app_view.rs:1764 ("workspace-loading-output").
- c14-128, PARTIAL, src/renderer/components/AnalyzerHistory.tsx:88: the Activity Transitions list omits same-state transitions (every repeated classification and its reason), because WorkspaceStore::set_activity drops unchanged states; Electron logs every setTabState call. Nearest: app-rs/crates/treeterm-model/src/workspace_store.rs:495 (set_activity).
