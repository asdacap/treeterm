# c01-applications_terminal_renderer

Verified: +3 rows, 11 rows changed

Scope: `src/applications/terminal/renderer.ts`, `terminalAnalyzerDebugger/renderer.ts`, `ttyList/renderer.ts`, `webApp/{forward,ports,renderer,runtime}.ts`, `workspaceSettings/renderer.ts`.

Main Rust counterparts: `treeterm-model/src/apps.rs` (registry, initial states), `treeterm-model/src/workspace_store.rs` (`init_tab`, `create_pty`, `on_pty_created`, `remove_tab`, `dispose_tab`, `on_web_app_event`), `treeterm-model/src/web_app.rs` (Web App start-up, probes, forward), `treeterm-model/src/browser_page.rs`, `treeterm-app/src/app_view.rs` (`web_app_tab`, `page_view`), `treeterm-app/src/browser_view.rs` (page memory), `treeterm-app/src/tool_views/*`.

## src/applications/terminal/renderer.ts

| # | Electron | Behaviour | Status | Rust parallel | Notes |
|---|---|---|---|---|---|
| c01-1 | src/applications/terminal/renderer.ts:6 | `TerminalDeps` injects `terminal.kill(connectionId, sessionId)` | NA | app-rs/crates/treeterm-model/src/terminal.rs:613 (TerminalModel::kill) | DI plumbing; the kill-PTY capability exists (KillPty RPC via `conn.kill_pty`). |
| c01-2 | src/applications/terminal/renderer.ts:16 | `makeTerminalOnWorkspaceLoad`: shared PTY lifecycle for every terminal-backed app (base terminal, variants, ghostty frontend) | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:827 (init_tab, Terminal/AiHarness/CustomRunner arm) | Ghostty frontend is registered as the same terminal: apps.rs:434 (`ghostty-terminal`). |
| c01-3 | src/applications/terminal/renderer.ts:21 | On load, reads the workspace store and the tab's persisted `TerminalState` | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:695 (init_tab) | Reads `app_states[tab_id].state`. |
| c01-4 | src/applications/terminal/renderer.ts:26 | Holds the in-flight creation promise (`creating`) so close() can act on it | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:211 (TerminalTab::Creating) | Rust keeps the creation task in `TerminalTab::Creating { _task }` (see c01-18 for what happens on close). |
| c01-5 | src/applications/terminal/renderer.ts:28 | `if (!state.ptyId)`: no PTY yet, start creating one | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:830 (init_tab → create_pty) | |
| c01-6 | src/applications/terminal/renderer.ts:28 | ptyId present: no creation; the terminal attaches to the existing PTY | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:829 (attach_terminal) | Edge: Rust attaches to any JSON string (even `""`), Electron re-creates for falsy `""`; neither app writes `""`. |
| c01-7 | src/applications/terminal/renderer.ts:31 | `state.ptyHandle ?? crypto.randomUUID()` fallback for tabs without a handle | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:831 (init_tab) | `uuid::Uuid::new_v4()` when `ptyHandle` is not a string. |
| c01-8 | src/applications/terminal/renderer.ts:32 | `ensureTty(handle, workspace.path, no sandbox, startupCommand)`: create PTY in the workspace dir | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:1005 (create_pty) | `CreatePty { cwd: workspace.path, handle, startup_command }`, sandbox none. |
| c01-9 | src/applications/terminal/renderer.ts:29 | Creation keyed by the stable `ptyHandle` so reconcile dispose/re-init churn returns the same PTY | SHARED | daemon-rs/crates/treeterm-daemon/src/pty_manager.rs:147 (handle reuse); app-rs/crates/treeterm-core/src/daemon.rs:151 (create_pty sends `handle`) | Same CreatePty RPC with `handle`; daemon returns the live session for a known handle. |
| c01-10 | src/applications/terminal/renderer.ts:33 | On creation resolve: write `ptyId` and `ptyHandle` into the tab state (persisted) | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:1060 (on_pty_created) | Also attaches the terminal and emits Changed (written to disk). |
| c01-11 | src/applications/terminal/renderer.ts:38 | On creation resolve: write `connectionId: ws.connectionId` into the tab state (used to route kill) | PARTIAL | app-rs/crates/treeterm-model/src/workspace_store.rs:1060 (on_pty_created) | Rust never writes `connectionId` (no `"connectionId"` anywhere in app-rs). Kill routing is unaffected (Rust kills through the workspace's own connection; Electron falls back to `ws.connectionId`), but tab state written by the port lacks the field. |
| c01-12 | src/applications/terminal/renderer.ts:33 | Creation rejection is not handled here (`void creating.then`): the tab stays on "Creating terminal..." | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:1036 (on_pty_created Err arm) | Rust goes further: the tab shows "Failed to start terminal: {error}" (TerminalTab::Failed). |
| c01-13 | src/applications/terminal/renderer.ts:45 | `disposeCachedTerminal()`: drop the cached terminal engine + Tty subscription kept across remounts (workspace switch) | PORTED | app-rs/crates/treeterm-app/src/app_view.rs:2509 (prune_terminal_views); app-rs/crates/treeterm-model/src/workspace_store.rs:1119 (dispose_tab) | Rust keeps one TerminalView per TerminalModel across workspace switches and drops it when the model is gone. |
| c01-14 | src/applications/terminal/renderer.ts:46 | Guard `if (this.cachedTerminal)`, null `mountedHandler`, `owner.dispose()` | NA | — | xterm engine cache mechanics, no user-visible effect of its own. |
| c01-15 | src/applications/terminal/renderer.ts:54 | close(): connection = current state's `connectionId ?? ws.connectionId` | PORTED | app-rs/crates/treeterm-model/src/terminal.rs:613 (TerminalModel::kill via `self.conn`) | Rust always kills on the workspace's connection (equivalent to the fallback). |
| c01-16 | src/applications/terminal/renderer.ts:56 | close(): `ptyId = current?.ptyId ?? state.ptyId` (falls back to the load-time id) | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:1366 (remove_tab, TerminalTab::Attached) | The attached TerminalModel holds the pty id. |
| c01-17 | src/applications/terminal/renderer.ts:57 | close(): ptyId known → `terminal.kill(connectionId, ptyId)` and return | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:1366 (remove_tab → TerminalModel::kill) | Rust also shows "Failed to kill terminal: …" overlay on failure (terminal.rs:624). |
| c01-18 | src/applications/terminal/renderer.ts:61 | close() while creation in flight: chain a kill onto the creation promise so the PTY is killed as soon as its id is known | PARTIAL | app-rs/crates/treeterm-model/src/workspace_store.rs:1369 (remove_tab Creating arm), :1048 (on_pty_created "tab gone → kill") | (v) Re-verified: `remove_tab` calls `dispose_tab` (:1383) which drops `TerminalTab::Creating { _task }`; dropping the GPUI Task drops its `create` `rt::Fut`, whose `AbortOnDrop` aborts the tokio RPC task (app-rs/crates/treeterm-core/src/rt.rs:74), so `on_pty_created` (and its kill) never runs. If the daemon already spawned the PTY it is orphaned (shows as an orphan in the TTY list). Same on workspace removal: `close_all_tabs` (:1650) skips Creating tabs, then the store is dropped. Window is narrow (creation normally resolves in ms; wider on a slow remote daemon). No test covers it. |
| c01-19 | src/applications/terminal/renderer.ts:66 | dispose(): dispose the cached terminal (PTY keeps running) | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:1119 (dispose_tab) | Detaches, never kills. |
| c01-20 | src/applications/terminal/renderer.ts:68 | dispose(): remove the tab's activity state | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:1122 (dispose_tab → activity.remove_tab_state) | |
| c01-21 | src/applications/terminal/renderer.ts:78 | Base terminal app: id `terminal`, name `Terminal`, icon `>` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:420 (AppRegistry::from_settings terminal closure) | |
| c01-22 | src/applications/terminal/renderer.ts:82 | createInitialState: `ptyId:null, ptyHandle, keepOnExit:false, idleDetectorDisabled:false, widthLimitDisabled:false` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:130 (AppDef::initial_state) | Same keys and defaults. |
| c01-23 | src/applications/terminal/renderer.ts:84 | A fresh `ptyHandle` minted per new tab | PORTED | app-rs/crates/treeterm-model/src/apps.rs:128 (initial_state) | Test apps.rs:692 asserts distinct handles. |
| c01-24 | src/applications/terminal/renderer.ts:90 | Base terminal runs the default shell (no startup command) | PORTED | app-rs/crates/treeterm-model/src/apps.rs:424 (`startup_command: None`) | |
| c01-25 | src/applications/terminal/renderer.ts:92 | render: `Terminal` keyed by tab with workspace/tabId/isVisible (cwd prop unused by Terminal.tsx) | PORTED | app-rs/crates/treeterm-app/src/app_view.rs:1920 (TerminalView + terminal_status_bar) | |
| c01-26 | src/applications/terminal/renderer.ts:100 | `canClose: true` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:427 | |
| c01-27 | src/applications/terminal/renderer.ts:102 | `showInNewTabMenu: true` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:432 (`terminal("terminal", true)`) | Test apps.rs:745. |
| c01-28 | src/applications/terminal/renderer.ts:103 | `displayStyle: 'flex'` | NA | — | CSS layout of the tab container. |
| c01-29 | src/applications/terminal/renderer.ts:104 | `isDefault: true` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:429 (`is_default: show`) | (v) Flag is inert in both apps: Electron's only reader `getDefaultApplications` (src/renderer/store/app.ts:182) has no callers; Rust never reads `is_default` outside a test (new workspaces use `default_for_workspace`, apps.rs:569). The Rust doc comment on `AppDef::is_default` ("Opened automatically in new workspaces") overstates it. |
| c01-30 | src/applications/terminal/renderer.ts:111 | Variant: id `terminal-<instance.id>`, name/icon from the instance | PORTED | app-rs/crates/treeterm-model/src/apps.rs:483 (settings.terminal.instances loop) | |
| c01-31 | src/applications/terminal/renderer.ts:115 | Variant createInitialState: same five fields | PORTED | app-rs/crates/treeterm-model/src/apps.rs:130 | Same `AppKind::Terminal` arm. |
| c01-32 | src/applications/terminal/renderer.ts:123 | Variant PTY runs `instance.startupCommand` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:489 (empty → None), :175 (startup_command) | Electron passes `""`; daemon ignores blank commands (pty_manager.rs:269), so identical. |
| c01-33 | src/applications/terminal/renderer.ts:125 | Variant render: same Terminal (its `startupCommand` prop is unused by Terminal.tsx) | PORTED | app-rs/crates/treeterm-app/src/app_view.rs:1920 | |
| c01-34 | src/applications/terminal/renderer.ts:134 | Variant `canClose: true` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:492 | |
| c01-35 | src/applications/terminal/renderer.ts:136 | Variant `showInNewTabMenu: true` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:493 | |
| c01-36 | src/applications/terminal/renderer.ts:137 | Variant `displayStyle: 'flex'` | NA | — | CSS layout. |
| c01-37 | src/applications/terminal/renderer.ts:138 | Variant `isDefault: instance.isDefault` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:494 | (v) Copied from the instance; inert in both apps (see c01-29). |
| c01-170 | src/applications/terminal/renderer.ts:64 | close() after PTY creation failed: the chained kill never runs (promise rejected), nothing is killed | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:1375 (remove_tab, `TerminalTab::Failed` arm does nothing) | (v) Added. Electron's tab sat on "Creating terminal..." (c01-12), Rust's shows "Failed to start terminal: …"; closing kills nothing in either. |

## src/applications/terminalAnalyzerDebugger/renderer.ts

| # | Electron | Behaviour | Status | Rust parallel | Notes |
|---|---|---|---|---|---|
| c01-38 | src/applications/terminalAnalyzerDebugger/renderer.ts:6 | id `system-prompt-debugger`, name `System Prompt Debugger`, icon 🔬 | PORTED | app-rs/crates/treeterm-model/src/apps.rs:476 (ToolApp::PromptDebugger) | Icon `\u{1F52C}`. |
| c01-39 | src/applications/terminalAnalyzerDebugger/renderer.ts:9 | createInitialState `{}` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:61 (ToolApp::initial_state) | |
| c01-40 | src/applications/terminalAnalyzerDebugger/renderer.ts:10 | onWorkspaceLoad: no daemon resources (no-op close/dispose) | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:881 (init_tool_tab PromptDebugger); :1352 (remove_tab Tool arm does nothing) | |
| c01-41 | src/applications/terminalAnalyzerDebugger/renderer.ts:11 | render `SystemPromptDebugger` | PORTED | app-rs/crates/treeterm-app/src/tool_views/mod.rs:133 (PromptDebuggerView) | View behaviour audited with the component chunk. |
| c01-42 | src/applications/terminalAnalyzerDebugger/renderer.ts:12 | `canClose: true` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:469 (tool closure) | |
| c01-43 | src/applications/terminalAnalyzerDebugger/renderer.ts:13 | `showInNewTabMenu: true` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:476 (`true`) | |
| c01-44 | src/applications/terminalAnalyzerDebugger/renderer.ts:14 | `displayStyle: 'flex'` | NA | — | CSS layout. |
| c01-45 | src/applications/terminalAnalyzerDebugger/renderer.ts:15 | `isDefault: false` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:471 | Test apps.rs:767. |

## src/applications/ttyList/renderer.ts

| # | Electron | Behaviour | Status | Rust parallel | Notes |
|---|---|---|---|---|---|
| c01-46 | src/applications/ttyList/renderer.ts:6 | `TtyListState` (empty marker type) | PORTED | app-rs/crates/treeterm-model/src/apps.rs:48 (ToolApp::TtyList) | |
| c01-47 | src/applications/ttyList/renderer.ts:9 | id `tty-list`, name `TTYs`, icon ☰ | PORTED | app-rs/crates/treeterm-model/src/apps.rs:480 | `\u{2630}`. |
| c01-48 | src/applications/ttyList/renderer.ts:12 | createInitialState `{}` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:61 | |
| c01-49 | src/applications/ttyList/renderer.ts:13 | onWorkspaceLoad: no-op close/dispose | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:867 (init_tool_tab TtyList) | |
| c01-50 | src/applications/ttyList/renderer.ts:14 | render `TtyListBrowser` | PORTED | app-rs/crates/treeterm-app/src/tool_views/mod.rs:128 (TtyListView) | |
| c01-51 | src/applications/ttyList/renderer.ts:15 | `canClose: true` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:469 | |
| c01-52 | src/applications/ttyList/renderer.ts:16 | `showInNewTabMenu: true` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:480 | |
| c01-53 | src/applications/ttyList/renderer.ts:17 | `displayStyle: 'flex'` | NA | — | CSS layout. |
| c01-54 | src/applications/ttyList/renderer.ts:18 | `isDefault: false` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:471 | |

## src/applications/webApp/forward.ts

| # | Electron | Behaviour | Status | Rust parallel | Notes |
|---|---|---|---|---|---|
| c01-55 | src/applications/webApp/forward.ts:6 | `FORWARD_ATTEMPTS = 5` | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:24 | |
| c01-56 | src/applications/webApp/forward.ts:7 | `FORWARD_POLL_MS = 500` | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:25 (FORWARD_POLL) | |
| c01-57 | src/applications/webApp/forward.ts:9 | `EnsureForwardOptions` (ssh/sleep/random DI + ids/ports) | NA | app-rs/crates/treeterm-model/src/web_app.rs:251 (ensure_forward params) | Test-injection plumbing. |
| c01-58 | src/applications/webApp/forward.ts:17 | `preferredLocalPort` tried first so the page origin (cookies, localStorage) survives restarts | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:255 (`preferred`); workspace_store.rs:715 (read from `localPort`) | |
| c01-59 | src/applications/webApp/forward.ts:21 | `settledForward`: poll until the forward leaves Connecting | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:225 (settled_forward) | |
| c01-60 | src/applications/webApp/forward.ts:23 | Lists the connection's forwards and finds the tab's by id | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:232 | `manager().list_port_forwards(connection)`. |
| c01-61 | src/applications/webApp/forward.ts:24 | Returns as soon as status is not Connecting (or the forward does not exist → undefined) | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:242 | Rust adds "the session went away" if the AppStore is gone. |
| c01-62 | src/applications/webApp/forward.ts:25 | Sleeps 500 ms between polls | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:243 | |
| c01-63 | src/applications/webApp/forward.ts:35 | Existing Active forward → reuse its local port | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:267 | |
| c01-64 | src/applications/webApp/forward.ts:36 | Existing non-active forward → remove it first | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:268 | |
| c01-65 | src/applications/webApp/forward.ts:38 | Up to 5 add attempts | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:271 | |
| c01-66 | src/applications/webApp/forward.ts:39 | Attempt 0 uses the preferred port when assigned; later attempts (or none preferred) a random 20000–44999 port | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:272 | |
| c01-67 | src/applications/webApp/forward.ts:42 | `addPortForward({ id, connectionId, localPort, remoteHost: 'localhost', remotePort, persist: false })` | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:276 (PortForwardConfig, Persistence::Ephemeral) | |
| c01-68 | src/applications/webApp/forward.ts:50 | Settled Active → return that local port | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:292 | |
| c01-69 | src/applications/webApp/forward.ts:52 | Not active (ExitOnForwardFailure: local port taken) → remove and try another | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:295 | |
| c01-70 | src/applications/webApp/forward.ts:55 | All attempts failed → error "Port forward failed after 5 attempts" | PARTIAL | app-rs/crates/treeterm-model/src/web_app.rs:298 | Message differs: Rust shows "Could not forward port {remote_port} after 5 attempts". |

## src/applications/webApp/ports.ts

| # | Electron | Behaviour | Status | Rust parallel | Notes |
|---|---|---|---|---|---|
| c01-71 | src/applications/webApp/ports.ts:5 | `HttpProbe` type (port → curl exit code) | NA | app-rs/crates/treeterm-model/src/web_app.rs:86 (probe) | Type alias. |
| c01-72 | src/applications/webApp/ports.ts:8 | `CURL_OK = 0` | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:194 (literal `0`) | |
| c01-73 | src/applications/webApp/ports.ts:9 | `CURL_COULDNT_CONNECT = 7` | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:27 | |
| c01-74 | src/applications/webApp/ports.ts:11 | `PORT_RANGE_START = 20000` | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:20 | |
| c01-75 | src/applications/webApp/ports.ts:12 | `PORT_RANGE_SIZE = 25000` | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:21 | |
| c01-76 | src/applications/webApp/ports.ts:13 | `FIND_PORT_ATTEMPTS = 20` | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:22 | |
| c01-77 | src/applications/webApp/ports.ts:14 | `POLL_INTERVAL_MS = 1000` | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:23 (POLL_INTERVAL) | |
| c01-78 | src/applications/webApp/ports.ts:16 | `WaitOutcome` Ready/Cancelled | NA | app-rs/crates/treeterm-model/src/web_app.rs:170 (`_run` task) | Rust cancels by dropping the start-up task; no outcome value needed. |
| c01-79 | src/applications/webApp/ports.ts:21 | `randomPort`: 20000 + floor(random × 25000) | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:35 (port_from), :39 (random_port) | Test web_app.rs:312 checks 20000..=44999. |
| c01-80 | src/applications/webApp/ports.ts:29 | `createHttpProbe`: curl runs through the daemon's exec (same host as the server, local or remote), cwd `/` | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:86 (probe); workspace_store.rs:721 (`conn.exec_client()`) | |
| c01-81 | src/applications/webApp/ports.ts:32 | curl args `-s -o /dev/null --connect-timeout 1 --max-time 2 http://localhost:<port>/` | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:90 | Identical. |
| c01-82 | src/applications/webApp/ports.ts:33 | Exec timeout `DEFAULT_EXEC_TIMEOUT_MS` (30 000 ms) | PARTIAL | app-rs/crates/treeterm-model/src/web_app.rs:96 | Rust uses 10 s (`ExecTimeout::After(10s)`). Only matters if exec itself hangs (curl caps at 2 s). |
| c01-83 | src/applications/webApp/ports.ts:34 | Exec start failure → throw `startResult.error` (shown as the page error) | PARTIAL | app-rs/crates/treeterm-model/src/web_app.rs:98 | Rust error text is prefixed: `curl: {error}`. |
| c01-84 | src/applications/webApp/ports.ts:38 | Exit event → resolve with curl's exit code (unsubscribe) | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:98 (`output.exit_code`) | |
| c01-85 | src/applications/webApp/ports.ts:41 | Exec Error event (e.g. curl missing) → reject with its message | PARTIAL | app-rs/crates/treeterm-model/src/web_app.rs:98 | Same `curl: ` prefix difference as c01-83. (v) Also covers a stream that ends without a result: Electron main emits Error "exec stream ended without result" (src/main/index.ts:658); Rust returns `ExecError::EndedWithoutResult` (app-rs/crates/treeterm-core/src/exec.rs:250), shown as "curl: exec stream ended without result". |
| c01-86 | src/applications/webApp/ports.ts:51 | `findFreePort`: up to 20 random ports probed | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:176 | |
| c01-87 | src/applications/webApp/ports.ts:55 | Exit 7 (couldn't connect) → port is free, use it | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:178 | |
| c01-88 | src/applications/webApp/ports.ts:55 | Any other exit (something answers / other error) → try another port | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:178 | Probe errors propagate in both. |
| c01-89 | src/applications/webApp/ports.ts:57 | Error "No free port found after 20 attempts" | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:183 | Same text. |
| c01-90 | src/applications/webApp/ports.ts:68 | `waitForHttp`: stop when cancelled (tab disposed) | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:145 (close drops `_run`); workspace_store.rs:1121 (dispose drops the model) | |
| c01-91 | src/applications/webApp/ports.ts:70 | curl exit 0 (any HTTP status) → server ready | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:194 | |
| c01-92 | src/applications/webApp/ports.ts:71 | Otherwise wait 1 s and probe again | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:195 | |
| c01-169 | src/applications/webApp/ports.ts:37 | Exec listener ignores Stdout/Stderr events and unsubscribes on the first Exit/Error (one result per probe) | PORTED | app-rs/crates/treeterm-core/src/exec.rs:231 (collect_output); app-rs/crates/treeterm-model/src/web_app.rs:98 | (v) Added. Rust buffers and discards the output (curl prints nothing with `-s -o /dev/null`); the stream is consumed up to the first Exit or error. |

## src/applications/webApp/renderer.ts

| # | Electron | Behaviour | Status | Rust parallel | Notes |
|---|---|---|---|---|---|
| c01-93 | src/applications/webApp/renderer.ts:12 | `WebAppDeps` (kill, ssh, openExternal, sleep, random) | NA | — | DI; each capability is mapped in its own row (c01-136, c01-143, c01-144). |
| c01-94 | src/applications/webApp/renderer.ts:20 | `resolveCommand`: replace every `$PORT` with the port | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:30 (resolve_command) | Test web_app.rs:306. |
| c01-95 | src/applications/webApp/renderer.ts:26 | id `webapp-<instance.id>`, name/icon from the instance | PORTED | app-rs/crates/treeterm-model/src/apps.rs:527 | |
| c01-96 | src/applications/webApp/renderer.ts:30 | createInitialState (terminal fields + ports + path) | PORTED | app-rs/crates/treeterm-model/src/apps.rs:137 | |
| c01-97 | src/applications/webApp/renderer.ts:33 | `keepOnExit: instance.keepOnExit` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:140 | |
| c01-98 | src/applications/webApp/renderer.ts:36 | `port` and `localPort` `{status:'unassigned'}`, `path: ''` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:143 | |
| c01-99 | src/applications/webApp/renderer.ts:43 | onWorkspaceLoad: a per-tab runtime store starting in AllocatingPort | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:102 (WebAppModel::new); workspace_store.rs:707 (init_tab WebApp arm) | |
| c01-100 | src/applications/webApp/renderer.ts:44 | Probe bound to the workspace's exec API and connection | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:721 | |
| c01-101 | src/applications/webApp/renderer.ts:45 | `forwardId = forwardIdForTab(tab.id)` | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:44 | |
| c01-102 | src/applications/webApp/renderer.ts:48 | Holds the in-flight PTY creation for close() | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:211 (TerminalTab::Creating) | See c01-137. |
| c01-103 | src/applications/webApp/renderer.ts:49 | `disposed` flag / `isCancelled()` stop the start-up flow | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:82 (`_run` task dropped) | |
| c01-104 | src/applications/webApp/renderer.ts:52 | `readState` / `update` helpers over the tab state | NA | app-rs/crates/treeterm-model/src/workspace_store.rs:1419 (update_tab_state) | Store plumbing. |
| c01-105 | src/applications/webApp/renderer.ts:59 | ensurePort: port already assigned → reuse it | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:172; workspace_store.rs:708 (reads `port`) | |
| c01-106 | src/applications/webApp/renderer.ts:60 | Otherwise find a free port and persist `port: {assigned, port}` | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:184 (PortAssigned); workspace_store.rs:1082 (on_web_app_event) | |
| c01-107 | src/applications/webApp/renderer.ts:66 | ensurePty: PTY already exists (ptyId) → nothing to create | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:732 (attach) | |
| c01-108 | src/applications/webApp/renderer.ts:67 | `ptyHandle ?? randomUUID()` fallback | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:717, :1110 | |
| c01-109 | src/applications/webApp/renderer.ts:68 | Create the server PTY with the command's `$PORT` resolved, in the workspace dir | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:734 (known port), :1114 (on PortAssigned) | |
| c01-110 | src/applications/webApp/renderer.ts:69 | Start-up awaits PTY creation; a creation failure rejects start() → Error phase with message and Retry | PARTIAL | app-rs/crates/treeterm-model/src/workspace_store.rs:1036 (on_pty_created Err → TerminalTab::Failed); app-rs/crates/treeterm-app/src/app_view.rs:2359 | Rust creates the PTY beside the start-up task. On failure the Browser sub-tab stays on "Waiting for server on port N…" forever (no Error/Retry), and the Shell sub-tab shows "The server starts once a port is found…" instead of the error (tab_content only exposes Attached terminals, workspace_store.rs:336). |
| c01-111 | src/applications/webApp/renderer.ts:70 | On creation: write `ptyId`, `ptyHandle` | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:1060 | |
| c01-112 | src/applications/webApp/renderer.ts:70 | On creation: write `connectionId` | PARTIAL | app-rs/crates/treeterm-model/src/workspace_store.rs:1060 | Not written by Rust (as c01-11). |
| c01-113 | src/applications/webApp/renderer.ts:74 | ensureLocalForward with preferred port = state `localPort` | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:202 | |
| c01-114 | src/applications/webApp/renderer.ts:83 | Persist `localPort: {assigned, port}` | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:205 (LocalPortAssigned); workspace_store.rs:1086 | |
| c01-115 | src/applications/webApp/renderer.ts:88 | start(): tab state gone or cancelled → stop | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:184 (`map_err "closed"`), :215 | Model only exists while the tab does. |
| c01-116 | src/applications/webApp/renderer.ts:90 | Every start (incl. Retry) shows AllocatingPort until port and PTY are ready | PARTIAL | app-rs/crates/treeterm-model/src/web_app.rs:163 | Cosmetic: with a known port Rust goes straight to WaitingForServer, so "Finding a free port…" is not shown (Electron shows it while a missing PTY is created). (v) Also: `start()` assigns `phase` without `cx.notify()` and `retry` (:132) does not notify (unlike `restart_forward` :140 and `BrowserPageModel::start`), so after clicking Retry the Error view can stay up until something else redraws the window (not checked at runtime). |
| c01-117 | src/applications/webApp/renderer.ts:92 | Re-read state after the port; tab gone or cancelled → stop | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:189 | |
| c01-118 | src/applications/webApp/renderer.ts:95 | Phase WaitingForServer{port} | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:164, :187 | Rust enters it before the PTY exists (see c01-110). |
| c01-119 | src/applications/webApp/renderer.ts:96 | Wait for HTTP; Cancelled → return quietly | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:194 | |
| c01-120 | src/applications/webApp/renderer.ts:97 | Local session → Ready with `http://localhost:<port>/` | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:198 | |
| c01-121 | src/applications/webApp/renderer.ts:101 | Remote session → phase Forwarding{port} | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:200 | |
| c01-122 | src/applications/webApp/renderer.ts:102 | Re-read state before forwarding (latest `localPort`); gone/cancelled → stop | PARTIAL | app-rs/crates/treeterm-model/src/web_app.rs:168 (`preferred` captured at start), :204 (`model.local_port`) | (v) Was PORTED. Rust never re-reads tab state: `preferred` is the model's `local_port`, read from `localPort` once at init (workspace_store.rs:715) and afterwards only updated by its own LocalPortAssigned. A `localPort` (likewise `port`, c01-105) written later by another window is ignored until the tab is re-initialised. Gone/cancelled is covered by the `_run` drop. Rarely visible (two windows on one ssh session). |
| c01-123 | src/applications/webApp/renderer.ts:104 | ensureLocalForward | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:202 | |
| c01-124 | src/applications/webApp/renderer.ts:105 | Cancelled after the forward → stop | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:203 | |
| c01-125 | src/applications/webApp/renderer.ts:106 | Ready with `http://localhost:<localPort>/` | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:208 | |
| c01-126 | src/applications/webApp/renderer.ts:111 | run(): failure after dispose is swallowed | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:216 | |
| c01-127 | src/applications/webApp/renderer.ts:112 | run(): failure → Error phase with `err.message` (or `String(err)`) | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:215; app-rs/crates/treeterm-app/src/app_view.rs:2367 (page_error with Retry) | |
| c01-128 | src/applications/webApp/renderer.ts:116 | Start-up runs as soon as the tab loads | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:119 | |
| c01-129 | src/applications/webApp/renderer.ts:119 | Ref exposes the runtime (phase) to the pane | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:123 (phase) | |
| c01-130 | src/applications/webApp/renderer.ts:120 | Ref carries page memory: the page reopens on the remembered `path` and every navigation on the start page's origin is saved to `path` | PARTIAL | app-rs/crates/treeterm-app/src/app_view.rs:2363 (`page_view(..., None, url, ...)`); app-rs/crates/treeterm-app/src/app_view.rs:2170 (page_view keeps one BrowserView per tab) | (v) Was MISSING. Within one run the page survives workspace switches because the tab's BrowserView is kept in `browser_views` (never pruned; off-screen webviews hidden, app_view.rs:700), which covers Electron's main reason for page memory (webview remount). But Web App tabs pass no memory, so `path` is never read or written: after a restart/reconnect, or when Retry/Restart yields a new local port (stale view replaced, app_view.rs:2171), the tab opens the start page; and Electron cannot reopen the page last visited in the native app. Searched `remember_pages_in`, `page_url`, `page_path`, `"path"` in web_app.rs / init_tab WebApp arm / app_view.rs, git log "page memory" (31ff7ce wired Browser tabs only). |
| c01-131 | src/applications/webApp/renderer.ts:121 | `retry` re-runs the whole start-up (port, PTY, wait, forward) | PARTIAL | app-rs/crates/treeterm-model/src/web_app.rs:132 (retry) | Rust retry restarts only the model's task; it never re-creates a failed server PTY (Electron's ensurePty retries it). |
| c01-132 | src/applications/webApp/renderer.ts:122 | `restartForward`: remove the forward, then re-run start-up | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:137 (restart_forward); app-rs/crates/treeterm-app/src/app_view.rs:2340 | |
| c01-133 | src/applications/webApp/renderer.ts:126 | Cached terminal + `disposeCachedTerminal` for the Shell sub-tab | PORTED | app-rs/crates/treeterm-app/src/app_view.rs:2344 (terminal_views), :2509 (prune_terminal_views) | |
| c01-134 | src/applications/webApp/renderer.ts:136 | close(): remote session → remove the tab's ssh forward | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:144 (close → remove_forward); workspace_store.rs:1362 | |
| c01-135 | src/applications/webApp/renderer.ts:137 | close(): connection/ptyId from current state (`connectionId ?? ws.connectionId`) | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:1366 | Kills via the workspace connection. |
| c01-136 | src/applications/webApp/renderer.ts:140 | close(): PTY known → kill it | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:1366 (TerminalModel::kill) | |
| c01-137 | src/applications/webApp/renderer.ts:145 | close() during PTY creation → kill once the id resolves | PARTIAL | app-rs/crates/treeterm-model/src/workspace_store.rs:1369, :1048 | Same defect as c01-18: dispose_tab drops the Creating task, so the kill in on_pty_created never runs. |
| c01-138 | src/applications/webApp/renderer.ts:148 | dispose(): mark cancelled (start-up stops) | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:1121 (dispose_tab drops the WebAppModel) | Forward and PTY are kept, as in Electron. |
| c01-139 | src/applications/webApp/renderer.ts:149 | dispose(): drop cached terminal | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:1120 | |
| c01-140 | src/applications/webApp/renderer.ts:150 | dispose(): remove activity state | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:1122 | |
| c01-141 | src/applications/webApp/renderer.ts:157 | render: state not a WebAppState (no `port`/`localPort` objects) → render nothing | PARTIAL | app-rs/crates/treeterm-model/src/workspace_store.rs:708 (`assigned` helper) | Rust has no shape guard: missing/odd `port`/`localPort` are treated as unassigned and the tab starts normally. |
| c01-142 | src/applications/webApp/renderer.ts:160 | render `WebAppPane` (keyed by tab, workspace, tabId, isVisible) | PORTED | app-rs/crates/treeterm-app/src/app_view.rs:2315 (web_app_tab) | Pane details belong to the WebAppPane chunk. |
| c01-143 | src/applications/webApp/renderer.ts:165 | Pane gets the ssh API (Port Forward sub-tab: status, output, restart) | PORTED | app-rs/crates/treeterm-model/src/forward_monitor.rs:29 (ForwardMonitor); app-rs/crates/treeterm-app/src/app_view.rs:2193 (port_forward_pane) | |
| c01-144 | src/applications/webApp/renderer.ts:166 | Pane gets `openExternal` (open page in system browser) | PORTED | app-rs/crates/treeterm-app/src/browser_view.rs:535 ("Open in external browser") | |
| c01-145 | src/applications/webApp/renderer.ts:170 | `canClose: true` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:536 | |
| c01-146 | src/applications/webApp/renderer.ts:171 | `showInNewTabMenu: true` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:537 | |
| c01-147 | src/applications/webApp/renderer.ts:172 | `displayStyle: 'flex'` | NA | — | CSS layout. |
| c01-148 | src/applications/webApp/renderer.ts:173 | `isDefault: instance.isDefault` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:538 | (v) Inert in both apps (see c01-29). |

## src/applications/webApp/runtime.ts

| # | Electron | Behaviour | Status | Rust parallel | Notes |
|---|---|---|---|---|---|
| c01-149 | src/applications/webApp/runtime.ts:4 | `WebAppPhase`: allocating-port, waiting-for-server, forwarding, ready, error | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:49 (WebAppPhase) | |
| c01-150 | src/applications/webApp/runtime.ts:12 | `WebAppRuntime` payloads (port / url / message per phase) | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:49 | Variants carry the same data. |
| c01-151 | src/applications/webApp/runtime.ts:20 | `PageMemory` interface (pageUrl, rememberPage) | PORTED | app-rs/crates/treeterm-app/src/browser_view.rs:80 (`memory`); app-rs/crates/treeterm-model/src/apps.rs:305, :327 | Used by Browser tabs only; see c01-130/c01-155. |
| c01-152 | src/applications/webApp/runtime.ts:28 | `PageRef`: runtime, `retry`, `restartForward` (shared by WebApp and Browser) | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:132, :137; app-rs/crates/treeterm-model/src/browser_page.rs:62, :67 | |
| c01-153 | src/applications/webApp/runtime.ts:36 | `WebAppRef` type | NA | — | Type composition. |
| c01-154 | src/applications/webApp/runtime.ts:38 | `forwardIdForTab` → `webapp-<tabId>` | PORTED | app-rs/crates/treeterm-model/src/web_app.rs:44 | Test web_app.rs:316. |
| c01-155 | src/applications/webApp/runtime.ts:46 | `createPageMemory(workspaceStore, tabId)`: binds page memory to a tab (WebApp and Browser) | PARTIAL | app-rs/crates/treeterm-app/src/browser_view.rs:130 (remember_pages_in) | Wired for Browser tabs only (app_view.rs:2290); Web App tabs get none (c01-130). (v) Note refined: Web App pages survive workspace switches via the cached view, not restarts or local-port changes. |
| c01-156 | src/applications/webApp/runtime.ts:48 | `pageUrl(home)`: the remembered path resolved on the start page's origin | PARTIAL | app-rs/crates/treeterm-model/src/apps.rs:305 (page_url); app-rs/crates/treeterm-model/src/browser_page.rs:87 | (v) Was PORTED. Electron uses `new URL(path, home)` (an absolute path replaces home's path); Rust appends the path to home, which only matches when the instance URL is a bare origin. For a Browser instance `http://localhost:3000/app/` the first load stores `path` "/app/" (page_path keeps the full path), and the next init opens `http://localhost:3000/app/app/` (Electron: `http://localhost:3000/app/`); a home with a query breaks the same way (`…/?q=1/x`). Tests apps.rs:627-629 cover only root homes. Browser tabs only (Web App tabs have no memory, c01-130). |
| c01-157 | src/applications/webApp/runtime.ts:50 | `state?.path ?? ''`: tabs persisted before `path` existed open the start page | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:753 (`unwrap_or_default`) | Browser tabs. (v) `path` is read once at tab init; see c01-171. |
| c01-158 | src/applications/webApp/runtime.ts:55 | rememberPage: pages on another origin are not remembered | PORTED | app-rs/crates/treeterm-model/src/apps.rs:327 (page_path) | Test apps.rs `pages_are_remembered_only_on_the_home_origin`. |
| c01-159 | src/applications/webApp/runtime.ts:56 | rememberPage writes `path = pathname + search + hash` to the tab state | PORTED | app-rs/crates/treeterm-app/src/browser_view.rs:293 (on_page) | Browser tabs. |
| c01-171 | src/applications/webApp/runtime.ts:49 | `pageUrl` reads the tab's current `path` whenever the page (re)mounts, so a remount after Retry/new forward opens the last remembered page | PARTIAL | app-rs/crates/treeterm-model/src/browser_page.rs:87 (start uses `self.path`), :46 (set once in `new`) | (v) Added. Rust reads `path` once at tab init (workspace_store.rs:753): a Browser tab whose Retry/Restart gets a new local port (new BrowserView) opens the init-time path, not the page last navigated to. Remote sessions with a changed local port only. |

