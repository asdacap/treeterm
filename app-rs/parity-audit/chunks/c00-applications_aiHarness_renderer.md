# c00-applications_aiHarness_renderer

Verified: +0 rows, 12 rows changed

Scope: the per-application definitions in `src/applications/*/renderer.ts(x)` plus `browser/url.ts` and the barrel `index.ts`. The Rust counterpart is mainly the registry `app-rs/crates/treeterm-model/src/apps.rs` (`AppKind`, `ToolApp`, `AppDef`, `AppRegistry::from_settings`) and the tab lifecycle in `app-rs/crates/treeterm-model/src/workspace_store.rs` (`init_tab` = `onWorkspaceLoad`, `remove_tab` = `ref.close()`, `dispose_tab` = `ref.dispose()`). Components rendered by these apps (AiHarness.tsx, BrowserPane.tsx, ReviewBrowser.tsx, ...) belong to other chunks. Here only the props and wiring are checked.

Cross-cutting facts I verified:
- `displayStyle: 'flex'` is the CSS display of the tab container. It has no Rust field and no user-visible effect, so every such row is NA.
- `isDefault` has no runtime consumer in Electron (`getDefaultApplications` in src/renderer/store/app.ts:182 is called only from tests) or in Rust (`AppDef::is_default` is never read outside apps.rs). Those rows are PORTED because the flag is carried with the same value.
- `canClose` is read by Rust `remove_tab` (workspace_store.rs:1359) and the tab strip's close button (app_view.rs:1805).
- `showInNewTabMenu` is read by `AppRegistry::new_tab_menu` (apps.rs:586) / `new_tab_menu_for` (apps.rs:591), which feeds the "+" menu (app_view.rs:1866).
- Only the active tab's content is rendered in Rust (app_view.rs:1917-1999), which matches the `isVisible` props.
- The daemon ignores `CreatePtyRequest.sandbox`: daemon-rs has no sandbox code, and grpc_server.rs:63-80 never reads it. Rust sends `sandbox: None` (core daemon.rs:158).

## src/applications/aiHarness/renderer.ts