## src/applications/workspaceSettings/renderer.ts

| # | Electron | Behaviour | Status | Rust parallel | Notes |
|---|---|---|---|---|---|
| c01-160 | src/applications/workspaceSettings/renderer.ts:6 | `WorkspaceSettingsConnected`: live list of registered applications (re-renders when settings add/remove apps) | PORTED | app-rs/crates/treeterm-app/src/tool_views/workspace_settings_view.rs:67 (`default_app_choices(store.registry())`); workspace_store.rs:1627 (apply_settings) | Read from the live registry each render. |
| c01-161 | src/applications/workspaceSettings/renderer.ts:20 | render `WorkspaceSettings` for this workspace with the applications | PORTED | app-rs/crates/treeterm-app/src/tool_views/mod.rs:115 (WorkspaceSettingsView) | |
| c01-162 | src/applications/workspaceSettings/renderer.ts:12 | id `workspace-settings`, name `Settings`, icon ⚙ | PORTED | app-rs/crates/treeterm-model/src/apps.rs:478 | `\u{2699}`. |
| c01-163 | src/applications/workspaceSettings/renderer.ts:16 | createInitialState `{}` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:61 | |
| c01-164 | src/applications/workspaceSettings/renderer.ts:18 | onWorkspaceLoad: no-op close/dispose | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:856 (ToolTab::WorkspaceSettings, no runtime) | |
| c01-165 | src/applications/workspaceSettings/renderer.ts:23 | `canClose: true` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:469 | |
| c01-166 | src/applications/workspaceSettings/renderer.ts:24 | `showInNewTabMenu: false` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:478 (`false`) | Test apps.rs:767 asserts it. |
| c01-167 | src/applications/workspaceSettings/renderer.ts:25 | `displayStyle: 'flex'` | NA | — | CSS layout. |
| c01-168 | src/applications/workspaceSettings/renderer.ts:26 | `isDefault: false` | PORTED | app-rs/crates/treeterm-model/src/apps.rs:471 | |