| # | Electron | Behaviour | Status | Rust parallel | Notes |
|---|---|---|---|---|---|
| c00-1 | src/applications/aiHarness/renderer.ts:8 | `TerminalDeps`: injected `terminal.kill(connectionId, sessionId)` used to kill the tab's PTY | PORTED | app-rs/crates/treeterm-model/src/terminal.rs:613 (TerminalModel::kill) -> app-rs/crates/treeterm-core/src/daemon.rs:170 (kill_pty) | DI plumbing is NA. The capability (KillPty RPC) exists, and Rust also shows "Failed to kill terminal: ..." on failure. |
| c00-2 | src/applications/aiHarness/renderer.ts:10-12 | `AiHarnessRef` = TerminalAppRef + the tab's `analyzer` | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:210-219 (TerminalTab::Attached { model, analyzer }) | Exposed via `WorkspaceStore::analyzer` (workspace_store.rs:367). |
| c00-3 | src/applications/aiHarness/renderer.ts:15 | One application per `settings.aiHarness.instances[]` entry (`createAiHarnessVariant`) | PORTED | app-rs/crates/treeterm-model/src/apps.rs:497-513 (AppRegistry::from_settings) | Rebuilt on settings change (WorkspaceStore::apply_settings). |
| c00-4 | src/applications/aiHarness/renderer.ts:17 | id `aiharness-<instance.id>` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:499 | Same id, so tabs open in both apps. |
| c00-5 | src/applications/aiHarness/renderer.ts:18-19 | name and icon from the instance (new-tab menu, tab strip icon) | PORTED | app-rs/crates/treeterm-model/src/apps.rs:500-501 | |
| c00-6 | src/applications/aiHarness/renderer.ts:22-23 | Initial state: `ptyId: null`, fresh `ptyHandle` UUID per tab | PORTED | app-rs/crates/treeterm-model/src/apps.rs:128,160-161 (AppDef::initial_state) | Test apps.rs:692 (terminal_tabs_start_without_a_pty_and_with_a_fresh_handle). |
| c00-7 | src/applications/aiHarness/renderer.ts:24 | Initial `keepOnExit` = instance.keepOnExit | PORTED | app-rs/crates/treeterm-model/src/apps.rs:162; read at workspace_store.rs:920 (terminal_options) | Test apps.rs:722. |
| c00-8 | src/applications/aiHarness/renderer.ts:25-29 | Initial `sandbox: { enabled: enableSandbox, allowNetwork, allowedPaths: [] }` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:163 | Test apps.rs:723. |
| c00-9 | src/applications/aiHarness/renderer.ts:30-32 | Initial `autoApprove: false`, `idleDetectorDisabled: false`, `widthLimitDisabled: false` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:164-166; read at workspace_store.rs:983 (autoApprove), :921 (widthLimitDisabled), :944 (idleDetectorDisabled) | |
| c00-10 | src/applications/aiHarness/renderer.ts:36-38 | `initAnalyzer(tab.id)`: every AI-harness tab gets an LLM analyzer | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:971-999 (start_analyzer, called from attach_terminal :937) | Only for `AppKind::AiHarness` (:979). Created when the terminal attaches rather than at load. Skipped when no LLM client could be built (:982; app_store.rs:97-103 logs "AI-harness analysis is off"). Deliberate (README): it reads the tab's own terminal, not a second headless one. |
| c00-11 | src/applications/aiHarness/renderer.ts:42 | Hold the in-flight creation (`creating`) so close() can still reach it | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:211-214 (TerminalTab::Creating { handle, _task }) | See c00-22 for what close() does with it. |
| c00-12 | src/applications/aiHarness/renderer.ts:46,59 | `disposed` guard: a creation resolving after dispose() neither writes state nor restarts the analyzer | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:1119-1120 (dispose_tab drops the TabRef, cancelling the Creating task) | on_pty_created never runs after dispose, so no analyzer is started. The daemon re-finds the PTY by handle on re-init. |
| c00-13 | src/applications/aiHarness/renderer.ts:48-51 | Restore (`ptyId` set): pre-open a background TTY writer so promptHarness does not open a second stream | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:829 (init_tab attaches every restored terminal tab at load), :424-443 (running_harness), :400-410 (prompt_harness) | (v) Was NA. The writer cache is Electron plumbing, but the capability behind it, prompting a restored harness tab that has not been opened yet, exists. init_tab attaches every terminal tab's TerminalModel at workspace load, and prompt_harness pastes through that model. |
| c00-14 | src/applications/aiHarness/renderer.ts:52 | Restore: start the analyzer on the existing PTY | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:829 (attach_terminal) -> :937 (start_analyzer) | |
| c00-15 | src/applications/aiHarness/renderer.ts:56 | New tab: handle = `state.ptyHandle ?? randomUUID()` | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:831-834 | |
| c00-16 | src/applications/aiHarness/renderer.ts:57 | `ensureTty(handle, workspace path, state.sandbox, instance.command)`: create (or re-find by handle) the PTY running the harness command | SHARED | app-rs/crates/treeterm-model/src/workspace_store.rs:1005-1031 (create_pty) -> app-rs/crates/treeterm-core/src/daemon.rs:151 (CreatePty RPC with handle); command from apps.rs:176 | Same daemon RPC and idempotency key. Sandbox is not sent (daemon.rs:158), but the daemon ignores it (daemon-rs grpc_server.rs:63-80), so behaviour is the same. Electron's per-store memo of in-flight creations is in createWorkspaceStore (another chunk). |
| c00-17 | src/applications/aiHarness/renderer.ts:58 | Creation rejected: no handler (unhandled rejection; the tab keeps "Starting AI harness...") | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:1036-1046 (on_pty_created Err -> TerminalTab::Failed), app_view.rs:1993 | Rust shows "Failed to start terminal: <error>", which improves on Electron. |
| c00-18 | src/applications/aiHarness/renderer.ts:60-65 | On creation: write `ptyId`, `ptyHandle` and `connectionId` into the tab state (persisted) | PARTIAL | app-rs/crates/treeterm-model/src/workspace_store.rs:1059-1062 (on_pty_created) | Rust writes `ptyId` and `ptyHandle` but never `connectionId`. Harmless: Electron's close() falls back to `ws.connectionId`, and Rust kills through the workspace's own connection. |
| c00-19 | src/applications/aiHarness/renderer.ts:66 | Start the analyzer once the new PTY exists | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:1063 (attach_terminal) -> :937 (start_analyzer) | |
| c00-20 | src/applications/aiHarness/renderer.ts:72-80 | `cachedTerminal` / `disposeCachedTerminal`: the terminal (engine + TTY subscription) survives view unmount and is torn down only on dispose | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:210-220 (TerminalModel owned by the store's TabRef); app_view.rs:1921-1924 (view cache by entity) | The model lives as long as the tab and is dropped in dispose_tab. The xterm caching itself is library glue. |
| c00-21 | src/applications/aiHarness/renderer.ts:81-85 | close(): read the current tab state for an up-to-date `ptyId`/`connectionId`, falling back to load-time values | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:1352-1368 (remove_tab matches the live TabRef) | The attached model carries the live pty id. |
| c00-22 | src/applications/aiHarness/renderer.ts:86-88 | close() with a known PTY: kill it on its connection | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:1366-1367 -> terminal.rs:613 (kill) | KillPty RPC. close_all_tabs (:1650) does the same when a workspace is removed. |
| c00-23 | src/applications/aiHarness/renderer.ts:90-91 | close() while the PTY is still being created: kill it as soon as the creation resolves | PARTIAL | app-rs/crates/treeterm-model/src/workspace_store.rs:1369-1372 (remove_tab, Creating arm), :1048-1058 (on_pty_created "closed while starting" kill) | (v) Confirmed. Electron's removeTab calls close() and then dispose() (createWorkspaceStore.ts:500-501), and the kill in close() still runs because `disposed` only guards the state write. In Rust the kill branch is unreachable. remove_tab calls dispose_tab (:1383 -> :1120), which drops `TerminalTab::Creating`. Dropping its GPUI Task cancels it, and dropping the CreatePty `Fut` aborts its tokio task (core rt.rs:72-80 AbortOnDrop), so on_pty_created never runs. If the daemon has already received the CreatePty, the harness process is orphaned and shows up only in the TTYs tab. close_all_tabs (:1661) relies on the same dead path. No test covers it. |
| c00-24 | src/applications/aiHarness/renderer.ts:93-98 | dispose(): mark disposed, dispose the cached terminal, stop the analyzer, drop the tab's activity state | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:1119-1124 (dispose_tab: drops terminal + analyzer, `activity.remove_tab_state`) | |
| c00-25 | src/applications/aiHarness/renderer.ts:104-106 | Render guard: state that is not an AiHarnessState (e.g. no `sandbox` object) renders nothing | PARTIAL | app-rs/crates/treeterm-model/src/workspace_store.rs:827-840 (no shape check; state read leniently) | Benign: Rust shows the terminal where Electron shows an empty pane. No action needed. |
| c00-26 | src/applications/aiHarness/renderer.ts:107-114 | Render the AiHarness view (terminal, prompt buttons, status bar) for the tab | PORTED | app-rs/crates/treeterm-app/src/app_view.rs:1920-1940 (terminal content, harness_prompt_buttons :2380, terminal_status_bar :2417) | `key` is React-only. `cwd`, `sandbox`, `isVisible` and `command` are passed but never read by AiHarness.tsx. Component behaviour is in another chunk. |
| c00-27 | src/applications/aiHarness/renderer.ts:115 | `backgroundColor` of the instance becomes the terminal background | PARTIAL | app-rs/crates/treeterm-model/src/apps.rs:187-199 (AppDef::background); app-rs/crates/treeterm-model/src/workspace_store.rs:260-268 (parse_hex_color), :907-918 (terminal_options) | (v) Was PORTED. Rust uses the colour only when it is `#rrggbb`. Electron's field is free text (placeholder "Background color (e.g., #1a1a24)", SettingsDialog.tsx:444-459), and the value goes straight to xterm's theme and to the padding wrapper's CSS (BaseTerminal.tsx:659), so `#000`, `black` or `rgb(...)` all work there. In Rust any such value falls back to #1a1a2e when the instance's sandbox is on, or to #1e1e1e otherwise. Electron's AiHarness never uses the sandbox colour. |
| c00-28 | src/applications/aiHarness/renderer.ts:116 | `disableScrollbar`: hide the terminal scrollbar (`.disable-scrollbar .xterm-viewport { overflow: hidden }`, styles/index.css:1399-1404) so the viewport does not consume wheel events and they reach the harness program | MISSING | app-rs/crates/treeterm-core/src/settings/mod.rs:255 and settings/read.rs:362 (parsed only); app-rs/crates/treeterm-app/src/settings_view/instance.rs:276 ("Disable Scrollbar" toggle); app-rs/crates/treeterm-app/src/terminal_element.rs:320 (scrollbar always built) | (v) Confirmed MISSING. Searched `disable_scrollbar`, `disableScrollbar`, `hide_scrollbar`/`show_scrollbar`, `scrollbar` in terminal_element.rs, terminal_view.rs and model terminal.rs, and `git log -- app-rs \| grep -i scrollbar` (only aef138f, which added the always-on scrollbar). The setting is parsed and edited in Settings, but `AppKind::AiHarness` (apps.rs:15-21) and TerminalOptions have no field for it, so the toggle is saved and does nothing. Harness tabs always show the scrollback scrollbar, and outside mouse mode the wheel scrolls the scrollback (terminal_view.rs:352-380). |
| c00-29 | src/applications/aiHarness/renderer.ts:120 | `canClose: true` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:509 | |
| c00-30 | src/applications/aiHarness/renderer.ts:121 | `showInNewTabMenu: true` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:510 | |
| c00-31 | src/applications/aiHarness/renderer.ts:122 | `displayStyle: 'flex'` | NA | | CSS display of the tab container. |
| c00-32 | src/applications/aiHarness/renderer.ts:123 | `isDefault: instance.isDefault` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:511 | No runtime consumer in either app. |

## src/applications/analyzerHistory/renderer.ts

| # | Electron | Behaviour | Status | Rust parallel | Notes |
|---|---|---|---|---|---|
| c00-33 | src/applications/analyzerHistory/renderer.ts:6 | id `analyzer-history` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:477 | Test apps.rs:767. |
| c00-34 | src/applications/analyzerHistory/renderer.ts:7-8 | name "Activity History", icon 📊 | PORTED | app-rs/crates/treeterm-model/src/apps.rs:477 | `\u{1F4CA}`. |
| c00-35 | src/applications/analyzerHistory/renderer.ts:9 | Initial state `{}` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:61-71 (ToolApp::initial_state) | `sourceTabId` is added by the opener (workspace_store.rs:1299-1300). |
| c00-36 | src/applications/analyzerHistory/renderer.ts:10 | onWorkspaceLoad: no daemon resources (close/dispose no-ops) | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:857-862 (init_tool_tab); remove_tab does nothing for `TabRef::Tool` | |
| c00-37 | src/applications/analyzerHistory/renderer.ts:11 | Render the AnalyzerHistory component with the tab's props | PORTED | app-rs/crates/treeterm-app/src/tool_views/analyzer_history_view.rs:28; model app-rs/crates/treeterm-model/src/tool_tabs/analyzer_history.rs:25 | Component behaviour is audited elsewhere. Rust lists are live, not refreshed by a button. |
| c00-38 | src/applications/analyzerHistory/renderer.ts:12 | `canClose: true` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:469 (tool closure) | |
| c00-39 | src/applications/analyzerHistory/renderer.ts:13 | `showInNewTabMenu: false` (opened only from the activity badge) | PORTED | app-rs/crates/treeterm-model/src/apps.rs:477 | Test apps.rs:783. Opener: workspace_store.rs:1299. |
| c00-40 | src/applications/analyzerHistory/renderer.ts:14 | `displayStyle: 'flex'` | NA | | CSS only. |
| c00-41 | src/applications/analyzerHistory/renderer.ts:15 | `isDefault: false` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:471 | |

## src/applications/browser/renderer.ts

| # | Electron | Behaviour | Status | Rust parallel | Notes |
|---|---|---|---|---|---|
| c00-42 | src/applications/browser/renderer.ts:11-16 | `BrowserDeps`: ssh API (port forwards), `openExternal`, `sleep`, `random` | PORTED | app-rs/crates/treeterm-core/src/connection/manager.rs:287,343,355 (add/remove/list_port_forwards); app-rs/crates/treeterm-app/src/browser_view.rs:93 (open_externally); web_app.rs:39 (random_port), executor timers | DI plumbing is NA. Every capability exists. |
| c00-43 | src/applications/browser/renderer.ts:18 | One application per `settings.browser.instances[]` entry | PORTED | app-rs/crates/treeterm-model/src/apps.rs:541-553 | |
| c00-44 | src/applications/browser/renderer.ts:20 | id `browser-<instance.id>` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:543 | |
| c00-45 | src/applications/browser/renderer.ts:21-22 | name and icon from the instance | PORTED | app-rs/crates/treeterm-model/src/apps.rs:544-545 | |
| c00-46 | src/applications/browser/renderer.ts:24-27 | Initial state `{ localPort: { status: 'unassigned' }, path: '' }` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:149 | |
| c00-47 | src/applications/browser/renderer.ts:32 | Runtime placeholder `Forwarding { port: 0 }`, replaced synchronously by run() | PORTED | app-rs/crates/treeterm-model/src/browser_page.rs:50,53 (new -> start) | |
| c00-48 | src/applications/browser/renderer.ts:33 | Forward id `webapp-<tabId>` (forwardIdForTab) | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:44 (forward_id_for_tab) | Test web_app.rs:316. |
| c00-49 | src/applications/browser/renderer.ts:34-35 | `disposed` / `isCancelled()` stop late results from landing | PORTED | app-rs/crates/treeterm-model/src/browser_page.rs:32,74,121-122 (`_run` task dropped; `this.update` Err ignored) | |
| c00-50 | src/applications/browser/renderer.ts:39-41 | Local session: page is Ready at once, opened directly | PORTED | app-rs/crates/treeterm-model/src/browser_page.rs:88-91 | Rust's Ready carries `page_url(home, path)` and `home`. Electron's carries home and the pane applies page memory. |
| c00-51 | src/applications/browser/renderer.ts:38,65-67 | Instance URL that is not http(s) on localhost/127.0.0.1: Error phase "URL must be http(s)://localhost or 127.0.0.1: <url>" with a Retry button (also on local sessions) | PARTIAL | app-rs/crates/treeterm-model/src/browser_page.rs:88-91 (local: no validation); app-rs/crates/treeterm-app/src/browser_view.rs:184-186 (Page::Failed) | Remote sessions match (browser_page.rs:93-99). On local sessions Rust goes Ready, and the view then refuses with a different text, "Only localhost pages can be shown: <url>", and no Retry. |
| c00-52 | src/applications/browser/renderer.ts:43-44 | Remote: read the tab's state; tab gone -> give up silently | PORTED | app-rs/crates/treeterm-model/src/browser_page.rs:29,103 (model holds `local_port`; dropped with the tab) | |
| c00-53 | src/applications/browser/renderer.ts:45-46 | Remote: target port of the URL (scheme default when absent); phase Forwarding { port } | PORTED | app-rs/crates/treeterm-model/src/browser_page.rs:93-102 | Shown as "Starting port forward for port N..." (app_view.rs:2292). |
| c00-54 | src/applications/browser/renderer.ts:47-55 | `ensureForward` to localhost:<port> on the ssh host, preferring the tab's persisted local port | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:251-299 (ensure_forward), browser_page.rs:103-107 | Internals (attempts, polling, failure text) are audited with webApp/forward.ts. Rust's failure text is "Could not forward port N after 5 attempts" vs Electron's "Port forward failed after 5 attempts". |
| c00-55 | src/applications/browser/renderer.ts:56 | Cancelled while forwarding: nothing written | PORTED | app-rs/crates/treeterm-model/src/browser_page.rs:74,121-122 | |
| c00-56 | src/applications/browser/renderer.ts:57-60 | Persist `localPort: { status: 'assigned', port }` so the page origin survives restarts | PORTED | app-rs/crates/treeterm-model/src/browser_page.rs:114-115 (LocalPortAssigned) -> workspace_store.rs:1082-1095 (on_web_app_event) | |
| c00-57 | src/applications/browser/renderer.ts:61 | Ready with the URL moved to `localhost:<localPort>` | PORTED | app-rs/crates/treeterm-model/src/browser_page.rs:108,116 (forwarded_url of page and home) | |
| c00-58 | src/applications/browser/renderer.ts:64-69 | run(): any failure -> Error phase with the message (unless cancelled) | PORTED | app-rs/crates/treeterm-model/src/browser_page.rs:111-123; app_view.rs:2293-2296 (page_error) | |
| c00-59 | src/applications/browser/renderer.ts:71 | Start immediately on load | PORTED | app-rs/crates/treeterm-model/src/browser_page.rs:53 | |
| c00-60 | src/applications/browser/renderer.ts:74 | Runtime phase exposed to the pane | PORTED | app-rs/crates/treeterm-model/src/browser_page.rs:57 (phase); workspace_store.rs:143-147 (TabContent::Browser) | |
| c00-61 | src/applications/browser/renderer.ts:75 | Page memory (`pageUrl` / `rememberPage`): reopen on the last same-origin page via tab state `path` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:305 (page_url), :327 (page_path); app-rs/crates/treeterm-app/src/browser_view.rs:293-299 | Detailed in webApp/runtime.ts's chunk. |
| c00-62 | src/applications/browser/renderer.ts:76 | `retry`: rerun start-up (Retry button) | PORTED | app-rs/crates/treeterm-model/src/browser_page.rs:62; app_view.rs:2295 | |
| c00-63 | src/applications/browser/renderer.ts:77-80 | `restartForward`: remove the forward, then rerun start-up (Port Forward sub-tab's Restart) | PORTED | app-rs/crates/treeterm-model/src/browser_page.rs:67-70; app_view.rs:2305-2308 | |
| c00-64 | src/applications/browser/renderer.ts:81-83 | close(): on a remote session remove the tab's port forward | PORTED | app-rs/crates/treeterm-model/src/browser_page.rs:73-84; called from workspace_store.rs:1373 (remove_tab) and :1650 (close_all_tabs) | Rust also cancels an in-flight start. |
| c00-65 | src/applications/browser/renderer.ts:84-86 | dispose(): only mark cancelled (the forward is kept) | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:1119 (dispose_tab drops the page model; forward untouched) | |
| c00-66 | src/applications/browser/renderer.ts:91-93 | Render guard: state without a `localPort` object renders nothing | PARTIAL | app-rs/crates/treeterm-model/src/workspace_store.rs:752-760 (lenient read; missing -> unassigned) | Benign: Rust shows the page. No action needed. |
| c00-67 | src/applications/browser/renderer.ts:94-100 | Render BrowserPane (workspace, tabId, ssh, openExternal) | PORTED | app-rs/crates/treeterm-app/src/app_view.rs:2271 (browser_tab), browser_view.rs (BrowserView) | Pane behaviour is audited elsewhere. |
| c00-68 | src/applications/browser/renderer.ts:103 | `canClose: true` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:549 | |
| c00-69 | src/applications/browser/renderer.ts:104 | `showInNewTabMenu: true` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:550 | |
| c00-70 | src/applications/browser/renderer.ts:105 | `displayStyle: 'flex'` | NA | | CSS only. |
| c00-71 | src/applications/browser/renderer.ts:106 | `isDefault: instance.isDefault` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:551 | |

## src/applications/browser/url.ts

| # | Electron | Behaviour | Status | Rust parallel | Notes |
|---|---|---|---|---|---|
| c00-72 | src/applications/browser/url.ts:1 | Allowed hosts: `localhost`, `127.0.0.1` | PARTIAL | app-rs/crates/treeterm-model/src/apps.rs:203-217 (is_allowed_webview_url) | (v) Was PORTED. Hosts match (test apps.rs:734), but the check is a string check, not WHATWG parsing. The scheme is compared case-sensitively (apps.rs:207), while WHATWG lowercases it, so `HTTP://localhost:3000` is accepted by Electron and refused by Rust, in the instance URL and in the address bar (resolve_address :293). IPv4 shorthands that WHATWG normalises to 127.0.0.1 (`127.1`, `0x7f.1`) are refused as well. |
| c00-73 | src/applications/browser/url.ts:5 | parseLocalUrl: an unparseable URL is refused | PORTED | app-rs/crates/treeterm-model/src/apps.rs:230-251 (PageUrl::parse -> None), :311-316, :292-295 | |
| c00-74 | src/applications/browser/url.ts:7 | Protocol must be `http:` or `https:` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:207,233 | |
| c00-75 | src/applications/browser/url.ts:7 | Hostname must be in LOCAL_HOSTS | PORTED | app-rs/crates/treeterm-model/src/apps.rs:216 | Userinfo tricks are covered by the test at :739. |
| c00-76 | src/applications/browser/url.ts:8 | Error text "URL must be http(s)://localhost or 127.0.0.1: <url>" | PORTED | app-rs/crates/treeterm-model/src/apps.rs:294,315 | Verbatim. |
| c00-77 | src/applications/browser/url.ts:14 | targetPort: explicit port | PORTED | app-rs/crates/treeterm-model/src/apps.rs:254-256 | |
| c00-78 | src/applications/browser/url.ts:16 | targetPort default: 443 for https, else 80 | PORTED | app-rs/crates/treeterm-model/src/apps.rs:248-249,255 | Test apps.rs:630-632. |
| c00-79 | src/applications/browser/url.ts:20-25 | forwardedUrl: same path/query/hash on `localhost:<localPort>` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:319-323 | Test apps.rs:634-635. |
| c00-80 | src/applications/browser/url.ts:27 | `HAS_SCHEME` = `^[a-z][a-z0-9+.-]*://` (case-insensitive) | PORTED | app-rs/crates/treeterm-model/src/apps.rs:286-290 | |
| c00-81 | src/applications/browser/url.ts:35-36 | resolveAddress: trim; empty -> "Enter a URL or path" | PORTED | app-rs/crates/treeterm-model/src/apps.rs:271-274 | Test apps.rs:673. |
| c00-82 | src/applications/browser/url.ts:37 | Page URL parsed (throws if invalid) | PORTED | app-rs/crates/treeterm-model/src/apps.rs:275 | Rust's text is "Invalid page URL: <url>" (Electron: TypeError "Invalid URL"). This is an internal invariant. |
| c00-83 | src/applications/browser/url.ts:38,40 | Input starting with `/`, `?` or `#` resolves against the page (path / query / fragment replaced) | PORTED | app-rs/crates/treeterm-model/src/apps.rs:276-284 | Test apps.rs:667-669. Edge: WHATWG resolution normalises `..` segments and treats `//host/x` as protocol-relative. Rust concatenates, and the webview normalises on load. |
| c00-84 | src/applications/browser/url.ts:39 | Input without a scheme gets `http://` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:291 | Test apps.rs:670. |
| c00-85 | src/applications/browser/url.ts:40 | Absolute input must pass parseLocalUrl | PORTED | app-rs/crates/treeterm-model/src/apps.rs:292-295 | Test apps.rs:674-677. |
| c00-86 | src/applications/browser/url.ts:41-43 | Different scheme or target port from the page -> "Only pages on <origin> can be opened in this tab" | PORTED | app-rs/crates/treeterm-model/src/apps.rs:297-299 | Verbatim. Test apps.rs:678-681. |
| c00-87 | src/applications/browser/url.ts:44-46 | Keep the page's hostname (localhost vs 127.0.0.1) so cookies/storage stay | PORTED | app-rs/crates/treeterm-model/src/apps.rs:301 | Test apps.rs:672. |
| c00-88 | src/applications/browser/url.ts:49 | Markdown MIME types `text/markdown`, `text/x-markdown` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:358 | |
| c00-89 | src/applications/browser/url.ts:50 | Markdown path regex `/\.(md\|markdown)$/i` on the pathname | PORTED | app-rs/crates/treeterm-model/src/apps.rs:361-362 | Lower-cased path, query excluded. Test apps.rs:605. |
| c00-90 | src/applications/browser/url.ts:57 | isMarkdownPage: a markdown MIME type -> markdown | PORTED | app-rs/crates/treeterm-model/src/apps.rs:358-360 | |
| c00-91 | src/applications/browser/url.ts:59 | `text/plain` with a .md/.markdown path -> markdown; HTML never | PORTED | app-rs/crates/treeterm-model/src/apps.rs:362 | Test apps.rs:601-608. |

## src/applications/chat/renderer.ts

| # | Electron | Behaviour | Status | Rust parallel | Notes |
|---|---|---|---|---|---|
| c00-92 | src/applications/chat/renderer.ts:6 | id `chat` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:475 | |
| c00-93 | src/applications/chat/renderer.ts:7-8 | name "Chat", icon "M" | PORTED | app-rs/crates/treeterm-model/src/apps.rs:475 | |
| c00-94 | src/applications/chat/renderer.ts:9-11 | Initial state `{ messages: [] }` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:63 | Test apps.rs:781. |
| c00-95 | src/applications/chat/renderer.ts:12 | onWorkspaceLoad: no daemon resources | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:871-880 (ChatModel; state persisted via ChatEvent::StateChanged) | |
| c00-96 | src/applications/chat/renderer.ts:13 | Render Chat | PORTED | app-rs/crates/treeterm-app/src/tool_views/chat_view.rs:14; app-rs/crates/treeterm-model/src/tool_tabs/chat.rs:46 | |
| c00-97 | src/applications/chat/renderer.ts:14 | `canClose: true` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:469 | |
| c00-98 | src/applications/chat/renderer.ts:15 | `showInNewTabMenu: true` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:475 | Test apps.rs:751. |
| c00-99 | src/applications/chat/renderer.ts:16 | `displayStyle: 'flex'` | NA | | CSS only. |
| c00-100 | src/applications/chat/renderer.ts:17 | `isDefault: false` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:471 | |

## src/applications/comments/renderer.ts

| # | Electron | Behaviour | Status | Rust parallel | Notes |
|---|---|---|---|---|---|
| c00-101 | src/applications/comments/renderer.ts:7 | id `comments` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:474 | |
| c00-102 | src/applications/comments/renderer.ts:8-9 | name "Comments", icon 💬 | PORTED | app-rs/crates/treeterm-model/src/apps.rs:474 | |
| c00-103 | src/applications/comments/renderer.ts:11 | Initial state `{}` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:70 | |
| c00-104 | src/applications/comments/renderer.ts:13 | onWorkspaceLoad: no daemon resources | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:863-866 (CommentsModel) | |
| c00-105 | src/applications/comments/renderer.ts:16-18 | Render guard: null / non-object state renders nothing | PARTIAL | app-rs/crates/treeterm-model/src/workspace_store.rs:863-866 (state not read) | Benign: Rust ignores the state and shows the list. No action needed. |
| c00-106 | src/applications/comments/renderer.ts:19-22 | Render CommentsList for the workspace | PORTED | app-rs/crates/treeterm-app/src/tool_views/comments_view.rs:16; app-rs/crates/treeterm-model/src/tool_tabs/comments.rs:84 | |
| c00-107 | src/applications/comments/renderer.ts:25 | `canClose: true` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:469 | |
| c00-108 | src/applications/comments/renderer.ts:26 | `showInNewTabMenu: true` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:474 | |
| c00-109 | src/applications/comments/renderer.ts:27 | `displayStyle: 'flex'` | NA | | CSS only. |
| c00-110 | src/applications/comments/renderer.ts:28 | `isDefault: false` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:471 | |

## src/applications/customRunner/renderer.ts

| # | Electron | Behaviour | Status | Rust parallel | Notes |
|---|---|---|---|---|---|
| c00-111 | src/applications/customRunner/renderer.ts:6 | `TerminalDeps.terminal.kill` | PORTED | app-rs/crates/treeterm-model/src/terminal.rs:613 (kill) | DI plumbing is NA. The capability is the KillPty RPC. |
| c00-112 | src/applications/customRunner/renderer.ts:8-10 | resolveTemplate: replace every `{{workspace_path}}` with the workspace path | PORTED | app-rs/crates/treeterm-model/src/apps.rs:177-179 (startup_command) | `str::replace` replaces all occurrences. Test apps.rs:725. |
| c00-113 | src/applications/customRunner/renderer.ts:12 | One application per `settings.customRunner.instances[]` entry | PORTED | app-rs/crates/treeterm-model/src/apps.rs:514-526 | Also offered in the header's Run menu (app_view.rs:1130-1137). |
| c00-114 | src/applications/customRunner/renderer.ts:14 | id `customrunner-<instance.id>` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:516 | |
| c00-115 | src/applications/customRunner/renderer.ts:15-16 | name and icon from the instance | PORTED | app-rs/crates/treeterm-model/src/apps.rs:517-518 | |
| c00-116 | src/applications/customRunner/renderer.ts:18-24 | Initial state: `ptyId: null`, fresh `ptyHandle`, `keepOnExit: false`, `idleDetectorDisabled: false`, `widthLimitDisabled: false` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:130-136 | |
| c00-117 | src/applications/customRunner/renderer.ts:32 | Hold the in-flight creation for close() | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:211-214 (TerminalTab::Creating) | See c00-127. |
| c00-118 | src/applications/customRunner/renderer.ts:34 | `ptyId` already set: create nothing (the Terminal view reattaches) | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:829 (attach_terminal) | |
| c00-119 | src/applications/customRunner/renderer.ts:35 | New tab: command = template resolved against the workspace path | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:705 (startup_command) -> apps.rs:177-179 | |
| c00-120 | src/applications/customRunner/renderer.ts:36 | handle = `state.ptyHandle ?? randomUUID()` | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:831-834 | |
| c00-121 | src/applications/customRunner/renderer.ts:37 | `ensureTty(handle, path, no sandbox, resolvedCommand)` | SHARED | app-rs/crates/treeterm-model/src/workspace_store.rs:1005-1031 (create_pty) -> app-rs/crates/treeterm-core/src/daemon.rs:151 | Same CreatePty RPC with handle. |
| c00-122 | src/applications/customRunner/renderer.ts:38-45 | On creation: write `ptyId`, `ptyHandle`, `connectionId` into tab state. There is no disposed guard, so it is written even after dispose while the tab exists | PARTIAL | app-rs/crates/treeterm-model/src/workspace_store.rs:1059-1062 (on_pty_created) | `connectionId` is never written (harmless, see c00-18). After a dispose, Rust drops the task, so the id does not land. The next init re-finds the same PTY by handle, so the end state matches. |
| c00-123 | src/applications/customRunner/renderer.ts:38 | Creation rejected: unhandled | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:1036-1046; app_view.rs:1993 | Rust shows "Failed to start terminal: <error>" (improvement). |
| c00-124 | src/applications/customRunner/renderer.ts:48-56 | `cachedTerminal` / `disposeCachedTerminal` | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:210-220; app_view.rs:1921-1924 | See c00-20. |
| c00-125 | src/applications/customRunner/renderer.ts:57-60 | close(): current `ptyId`/`connectionId` from the store, with fallbacks | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:1352-1368 | |
| c00-126 | src/applications/customRunner/renderer.ts:61-63 | close() with a known PTY: kill it | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:1366-1367 -> terminal.rs:613 | |
| c00-127 | src/applications/customRunner/renderer.ts:65-66 | close() while creating: kill the PTY once creation resolves | PARTIAL | app-rs/crates/treeterm-model/src/workspace_store.rs:1369-1372, :1048-1058 | (v) Confirmed. This is the same dead path as c00-23. remove_tab -> dispose_tab drops the Creating task, cancelling the CreatePty future (rt.rs AbortOnDrop), so on_pty_created's kill never runs and a runner PTY the daemon already started is orphaned. Electron kills it (close() runs before dispose(), and customRunner has no disposed guard). |
| c00-128 | src/applications/customRunner/renderer.ts:68-71 | dispose(): dispose the cached terminal, drop the tab's activity state | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:1119-1124 | |
| c00-129 | src/applications/customRunner/renderer.ts:76-82 | Render the generic Terminal view (no analyzer) | PORTED | app-rs/crates/treeterm-app/src/app_view.rs:1920-1940 | No analyzer: start_analyzer returns None for non-harness apps (workspace_store.rs:979). Activity comes from the idle detector (:940-955). |
| c00-130 | src/applications/customRunner/renderer.ts:84 | `canClose: true` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:522 | |
| c00-131 | src/applications/customRunner/renderer.ts:85 | `showInNewTabMenu: true` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:523 | |
| c00-132 | src/applications/customRunner/renderer.ts:86 | `displayStyle: 'flex'` | NA | | CSS only. |
| c00-133 | src/applications/customRunner/renderer.ts:87 | `isDefault: instance.isDefault` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:524 | |

## src/applications/editor/renderer.ts

| # | Electron | Behaviour | Status | Rust parallel | Notes |
|---|---|---|---|---|---|
| c00-134 | src/applications/editor/renderer.ts:7 | id `editor` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:454 | |
| c00-135 | src/applications/editor/renderer.ts:8-9 | name "Editor", icon ✏ (`✏`) | PORTED | app-rs/crates/treeterm-model/src/apps.rs:455-456 | |
| c00-136 | src/applications/editor/renderer.ts:11-19 | Initial state: status `ready`, empty filePath/originalContent/currentContent, language `plaintext`, `isDirty: false`, viewMode `editor` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:150-153 | Enum strings match (EditorStatus.Ready='ready', EditorViewMode.Editor='editor'). |
| c00-137 | src/applications/editor/renderer.ts:21 | onWorkspaceLoad: no daemon resources | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:804-821 (TextEditor model; title kept as the file name) | |
| c00-138 | src/applications/editor/renderer.ts:23-27 | Render FileEditor (workspace, tabId) | PORTED | app-rs/crates/treeterm-app/src/text_editor_view.rs:26; app-rs/crates/treeterm-model/src/text_editor.rs:60 | |
| c00-139 | src/applications/editor/renderer.ts:29 | `canClose: true` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:458 | |
| c00-140 | src/applications/editor/renderer.ts:30 | `showInNewTabMenu: false` (opened from the Files viewer) | PORTED | app-rs/crates/treeterm-model/src/apps.rs:459; opener workspace_store.rs:1248 (open_in_editor) | |
| c00-141 | src/applications/editor/renderer.ts:31 | `displayStyle: 'flex'` | NA | | CSS only. |
| c00-142 | src/applications/editor/renderer.ts:32 | `isDefault: false` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:460 | |

## src/applications/filesystem/renderer.ts

| # | Electron | Behaviour | Status | Rust parallel | Notes |
|---|---|---|---|---|---|
| c00-143 | src/applications/filesystem/renderer.ts:6 | id `filesystem` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:436 | |
| c00-144 | src/applications/filesystem/renderer.ts:7-8 | name "Files", icon 📂 | PORTED | app-rs/crates/treeterm-model/src/apps.rs:437-438 | |
| c00-145 | src/applications/filesystem/renderer.ts:10-13 | Initial state `{ selectedPath: null, expandedDirs: [] }` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:147 | |
| c00-146 | src/applications/filesystem/renderer.ts:15 | onWorkspaceLoad: no daemon resources | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:742-751 (FileBrowser model; state merged back) | |
| c00-147 | src/applications/filesystem/renderer.ts:17-23 | Render FilesystemBrowser (workspace, tabId) | PORTED | app-rs/crates/treeterm-app/src/file_browser_view.rs:221; app-rs/crates/treeterm-model/src/file_browser.rs:260 | |
| c00-148 | src/applications/filesystem/renderer.ts:25 | `canClose: true` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:440 | |
| c00-149 | src/applications/filesystem/renderer.ts:26 | `showInNewTabMenu: true` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:441 | |
| c00-150 | src/applications/filesystem/renderer.ts:27 | `displayStyle: 'flex'` | NA | | CSS only. |
| c00-151 | src/applications/filesystem/renderer.ts:28 | `isDefault: true` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:442 | No runtime consumer in either app. |

## src/applications/ghosttyTerminal/renderer.ts

| # | Electron | Behaviour | Status | Rust parallel | Notes |
|---|---|---|---|---|---|
| c00-152 | src/applications/ghosttyTerminal/renderer.ts:13 | A second built-in terminal application (always registered) | PORTED | app-rs/crates/treeterm-model/src/apps.rs:433-434 (`terminal("ghostty-terminal", false)`) | Registered so existing ghostty tabs open. Natively it is the same terminal. |
| c00-153 | src/applications/ghosttyTerminal/renderer.ts:15 | id `ghostty-terminal` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:434 | |
| c00-154 | src/applications/ghosttyTerminal/renderer.ts:16 | name "Terminal (Ghostty)" | PARTIAL | app-rs/crates/treeterm-model/src/apps.rs:422,434 | Rust names it "Terminal" (affects the default title "<name> <n>" from add_tab, workspace_store.rs:1147). Intentional per the apps.rs:433 comment, but not listed in README. (v) The name also shows in Settings -> General -> "Default Application for New Worktrees", which lists every registered app by name (app-rs/crates/treeterm-app/src/settings_view/form.rs:416-421, as Electron's SettingsDialog.tsx:61,184-187). Rust shows two entries both called "Terminal", one of which saves `ghostty-terminal`. Electron shows "Terminal" and "Terminal (Ghostty)". |
| c00-155 | src/applications/ghosttyTerminal/renderer.ts:17 | icon 👻 | PARTIAL | app-rs/crates/treeterm-model/src/apps.rs:423; tab strip icon app_view.rs:1806 | Rust uses ">", so existing ghostty tabs show ">" instead of 👻 in the tab strip. |
| c00-156 | src/applications/ghosttyTerminal/renderer.ts:19-25 | Initial terminal state (ptyId null, fresh ptyHandle, flags false) | PORTED | app-rs/crates/treeterm-model/src/apps.rs:130-136 | |
| c00-157 | src/applications/ghosttyTerminal/renderer.ts:27 | PTY lifecycle shared with the xterm terminal (`makeTerminalOnWorkspaceLoad`) | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:827-840 (AppKind::Terminal arm) | Lifecycle details are audited with terminal/renderer.ts. |
| c00-158 | src/applications/ghosttyTerminal/renderer.ts:29-33 | Rendered by ghostty-web instead of xterm.js | PORTED | app-rs/crates/treeterm-app/src/app_view.rs:1920-1940 (same alacritty TerminalView) | Library difference only. |
| c00-159 | src/applications/ghosttyTerminal/renderer.ts:35 | `canClose: true` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:427 | |
| c00-160 | src/applications/ghosttyTerminal/renderer.ts:37 | `showInNewTabMenu: true` (new ghostty tabs can be opened) | PARTIAL | app-rs/crates/treeterm-model/src/apps.rs:428,434 (show = false) | Hidden from the "+" menu; test apps.rs:745 (the_ghostty_alias_is_hidden_from_the_menu). Intentional per code, not in README. (v) It is also missing from the lists built from the new-tab menu: the Settings tab's "Default Application for Children" (app-rs/crates/treeterm-model/src/tool_tabs/workspace_settings.rs:17-20; Electron WorkspaceSettings.tsx:17) and the child dialog's custom application list (child_dialog.rs:735-736). The native alias is the same terminal, so only the choice of the Ghostty frontend for Electron is lost. |
| c00-161 | src/applications/ghosttyTerminal/renderer.ts:38 | `displayStyle: 'flex'` | NA | | CSS only. |
| c00-162 | src/applications/ghosttyTerminal/renderer.ts:39 | `isDefault: false` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:429 | |

## src/applications/github/renderer.ts

| # | Electron | Behaviour | Status | Rust parallel | Notes |
|---|---|---|---|---|---|
| c00-163 | src/applications/github/renderer.ts:6 | id `github` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:479 | |
| c00-164 | src/applications/github/renderer.ts:7-8 | name "GitHub", icon 🐙 | PORTED | app-rs/crates/treeterm-model/src/apps.rs:479 | |
| c00-165 | src/applications/github/renderer.ts:10 | Initial state `{}` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:70 | Test apps.rs:782. |
| c00-166 | src/applications/github/renderer.ts:12 | onWorkspaceLoad: no daemon resources | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:855 (ToolTab::GitHub) | |
| c00-167 | src/applications/github/renderer.ts:14-20 | Render GitHubBrowser (workspace, isVisible): renders only while visible | PORTED | app-rs/crates/treeterm-app/src/tool_views/github_view.rs:14; app_view.rs:1977-1988 (only the active tab is rendered) | (v) Note corrected: GitHubBrowser returns null while hidden (GitHubBrowser.tsx:24) and does not poll. In both apps the PR data comes from the session's git controller (tool_views/mod.rs:92). |
| c00-168 | src/applications/github/renderer.ts:22 | `canClose: true` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:469 | |
| c00-169 | src/applications/github/renderer.ts:23 | `showInNewTabMenu: true` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:479 | |
| c00-170 | src/applications/github/renderer.ts:24 | `displayStyle: 'flex'` | NA | | CSS only. |
| c00-171 | src/applications/github/renderer.ts:25 | `isDefault: false` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:471 | |

## src/applications/index.ts

| # | Electron | Behaviour | Status | Rust parallel | Notes |
|---|---|---|---|---|---|
| c00-172 | src/applications/index.ts:1-12 | Barrel re-exports of the application factories | NA | app-rs/crates/treeterm-model/src/apps.rs:419 (AppRegistry::from_settings builds them all) | Module plumbing. Registration order is checked with store/app.ts. |

## src/applications/review/renderer.ts

| # | Electron | Behaviour | Status | Rust parallel | Notes |
|---|---|---|---|---|---|
| c00-173 | src/applications/review/renderer.ts:7 | id `review` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:445 | |
| c00-174 | src/applications/review/renderer.ts:8-9 | name "Review", icon 📋 | PORTED | app-rs/crates/treeterm-model/src/apps.rs:446-447 | |
| c00-175 | src/applications/review/renderer.ts:13 | Initial `viewMode: 'committed'` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:148 | |
| c00-176 | src/applications/review/renderer.ts:12 | `parentWorkspaceId` optional: absent = top-level worktree; set for a child | PARTIAL | app-rs/crates/treeterm-model/src/workspace_store.rs:1160-1169 (initial_tab_state) | (v) Was PORTED. Rust adds the field only when the workspace is a worktree (`file.is_worktree`, :1163). Electron's "+" menu and Review button add `parentId ?? undefined` for any workspace with a parent (WorkspacePane.tsx:216,512). A non-worktree workspace dragged under another gets the field in Electron but not in Rust. Rust's own Review ignores the field (c00-180), so only Electron reading a Rust-made tab sees the difference: it treats the tab as top-level. |
| c00-177 | src/applications/review/renderer.ts:16 | onWorkspaceLoad: no daemon resources | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:769-803 (ReviewModel and its events) | |
| c00-178 | src/applications/review/renderer.ts:19-21 | Render guard: non-object state or non-string `parentWorkspaceId` renders nothing | PARTIAL | app-rs/crates/treeterm-model/src/workspace_store.rs:769-775 (no shape check) | Benign: Rust shows the review. No action needed. |
| c00-179 | src/applications/review/renderer.ts:22-28 | Render ReviewBrowser (workspace, tabId, isVisible). It reloads whenever the tab goes from hidden to shown | PARTIAL | app-rs/crates/treeterm-app/src/review_view.rs:40; app-rs/crates/treeterm-app/src/app_view.rs:1853-1856 (refresh on tab-strip click); app-rs/crates/treeterm-model/src/workspace_store.rs:1506-1517 (refresh after a keep-worktree merge) | (v) Was PORTED. Electron runs handleRefresh, which also refreshes git, on every hidden->shown edge (ReviewBrowser.tsx:266-273). That includes re-selecting the workspace in the tree and focusing the tab via the header's Review button or the tree's "Review & Merge" (openOrFocusTab). Rust refreshes only when the tab is clicked in the tab strip, even if it is already active, which Electron does not do. Focusing it through open_review (app_view.rs:1091-1096), the tree menu (tree_pane.rs:600) or by switching workspaces shows the old diff until Refresh is pressed. |
| c00-180 | src/applications/review/renderer.ts:26 | The parent to compare/merge against comes from the tab state's `parentWorkspaceId` | PARTIAL | app-rs/crates/treeterm-model/src/workspace_store.rs:770-774 (ReviewBase from `baseBranchOverride`, else the session-kept `parent_branch`); app-rs/crates/treeterm-model/src/session_store.rs:946-959 (refresh_parent_branches) | Rust never reads `parentWorkspaceId` and uses the workspace's current parent. The two diverge when the state's id differs from the actual parent (e.g. after reparenting, or an old tab without the field, which Electron treats as top-level). (v) Confirmed: Electron uses the prop for the base branch, the conflict check and `hasParent` (ReviewBrowser.tsx:103-114,335). Rust follows the actual current parent, which is arguably the better behaviour. |
| c00-181 | src/applications/review/renderer.ts:31 | `canClose: true` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:449 | |
| c00-182 | src/applications/review/renderer.ts:32 | `showInNewTabMenu: true` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:450 | |
| c00-183 | src/applications/review/renderer.ts:33 | `displayStyle: 'flex'` | NA | | CSS only. |
| c00-184 | src/applications/review/renderer.ts:34 | `isDefault: false` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:451 | |

## src/applications/sshUpload/renderer.tsx

| # | Electron | Behaviour | Status | Rust parallel | Notes |
|---|---|---|---|---|---|
| c00-185 | src/applications/sshUpload/renderer.tsx:7 | `SshUploadState`: empty marker state | PORTED | app-rs/crates/treeterm-model/src/apps.rs:56,70 (ToolApp::SshUpload, `{}`) | |
| c00-186 | src/applications/sshUpload/renderer.tsx:9-12 | SshUploadTab: the pane is bound to the workspace's connection | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:891-894 (SshUpload::new(self.remote)); app-rs/crates/treeterm-model/src/tool_tabs/ssh_upload.rs:65 | The reactive store subscription is React mechanics. |
| c00-187 | src/applications/sshUpload/renderer.tsx:15 | id `ssh-upload` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:481 | |
| c00-188 | src/applications/sshUpload/renderer.tsx:16-17 | name "Upload", icon ⬆ | PORTED | app-rs/crates/treeterm-model/src/apps.rs:481 | `\u{2B06}`. |
| c00-189 | src/applications/sshUpload/renderer.tsx:18 | Initial state `{}` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:70 | |
| c00-190 | src/applications/sshUpload/renderer.tsx:19 | onWorkspaceLoad: no daemon resources | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:891-894 | |
| c00-191 | src/applications/sshUpload/renderer.tsx:20 | Render SshUploadPane | PORTED | app-rs/crates/treeterm-app/src/tool_views/ssh_upload_view.rs:14 | |
| c00-192 | src/applications/sshUpload/renderer.tsx:21 | `canClose: true` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:469 | |
| c00-193 | src/applications/sshUpload/renderer.tsx:22 | `showInNewTabMenu: true` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:481 | |
| c00-194 | src/applications/sshUpload/renderer.tsx:23 | `displayStyle: 'flex'` | NA | | CSS only. |
| c00-195 | src/applications/sshUpload/renderer.tsx:24 | `isDefault: false` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:471 | |
| c00-196 | src/applications/sshUpload/renderer.tsx:25 | `availability: RemoteOnly`: offered in the new-tab menu only on ssh sessions | PORTED | app-rs/crates/treeterm-model/src/apps.rs:89-91 (available_on), :591 (new_tab_menu_for); app_view.rs:1866 | Test apps.rs:758. Electron uses availability only in the new-tab menus (FlexLayoutPane.tsx:47, WorkspacePane.tsx:63). |

## Gaps

- c00-18, PARTIAL (low), src/applications/aiHarness/renderer.ts:60-65. After creating an AI-harness PTY, Rust writes `ptyId`/`ptyHandle` but not `connectionId` into the tab state. Harmless, because Electron's close() falls back to `ws.connectionId`. Nearest: app-rs/crates/treeterm-model/src/workspace_store.rs:1059-1062 (on_pty_created).
- c00-23, PARTIAL (medium), src/applications/aiHarness/renderer.ts:90-91. Closing an AI-harness tab while its PTY is being created does not kill the PTY. dispose_tab drops the `Creating` task, which cancels the CreatePty future, so the "closed while starting" kill in on_pty_created is unreachable and the harness process can be orphaned. Nearest: app-rs/crates/treeterm-model/src/workspace_store.rs:1369-1372 (remove_tab), :1048-1058 (on_pty_created), :1119 (dispose_tab).
- c00-25, PARTIAL (low), src/applications/aiHarness/renderer.ts:104-106. There is no AiHarnessState shape guard: Rust renders the terminal where Electron renders nothing (benign). Nearest: app-rs/crates/treeterm-model/src/workspace_store.rs:827-840.
- c00-27, PARTIAL (low), src/applications/aiHarness/renderer.ts:115. The AI-harness `backgroundColor` is used only when it is `#rrggbb`. Other CSS colours (`#000`, `black`, `rgb()`) that Electron honours fall back to the sandbox colour or the default. Nearest: app-rs/crates/treeterm-model/src/workspace_store.rs:260-268 (parse_hex_color), :907-918.
- c00-28, MISSING (medium), src/applications/aiHarness/renderer.ts:116. The AI-harness `disableScrollbar` option is parsed and editable in Settings but never applied: harness terminals always show the scrollbar. Nearest: app-rs/crates/treeterm-core/src/settings/read.rs:362, app-rs/crates/treeterm-app/src/settings_view/instance.rs:276, app-rs/crates/treeterm-app/src/terminal_element.rs:320.
- c00-51, PARTIAL (medium), src/applications/browser/renderer.ts:38,65-67. When a Browser instance URL on a local session is not localhost, Rust shows "Only localhost pages can be shown: <url>" with no Retry, instead of Electron's Error phase "URL must be http(s)://localhost or 127.0.0.1: <url>" with Retry. Nearest: app-rs/crates/treeterm-model/src/browser_page.rs:88-91, app-rs/crates/treeterm-app/src/browser_view.rs:184-186.
- c00-66, PARTIAL (low), src/applications/browser/renderer.ts:91-93. There is no BrowserState shape guard: Rust renders the page (benign). Nearest: app-rs/crates/treeterm-model/src/workspace_store.rs:752-760.
- c00-72, PARTIAL (low), src/applications/browser/url.ts:1. The localhost check is case-sensitive on the scheme and does not normalise IPv4 shorthands, so `HTTP://localhost` and `http://127.1` are refused where Electron accepts them. Nearest: app-rs/crates/treeterm-model/src/apps.rs:203-217.
- c00-105, PARTIAL (low), src/applications/comments/renderer.ts:16-18. There is no CommentsState guard: Rust renders the list (benign). Nearest: app-rs/crates/treeterm-model/src/workspace_store.rs:863-866.
- c00-122, PARTIAL (low), src/applications/customRunner/renderer.ts:38-45. On custom-runner PTY creation, `connectionId` is not written to the tab state, and nothing is written after dispose. The handle re-finds the PTY, so the end state matches. Nearest: app-rs/crates/treeterm-model/src/workspace_store.rs:1059-1062.
- c00-127, PARTIAL (medium), src/applications/customRunner/renderer.ts:65-66. Closing a custom-runner tab mid-creation does not kill the PTY once it exists (same dead path as c00-23). Nearest: app-rs/crates/treeterm-model/src/workspace_store.rs:1369-1372, :1048-1058.
- c00-154, PARTIAL (medium), src/applications/ghosttyTerminal/renderer.ts:16. The ghostty alias is named "Terminal" instead of "Terminal (Ghostty)", so Settings' "Default Application for New Worktrees" lists two entries both called "Terminal". Nearest: app-rs/crates/treeterm-model/src/apps.rs:422,434; app-rs/crates/treeterm-app/src/settings_view/form.rs:416-421.
- c00-155, PARTIAL (low), src/applications/ghosttyTerminal/renderer.ts:17. Ghostty tabs show the ">" icon instead of 👻 in the tab strip. Nearest: app-rs/crates/treeterm-model/src/apps.rs:423; app-rs/crates/treeterm-app/src/app_view.rs:1806.
- c00-160, PARTIAL (low), src/applications/ghosttyTerminal/renderer.ts:37. "Terminal (Ghostty)" is hidden from the new-tab menu, the workspace "Default Application for Children" list and the child dialog's application list. This is intentional per code and test, but not listed in README. Nearest: app-rs/crates/treeterm-model/src/apps.rs:428,434 (test :745).
- c00-176, PARTIAL (low), src/applications/review/renderer.ts:12. A new Review tab gets `parentWorkspaceId` only on worktree workspaces. Electron adds it for any workspace with a parent. Nearest: app-rs/crates/treeterm-model/src/workspace_store.rs:1160-1169.
- c00-178, PARTIAL (low), src/applications/review/renderer.ts:19-21. There is no ReviewState shape guard: Rust renders the review (benign). Nearest: app-rs/crates/treeterm-model/src/workspace_store.rs:769-775.
- c00-179, PARTIAL (medium), src/applications/review/renderer.ts:22-28. Review reloads only when its tab is clicked in the tab strip. It does not reload when shown by switching back to the workspace, by the header's Review button or by the tree's "Review & Merge", as Electron's hidden->shown refresh does. Nearest: app-rs/crates/treeterm-app/src/app_view.rs:1853-1856, :1091-1096; app-rs/crates/treeterm-app/src/tree_pane.rs:600.
- c00-180, PARTIAL (low), src/applications/review/renderer.ts:26. The Review parent comes from the session's current parent branch instead of the tab state's `parentWorkspaceId`, so the two diverge after reparenting or for tabs without the field. Rust's choice is arguably better. Nearest: app-rs/crates/treeterm-model/src/workspace_store.rs:770-774.