## Gaps

- c01-11, PARTIAL, src/applications/terminal/renderer.ts:38: `connectionId` is never written into terminal tab state. Only Electron's close() reads it, and it falls back to the workspace's connection, so nothing visible changes. Nearest: app-rs/crates/treeterm-model/src/workspace_store.rs:1060 (on_pty_created).
- c01-18, PARTIAL, src/applications/terminal/renderer.ts:61: closing a tab, or removing its workspace, while its PTY is being created does not kill that PTY. `dispose_tab` drops `TerminalTab::Creating { _task }`, which aborts the RPC (`rt::Fut` AbortOnDrop), so `on_pty_created`'s "tab gone → kill" never runs, and a PTY the daemon already spawned is orphaned. Nearest: workspace_store.rs:1369 (remove_tab), :1048 (on_pty_created), :1650 (close_all_tabs).
- c01-70, PARTIAL, src/applications/webApp/forward.ts:55: the final forward error is "Could not forward port N after 5 attempts" (Electron: "Port forward failed after 5 attempts"). Nearest: web_app.rs:298.
- c01-82, PARTIAL, src/applications/webApp/ports.ts:33: the curl probe's exec timeout is 10 s (Electron: 30 s). Matters only if exec itself hangs. Nearest: web_app.rs:96.
- c01-83, PARTIAL, src/applications/webApp/ports.ts:34: an error from exec failing to start is shown with a `curl: ` prefix. Nearest: web_app.rs:98.
- c01-85, PARTIAL, src/applications/webApp/ports.ts:41: an exec Error event (and a stream ending without a result) is also shown with the `curl: ` prefix. Nearest: web_app.rs:98.
- c01-110, PARTIAL, src/applications/webApp/renderer.ts:69: when the server PTY fails to start there is no Error phase and no Retry. The page stays on "Waiting for server on port N…", and the Shell sub-tab shows "The server starts once a port is found…" instead of the error. Nearest: workspace_store.rs:1036, :336; app_view.rs:2359.
- c01-112, PARTIAL, src/applications/webApp/renderer.ts:70: `connectionId` is never written into Web App tab state (no visible effect, as c01-11). Nearest: workspace_store.rs:1060.
- c01-116, PARTIAL, src/applications/webApp/renderer.ts:90: a start with a known port skips the AllocatingPort phase (cosmetic). `WebAppModel::start` and `retry` also change the phase without `cx.notify()`, so after Retry the Error view may stay until something else redraws (not checked at runtime). Nearest: web_app.rs:132, :163.
- c01-122, PARTIAL, src/applications/webApp/renderer.ts:102: the Web App model never re-reads `localPort` (or `port`) from tab state after init, so a value another window wrote later is ignored. Nearest: web_app.rs:168; workspace_store.rs:715.
- c01-130, PARTIAL, src/applications/webApp/renderer.ts:120: Web App tabs have no persisted page memory. Pages survive workspace switches (cached BrowserView), but `path` is never read or written, so after a restart or reconnect, or when the local port changes, the tab opens the start page. Nearest: app_view.rs:2363 (passes `None`); the Browser-tab implementation is browser_view.rs:130.
- c01-131, PARTIAL, src/applications/webApp/renderer.ts:121: Retry does not re-create a failed server PTY (Electron's ensurePty retries it). Nearest: web_app.rs:132.
- c01-137, PARTIAL, src/applications/webApp/renderer.ts:145: closing a Web App tab while its PTY is being created does not kill that PTY (same mechanism as c01-18). Nearest: workspace_store.rs:1369, :1048.
- c01-141, PARTIAL, src/applications/webApp/renderer.ts:157: there is no WebAppState shape guard. A malformed state is treated as unassigned ports and the tab starts, where Electron renders nothing. Nearest: workspace_store.rs:708.
- c01-155, PARTIAL, src/applications/webApp/runtime.ts:46: page memory (`createPageMemory`) is wired for Browser tabs only, not Web App tabs (c01-130). Nearest: browser_view.rs:130; app_view.rs:2290.
- c01-156, PARTIAL, src/applications/webApp/runtime.ts:48: `page_url` appends the remembered path to the home URL instead of resolving it. A Browser instance URL with a path (or query) reopens at a doubled path, e.g. `http://localhost:3000/app/app/`. Nearest: apps.rs:305 (page_url); browser_page.rs:87.
- c01-171, PARTIAL, src/applications/webApp/runtime.ts:49: a Browser tab reads `path` once at init, so a remount after Retry/Restart with a new local port opens the init-time page, not the last one remembered. Nearest: browser_page.rs:46, :87.
