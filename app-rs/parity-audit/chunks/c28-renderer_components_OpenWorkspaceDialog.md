# c28-renderer_components_OpenWorkspaceDialog

Verified: +2 rows, 23 rows changed

Rust paths below are relative to the repo root. `owd` = `app-rs/crates/treeterm-app/src/open_workspace_dialog.rs`, `bv` = `app-rs/crates/treeterm-app/src/browser_view.rs`, `av` = `app-rs/crates/treeterm-app/src/app_view.rs`, `apps` = `app-rs/crates/treeterm-model/src/apps.rs`, `fm` = `app-rs/crates/treeterm-model/src/forward_monitor.rs`, `hp` = `app-rs/crates/treeterm-model/src/harness_prompts.rs`. `wry` = the `lb-wry` 0.53.3 crate the webview is built on (`~/.cargo/registry/src/*/lb-wry-0.53.3`).

## src/renderer/components/OpenWorkspaceDialog.tsx

| # | Electron | Behaviour | Status | Rust parallel | Notes |
|---|---|---|---|---|---|
| c28-1 | src/renderer/components/OpenWorkspaceDialog.tsx:8 | `EMPTY_RECENT` / `?? EMPTY_RECENT` (:25): a connection with no recent dirs gets an empty list | PORTED | app-rs/crates/treeterm-model/src/session_names.rs:113 (recent_directories, `map_or(&[])`) | |
| c28-2 | src/renderer/components/OpenWorkspaceDialog.tsx:10 | `OpenWorkspaceDialogProps` type (onOpen, onCancel, selectFolder, connectionKey, isRemote, readDirectory) | PORTED | owd:17 (OnPicked), owd:20 (Location), owd:479 (open) | `onOpen`'s `settings` argument has no Rust counterpart (see c28-18) |
| c28-3 | src/renderer/components/OpenWorkspaceDialog.tsx:13 | `selectFolder` capability: native directory picker, null when cancelled | PORTED | owd:130 (pick_native, `prompt_for_paths` directories only) | Rust titles the picker "Open Workspace" |
| c28-4 | src/renderer/components/OpenWorkspaceDialog.tsx:14 | `connectionKey`: recent dirs kept per connection (`local` / `user@host:port`) | PORTED | app-rs/crates/treeterm-core/src/ui_state.rs:41 (session_key), app-rs/crates/treeterm-app/src/tree_pane.rs:149 | Stored in `ui-state.json` as `local` / `ssh:user@host:port`; Electron's localStorage list is not imported |
| c28-5 | src/renderer/components/OpenWorkspaceDialog.tsx:15 | `isRemote` selects remote behaviour (placeholder, Browse) | PORTED | app-rs/crates/treeterm-app/src/tree_pane.rs:144 (Location::Remote / Local) | |
| c28-6 | src/renderer/components/OpenWorkspaceDialog.tsx:16 | `readDirectory` capability: daemon ReadDirectory(workspace `/`, dirPath) | SHARED | owd:194 (`fs_client().read_directory("/", &path)`), app-rs/crates/treeterm-core/src/fs.rs:178 | Same daemon RPC |
| c28-7 | src/renderer/components/OpenWorkspaceDialog.tsx:20 | `selectedPath` state, starts empty | PORTED | owd:86 (folder InputState) | |
| c28-8 | src/renderer/components/OpenWorkspaceDialog.tsx:21 | `isSelecting` state (native picker up) | PORTED | owd:28 (Picker::Native { picking }), owd:101 (picking) | |
| c28-9 | src/renderer/components/OpenWorkspaceDialog.tsx:22 | `selectedAppId` state (default application choice, '' = global) | MISSING | nearest: app-rs/crates/treeterm-model/src/tool_tabs/workspace_settings.rs:17 | (v) Re-searched "Default Application", default_application_id, selectedAppId, default_app_choices, "Use Global Default": `OpenWorkspaceDialog` (owd:47) holds only folder/recent/picker, and `OnPicked` (owd:17) carries only the path |
| c28-10 | src/renderer/components/OpenWorkspaceDialog.tsx:23 | `showBrowser` state (remote browser shown) | PORTED | owd:30 (Picker::Remote { browser: Option<Browser> }) | |
| c28-11 | src/renderer/components/OpenWorkspaceDialog.tsx:26 | `addRecent` action: move to front, dedupe, keep 10 | PORTED | app-rs/crates/treeterm-model/src/session_names.rs:118 (add_recent_directory), app-rs/crates/treeterm-core/src/settings/mod.rs:42 (MAX_RECENT_DIRECTORIES = 10) | Test app-rs/crates/treeterm-model/tests/session_names.rs:92 |
| c28-12 | src/renderer/components/OpenWorkspaceDialog.tsx:28 | `availableApps` = registered applications filtered by `showInNewTabMenu` | MISSING | nearest: app-rs/crates/treeterm-model/src/tool_tabs/workspace_settings.rs:17 (default_app_choices), apps:586 (new_tab_menu) | (v) The same list ("Use Global Default" + `new_tab_menu()`) exists as `default_app_choices` and is used by the workspace Settings tab (tool_views/workspace_settings_view.rs:67) and the child dialog, but not by the Open Workspace dialog |
| c28-13 | src/renderer/components/OpenWorkspaceDialog.tsx:32 | `handleSelectFolder` marks the picker as up | PORTED | owd:118 | |
| c28-14 | src/renderer/components/OpenWorkspaceDialog.tsx:35 | `if (path)`: a chosen folder replaces the typed path; cancel keeps it | PORTED | owd:137-149 | |
| c28-15 | src/renderer/components/OpenWorkspaceDialog.tsx:38 | `finally`: picker flag cleared even when the picker fails | PORTED | owd:140-146 | Rust also logs the failure |
| c28-16 | src/renderer/components/OpenWorkspaceDialog.tsx:44 | `handleOpen` guard: empty path does nothing | PORTED | owd:107 (open_selected) | Rust also refuses while the picker is up |
| c28-17 | src/renderer/components/OpenWorkspaceDialog.tsx:46 | Opening adds the path to this connection's recent dirs | PORTED | app-rs/crates/treeterm-app/src/tree_pane.rs:157 | |
| c28-18 | src/renderer/components/OpenWorkspaceDialog.tsx:48 | `settings = selectedAppId ? { defaultApplicationId } : undefined`: the new workspace saves the chosen app, opens its first tab with it and children inherit it | MISSING | nearest: app-rs/crates/treeterm-model/src/session_store.rs:1407 (open_workspace(path) has no settings argument), :1448 (default_for_workspace([], global)), :1477 (`WorktreeSettings::default()`) | (v) Confirmed by reading `finish_open_workspace`: a root workspace always gets the global default app as its first tab and empty settings. Electron's `addWorkspace(path, { settings })` (createSessionStore.ts:1484, :1508) uses the chosen app for the first tab and saves it. In Rust it can only be set afterwards in the workspace Settings tab (workspace_store.rs:1332 set_default_application), which affects children only, not the already-created first tab |
| c28-19 | src/renderer/components/OpenWorkspaceDialog.tsx:52 | `onOpen(path, settings)`: SessionPanel adds the workspace and closes the dialog | PORTED | owd:110-111, app-rs/crates/treeterm-app/src/tree_pane.rs:158 (open_workspace + select) | Settings part: c28-18 |
| c28-20 | src/renderer/components/OpenWorkspaceDialog.tsx:56 | Enter anywhere in the dialog opens when a path is set and the picker is not up | PARTIAL | owd:89 (PressEnter on the folder field) -> owd:105 | (v) Note rewritten. Same guard. Rust reacts only to Enter in the folder field. That field keeps focus when a recent row is clicked, so "click a recent dir, press Enter" works. The remaining difference is where else Enter can be pressed. In the remote browser's path field (owd:167), Enter only navigates in Rust. In Electron it navigates and also bubbles to the dialog, which opens the folder-field path when one is set. With focus on a dialog button, Enter opens in Electron. Rust has no focusable buttons |
| c28-21 | src/renderer/components/OpenWorkspaceDialog.tsx:59 | Escape cancels the dialog | PORTED | app-rs/vendor/gpui-component/src/dialog/dialog.rs:226,737 (keyboard default, close_on_escape) | |
| c28-22 | src/renderer/components/OpenWorkspaceDialog.tsx:64 | `handleSelectRecent`: a recent dir fills the folder field | PORTED | owd:380-383 | |
| c28-23 | src/renderer/components/OpenWorkspaceDialog.tsx:68 | `getFolderName`: last `/` or `\` segment, else the whole path | PORTED | owd:65 (folder_name), test owd:492 | |
| c28-24 | src/renderer/components/OpenWorkspaceDialog.tsx:74 | Clicking the overlay cancels | PORTED | app-rs/vendor/gpui-component/src/dialog/dialog.rs:229,738 (overlay_closable default) | |
| c28-25 | src/renderer/components/OpenWorkspaceDialog.tsx:75 | `stopPropagation` so clicks inside do not reach the overlay | PORTED | app-rs/vendor/gpui-component/src/dialog/dialog.rs:738 (close_on_backdrop_press: only the backdrop closes) | (v) Was NA. The visible effect (a click inside the dialog does not cancel it) holds in Rust |
| c28-26 | src/renderer/components/OpenWorkspaceDialog.tsx:77 | Title "Open Workspace" | PORTED | owd:440 | |
| c28-27 | src/renderer/components/OpenWorkspaceDialog.tsx:78 | Close button `×` cancels | PORTED | owd:440, app-rs/crates/treeterm-app/src/ui.rs:299 (dialog_header) | |
| c28-28 | src/renderer/components/OpenWorkspaceDialog.tsx:84 | Recent section hidden when the list is empty | PORTED | owd:435 | |
| c28-29 | src/renderer/components/OpenWorkspaceDialog.tsx:86 | Label "Recent Directories" | PORTED | owd:395 | |
| c28-30 | src/renderer/components/OpenWorkspaceDialog.tsx:87 | Recent list scrolls past 200px | PORTED | owd:355 (max_h 200, overflow_y_scroll) | |
| c28-31 | src/renderer/components/OpenWorkspaceDialog.tsx:88 | One clickable row per recent dir, most recent first | PORTED | owd:356-384 | |
| c28-32 | src/renderer/components/OpenWorkspaceDialog.tsx:93 | Row tooltip `title={dir}` shows the full path | PARTIAL | owd:358-383 (render_recent) | (v) Confirmed. Rust has no tooltip. Both apps truncate the path line (Electron `.recent-directory-path` uses `text-overflow: ellipsis`; Rust uses `truncate()` at owd:379), so in Rust a long path cannot be read in full |
| c28-33 | src/renderer/components/OpenWorkspaceDialog.tsx:95 | Row shows folder name over full path | PORTED | owd:377, owd:379 | |
| c28-34 | src/renderer/components/OpenWorkspaceDialog.tsx:104 | Label "Folder" | PORTED | owd:455 | |
| c28-35 | src/renderer/components/OpenWorkspaceDialog.tsx:106 | Editable path field bound to selectedPath | PORTED | owd:86, owd:90 (Change -> notify), owd:461 | |
| c28-36 | src/renderer/components/OpenWorkspaceDialog.tsx:110 | Placeholder: remote "Enter remote path (e.g. /home/user/project)", local "Enter or browse for a folder..." | PORTED | owd:81, owd:83 | Text matches |
| c28-37 | src/renderer/components/OpenWorkspaceDialog.tsx:112 | `autoFocus` on the path field | PORTED | owd:483 | |
| c28-38 | src/renderer/components/OpenWorkspaceDialog.tsx:114 | Local only: Browse button opens the native picker; disabled while it is up | PORTED | owd:114-119 (browse), owd:427 | |
| c28-39 | src/renderer/components/OpenWorkspaceDialog.tsx:120 | Label "Opening..." while picking, else "Browse..." | PORTED | owd:409, owd:411 | |
| c28-40 | src/renderer/components/OpenWorkspaceDialog.tsx:123 | Remote Browse button only when remote and `readDirectory` given | PORTED | owd:30 (Remote always carries the session) | |
| c28-41 | src/renderer/components/OpenWorkspaceDialog.tsx:126 | Remote Browse toggles the directory browser | PORTED | owd:121-124 | |
| c28-42 | src/renderer/components/OpenWorkspaceDialog.tsx:128 | Label "Hide" while shown, else "Browse..." | PORTED | owd:410-411 | |
| c28-43 | src/renderer/components/OpenWorkspaceDialog.tsx:134 | Browser rendered under the field while shown | PORTED | owd:431-434, owd:465 | |
| c28-44 | src/renderer/components/OpenWorkspaceDialog.tsx:137 | Browser starts at the typed path, else `/` | PORTED | owd:163 (show_browser) | |
| c28-45 | src/renderer/components/OpenWorkspaceDialog.tsx:138 | Browser `onSelect` fills the path and hides the browser | PORTED | owd:338-343 | |
| c28-46 | src/renderer/components/OpenWorkspaceDialog.tsx:142 | Browser `onCancel` hides it | PORTED | owd:335, owd:183 (hide_browser) | |
| c28-47 | src/renderer/components/OpenWorkspaceDialog.tsx:147 | Label "Default Application" | MISSING | nearest: app-rs/crates/treeterm-app/src/child_dialog.rs:795 (same label in the child dialog) | (v) Searched "Default Application" across app-rs/crates. It appears only in child_dialog.rs, workspace_settings_view.rs ("Default Application for Children") and settings_view ("Default Application for New Worktrees"); owd's render (owd:402-475) has only the Recent, Folder and browser sections |
| c28-48 | src/renderer/components/OpenWorkspaceDialog.tsx:148 | Default-application select bound to selectedAppId | MISSING | nearest: app-rs/crates/treeterm-app/src/tool_views/workspace_settings_view.rs:67 (same picker in the Settings tab) | (v) Searched pane_select_box, PopupMenu, default_app in owd: none. The UI for c28-18 |
| c28-49 | src/renderer/components/OpenWorkspaceDialog.tsx:153 | Option "Use Global Default" (value '') | MISSING | nearest: app-rs/crates/treeterm-model/src/tool_tabs/workspace_settings.rs:18 | (v) The Rust dialog always behaves as if this option were chosen |
| c28-50 | src/renderer/components/OpenWorkspaceDialog.tsx:154 | Disabled separator option "──────────" | MISSING | nearest: app-rs/crates/treeterm-app/src/tool_views/workspace_settings_view.rs (menu.separator() in the Settings-tab picker) | (v) Searched "──" across app-rs/crates: no hits |
| c28-51 | src/renderer/components/OpenWorkspaceDialog.tsx:155 | One option per new-tab-menu app (id -> name) | MISSING | nearest: app-rs/crates/treeterm-model/src/tool_tabs/workspace_settings.rs:19 | (v) The list exists (c28-12) but is not offered here |
| c28-52 | src/renderer/components/OpenWorkspaceDialog.tsx:161 | Hint "The application to open by default in this workspace. Child worktrees will inherit this setting." | MISSING | | (v) Searched "Child worktrees will inherit", "inherit this setting", "open by default in this workspace": no hits |
| c28-53 | src/renderer/components/OpenWorkspaceDialog.tsx:169 | Cancel button | PORTED | owd:470 | |
| c28-54 | src/renderer/components/OpenWorkspaceDialog.tsx:172 | Open button; disabled when the path is empty or the picker is up | PORTED | owd:436-438 | |
| c28-158 | src/renderer/components/OpenWorkspaceDialog.tsx:74 | `.dialog-overlay` dims the window (rgba 0,0,0,.6) under a centred 500px dialog | PORTED | owd:484 (dialog_frame(.., 500.)), app-rs/crates/treeterm-app/src/ui.rs:250 (centered), app-rs/vendor/gpui-component/src/root.rs:304 (overlay_visible on the topmost dialog) | (v) Added. Overlay colour is the Kit theme's `overlay` |

## src/renderer/components/PageViews.tsx

| # | Electron | Behaviour | Status | Rust parallel | Notes |
|---|---|---|---|---|---|
| c28-55 | src/renderer/components/PageViews.tsx:13 | `PageSsh` type: list forwards, status events, output watch | PORTED | app-rs/crates/treeterm-core/src/connection/manager.rs:355 (list_port_forwards), :372 (watch_port_forward_status), :362 (watch_port_forward_output) | In-process manager instead of IPC |
| c28-56 | src/renderer/components/PageViews.tsx:15 | `DetailsLink` { label, onClick } | PORTED | av:2115 (page_link) | |
| c28-57 | src/renderer/components/PageViews.tsx:20 | `BrowserViewProps` (runtime, onRetry, detailsLinks, openExternal, memory) | PORTED | av:2271 (browser_tab), av:2315 (web_app_tab) | |
| c28-58 | src/renderer/components/PageViews.tsx:30 | Detail links rendered as accent link buttons | PORTED | av:2115-2124 | Browser on ssh: "View port forward" (av:2285); Web App: "View shell" (av:2336) |
| c28-59 | src/renderer/components/PageViews.tsx:33 | `waiting(message)`: spinner, message, links | PORTED | av:2127 (page_waiting) | |
| c28-60 | src/renderer/components/PageViews.tsx:42 | One view per phase, exhaustive | PORTED | av:2362-2370, av:2287-2296 | Exhaustive `match` |
| c28-61 | src/renderer/components/PageViews.tsx:43 | AllocatingPort: "Finding a free port…" | PORTED | av:2364 | |
| c28-62 | src/renderer/components/PageViews.tsx:44 | WaitingForServer: "Waiting for server on port N…" | PORTED | av:2365 | |
| c28-63 | src/renderer/components/PageViews.tsx:45 | Forwarding: "Starting port forward for port N…" | PORTED | av:2366, av:2292 | |
| c28-64 | src/renderer/components/PageViews.tsx:46 | Ready: the embedded page with openExternal and page memory | PORTED | av:2170 (page_view), av:2289, av:2363 | Memory differences: c28-71, c28-78 |
| c28-65 | src/renderer/components/PageViews.tsx:47 | Error: red message, "Retry" (onRetry), links | PORTED | av:2148 (page_error) | |
| c28-66 | src/renderer/components/PageViews.tsx:57 | Phase-to-view cast and dispatch | NA | | TypeScript mechanics |
| c28-67 | src/renderer/components/PageViews.tsx:61 | `PageKind` / `PageContent` (web or markdown text) | PORTED | apps:334 (PageContent) | |
| c28-68 | src/renderer/components/PageViews.tsx:68 | `PageProbe` { contentType, url, text } | PORTED | apps:349 (PageProbe) | |
| c28-69 | src/renderer/components/PageViews.tsx:75 | `PROBE_SCRIPT`: contentType, location.href, body innerText or "" | PORTED | apps:344 (PAGE_PROBE_SCRIPT) | Wrapped in JSON.stringify for wry |
| c28-70 | src/renderer/components/PageViews.tsx:77 | `WebViewProps`: url is the start page (Home) | PORTED | bv:69 (BrowserView.url), bv:130 | |
| c28-71 | src/renderer/components/PageViews.tsx:87 | `src = memory.pageUrl(url)`, read once per mount: the tab reopens on its remembered page | PARTIAL | app-rs/crates/treeterm-model/src/browser_page.rs:87, apps:305 (page_url), av:2171-2173 | (v) Note extended. Browser tabs: `page_url` concatenates instead of resolving like `new URL(path, home)`. The first load already stores `path` = the start page's own path, so a start page with a path (`http://localhost:3000/app/`) reopens at `/app/app/`, and a page remembered at `/app/x` reopens at `/app/app/x`. With an empty path the start page loses its trailing slash (`.../app/` opens `.../app`; test apps.rs:629 pins this for `/`). Web App tabs: `page_view(.., None, ..)` (av:2363), so they reopen on the start page. Within a session Rust keeps the live view across workspace switches (Electron remounts and reloads it), so the Web App loss shows after a restart, a reconnect or a new forward port |
| c28-72 | src/renderer/components/PageViews.tsx:88 | Address starts at the opened page | PORTED | bv:104 | Normalised through resolve_address |
| c28-73 | src/renderer/components/PageViews.tsx:89 | Back/Forward start disabled | PORTED | bv:109-110 | |
| c28-74 | src/renderer/components/PageViews.tsx:90 | Not loading at first | PORTED | bv:108 | |
| c28-75 | src/renderer/components/PageViews.tsx:92 | Content starts as Web; previous content stays until the next load is probed | PORTED | bv:115, bv:280-290 | |
| c28-76 | src/renderer/components/PageViews.tsx:96 | `if (!view) return` | NA | | React ref timing |
| c28-77 | src/renderer/components/PageViews.tsx:97 | On navigation the address shows the new URL | PORTED | bv:250-270 (on_page Started/Finished) | (v) Note added: full loads only; in-page navigations: c28-80 |
| c28-78 | src/renderer/components/PageViews.tsx:100 | `memory.rememberPage(url, navigated)`: path+query+hash saved to tab state `path` when on the start page's origin | PARTIAL | bv:293-299, apps:327 (page_path) | (v) Confirmed. Browser tabs: ported (other origins are skipped, as in Electron). Web App tabs get no memory (av:2363 passes None; web_app.rs has no `path`), so their page is never saved to the tab state. Electron's WebApp ref spreads `createPageMemory` (src/applications/webApp/renderer.ts:120) |
| c28-79 | src/renderer/components/PageViews.tsx:101 | Back/Forward enabled from the webview's real history (canGoBack/canGoForward) | PARTIAL | bv:250-266, bv:314 (go) | (v) Note extended. Rust keeps its own back/forward stacks, fed by wry's Started event (a committed full load). History moves the page makes itself do not match: a page's `history.back()` is pushed as a new entry and clears Forward, and `location.replace` adds an entry. History moves that do not reload the page (pushState, hash) never reach the stacks. Toolbar Back still calls WKWebView `goBack`, which can land on such an entry and fire no load. The `Pending::Back` it set then mislabels the next real load (pops Back, pushes Forward) |
| c28-80 | src/renderer/components/PageViews.tsx:113 | `did-navigate-in-page` (hash, pushState) also updates address, memory and history | MISSING | nearest: bv:216 (with_on_page_load_handler) | (v) Re-searched pushState, hashchange, popstate, in-page, same-document, with_ipc_handler, initialization_script, did-navigate in app-rs/crates and vendor: none. wry only calls the handler from didCommitNavigation / didFinishNavigation (wry src/wkwebview/navigation.rs:17,38; WebKitGTK Committed/Finished, src/webkitgtk/mod.rs:489). With a single-page app's route changes, the address bar, the remembered `path` and Back/Forward go stale |
| c28-81 | src/renderer/components/PageViews.tsx:103 | `did-start-loading` sets loading | PARTIAL | bv:250-251 (Started -> loading) | (v) Was PORTED. wry's `Started` fires on didCommitNavigation (WebKitGTK `Committed`), when the response starts arriving, not when the request starts. While a slow server works on a request (e.g. a dev server compiling a route), Electron shows Stop and the progress line. Rust shows Reload and no progress until the page commits. A load that fails before committing never shows as loading |
| c28-82 | src/renderer/components/PageViews.tsx:104 | `did-stop-loading` clears loading (and the probe runs) | PARTIAL | bv:268-271 (Finished -> loading off, probe) | (v) Was PORTED. On macOS wry's delegate implements no didFailNavigation, so a load that fails after committing (connection dropped mid-page) sends no `Finished`. Stop and the progress line then stay up until another load finishes or Stop is clicked (bv:336 clears it by hand). Electron's `did-stop-loading` also fires after a failure |
| c28-83 | src/renderer/components/PageViews.tsx:106 | After each load, probe; `isMarkdownPage` -> Markdown(text), else Web | PORTED | bv:271, bv:158 (probe), apps:367 (page_content), apps:357 (is_markdown_page) | Rust also reloads text/markdown through a proxy scheme (WebKit cannot show it), per README Status (M8) |
| c28-84 | src/renderer/components/PageViews.tsx:110 | Probe failure only logged; content unchanged | PARTIAL | bv:281-287, bv:168 | A probe result that cannot be parsed sets content to Web in Rust (removes a markdown overlay). Electron keeps the previous content |
| c28-85 | src/renderer/components/PageViews.tsx:112 | Listener add/remove on the webview | NA | | Effect plumbing; the Rust channel ends with the view |
| c28-86 | src/renderer/components/PageViews.tsx:124 | `load(target)`: loadURL; a rejection is only logged | PORTED | bv:310 (load) | (v) Behaviour text narrowed. Electron's comment "the page shows it" (the failure) does not hold in Rust: see c28-159 |
| c28-87 | src/renderer/components/PageViews.tsx:131 | Markdown click: only `<a href>` clicks are handled | PORTED | bv:468-475 (markdown_preview_linked link callback) | |
| c28-88 | src/renderer/components/PageViews.tsx:133 | `preventDefault` so links do not navigate the host window | NA | | DOM-only; the native preview never navigates |
| c28-89 | src/renderer/components/PageViews.tsx:134 | Link resolved against the current page; same origin -> load in the tab | PORTED | apps:403 (markdown_link InTab), bv:177 | |
| c28-90 | src/renderer/components/PageViews.tsx:136 | Other origin -> system browser | PORTED | bv:178, bv:93 (open_externally) | |
| c28-91 | src/renderer/components/PageViews.tsx:141 | Web: no overlay; Markdown: rendered preview over the still-mounted webview | PORTED | bv:463-478, bv:150 (sync_visibility hides the native view) | |
| c28-92 | src/renderer/components/PageViews.tsx:153 | Cmd/Ctrl+L focuses the address bar | PORTED | bv:554-560 | |
| c28-93 | src/renderer/components/PageViews.tsx:163 | Toolbar `loading`: progress line under it | PORTED | bv:539-549 | (v) Note added: it starts later than in Electron, see c28-81 |
| c28-94 | src/renderer/components/PageViews.tsx:164 | "Back" ←, disabled without history, goBack | PORTED | bv:516-518 | |
| c28-95 | src/renderer/components/PageViews.tsx:165 | "Forward" →, disabled without history, goForward | PORTED | bv:519-521 | |
| c28-96 | src/renderer/components/PageViews.tsx:166 | While loading: "Stop" ✕ stops the load | PORTED | bv:523, bv:336 (stop) | |
| c28-97 | src/renderer/components/PageViews.tsx:168 | Otherwise "Reload" ⟳ with `reloadIgnoringCache` | PARTIAL | bv:525, bv:328 (reload) | Plain `reload()`, not a cache-bypassing reload |
| c28-98 | src/renderer/components/PageViews.tsx:169 | "Home" ⌂ loads the start page | PORTED | bv:530 | |
| c28-99 | src/renderer/components/PageViews.tsx:170 | Address bar in the toolbar | PORTED | bv:347 (address), bv:483 | |
| c28-100 | src/renderer/components/PageViews.tsx:171 | "Open in external browser" ↗ opens the current URL | PORTED | bv:535-536 | |
| c28-101 | src/renderer/components/PageViews.tsx:174 | `<webview src partition="persist:webapp">`: the page, with storage kept across restarts | PORTED | bv:192 (wry builder, with_url), bv:464 (white background) | No named partition; uses wry's default persistent data store |
| c28-102 | src/renderer/components/PageViews.tsx:192 | `Draft` (not editing / editing text + error) | PORTED | bv:62 (Draft) | Text lives in the InputState |
| c28-103 | src/renderer/components/PageViews.tsx:197 | Shows the page URL unless editing; error only while editing | PORTED | bv:448-452, bv:456-459 | |
| c28-104 | src/renderer/components/PageViews.tsx:202 | Enter: resolveAddress OK -> stop editing, blur, navigate | PORTED | bv:364-371, apps:270 (resolve_address) | |
| c28-105 | src/renderer/components/PageViews.tsx:208 | Enter: resolve error -> keep text, show error | PORTED | bv:372 | |
| c28-106 | src/renderer/components/PageViews.tsx:212 | Escape: drop the draft, blur | PARTIAL | bv:561-567 | (v) Was PORTED. Rust acts only while a draft is open (`Draft::Editing`). After Cmd/Ctrl+L or a click into the bar without typing, Escape leaves the bar focused (the Kit input's own Escape only propagates), while Electron always blurs it. The Rust handler sits on the whole view, so Escape elsewhere in the tab also drops a kept error draft |
| c28-107 | src/renderer/components/PageViews.tsx:221 | `invalid` class (red border) on error | PORTED | bv:493 (#f44336) | |
| c28-108 | src/renderer/components/PageViews.tsx:222 | `aria-label="Address"` | NA | | GPUI has no accessibility tree (app-wide) |
| c28-109 | src/renderer/components/PageViews.tsx:223 | Tooltip: the error, else the text | PORTED | bv:482, bv:505 | |
| c28-110 | src/renderer/components/PageViews.tsx:224 | `spellCheck={false}` | PORTED | bv:483-503 (address field via ui::field) | (v) Ref added. GPUI Kit inputs do not spell-check |
| c28-111 | src/renderer/components/PageViews.tsx:226 | Focus selects all | PORTED | bv:354 | |
| c28-112 | src/renderer/components/PageViews.tsx:227 | Blur drops the draft unless it has an error | PORTED | bv:376-380 | |
| c28-113 | src/renderer/components/PageViews.tsx:228 | Typing starts a draft and clears the error | PORTED | bv:355-363 | |
| c28-114 | src/renderer/components/PageViews.tsx:234 | `PortForwardViewProps` (ssh, connectionId, forwardId, onRestart) | PORTED | av:2193 (port_forward_pane), fm:41 (ForwardMonitor::new) | |
| c28-115 | src/renderer/components/PageViews.tsx:241 | `LoadedForward` { id, info } | PORTED | fm:19 (ForwardState) | |
| c28-116 | src/renderer/components/PageViews.tsx:249 | Subscribe to status before listing, so no change is missed | PORTED | fm:67 (watch gives initial + updates together) | |
| c28-117 | src/renderer/components/PageViews.tsx:250 | Ignore other forwards' statuses | PORTED | fm:67 (one watch per id) | |
| c28-118 | src/renderer/components/PageViews.tsx:252 | Status replaces the info; Connecting = new ssh process -> watch its output again | PORTED | fm:92-101 (on_status) | Rust re-watches when the status changes to Connecting |
| c28-119 | src/renderer/components/PageViews.tsx:254 | Find this tab's forward in the connection's list | PORTED | fm:60-70 (watch), fm:42 (retried when the page changes) | |
| c28-120 | src/renderer/components/PageViews.tsx:257 | List failure -> red error instead of the view (:263) | NA | | IPC failure path; the in-process manager only reports "not found", shown as c28-121 |
| c28-121 | src/renderer/components/PageViews.tsx:266 | No forward: "No port forward yet. It starts once the server is ready." | PORTED | av:2199-2206 | |
| c28-122 | src/renderer/components/PageViews.tsx:270 | Restart offered only for Error / Stopped | PORTED | av:2209 | |
| c28-123 | src/renderer/components/PageViews.tsx:274 | "localhost:L → host:R (status)" line | PORTED | av:2224-2229 | Status strings match (types.rs as_str) |
| c28-124 | src/renderer/components/PageViews.tsx:277 | "Restart" button -> onRestart | PORTED | av:2232-2245, app-rs/crates/treeterm-model/src/browser_page.rs:67 (restart_forward) | |
| c28-125 | src/renderer/components/PageViews.tsx:279 | Error status shows the forward's error | PORTED | av:2210-2213, av:2261 | |
| c28-126 | src/renderer/components/PageViews.tsx:280 | ForwardLog remounts per watch generation | PORTED | fm:105 (watch_output) | |
| c28-127 | src/renderer/components/PageViews.tsx:291 | Output lines appended, no limit | PARTIAL | fm:117-126 | Rust keeps the last 1000 lines (MAX_LINES, fm:16) |
| c28-128 | src/renderer/components/PageViews.tsx:295 | Scrollback put before lines already received | PORTED | fm:115 | |
| c28-129 | src/renderer/components/PageViews.tsx:294 | Unmounted before the watch resolves -> unsubscribe | NA | | Rust task drop cancels |
| c28-130 | src/renderer/components/PageViews.tsx:297 | Watch failure -> "Failed to watch output: <msg>" | PORTED | fm:133-135 | |
| c28-131 | src/renderer/components/PageViews.tsx:300 | Cleanup unsubscribes | NA | | Effect plumbing |
| c28-132 | src/renderer/components/PageViews.tsx:303 | Lines shown in `.ssh-pane-output` | PORTED | av:2247-2256 | |
| c28-159 | src/renderer/components/PageViews.tsx:126 | A load that fails (server down, connection refused, bad host) is shown in the tab by the webview's error page | PARTIAL | bv:216 (with_on_page_load_handler, no failure callback), bv:310 (load) | (v) Added. On macOS, WKWebView draws no error page and wry reports no provisional failure. A failed load from the address bar, Home, Reload or a markdown link leaves the previous page (blank on the first load) with no message, no loading state (c28-81) and the address reset to the old URL. WebKitGTK shows its own error page (Linux not checked) |

## src/renderer/components/ParentReviewCommentsButton.tsx

| # | Electron | Behaviour | Status | Rust parallel | Notes |
|---|---|---|---|---|---|
| c28-133 | src/renderer/components/ParentReviewCommentsButton.tsx:6 | Button among the AI harness's floating buttons (after Address Comments, before PR comments) | PORTED | hp:25 (ParentReviewComments), hp:68-99 (order), av:1930-1934 | Shown on tabs with an analyzer (AI harness) |
| c28-134 | src/renderer/components/ParentReviewCommentsButton.tsx:8 | Follows `isDiffCleanFromParent` (nothing of its own to merge) | PORTED | app-rs/crates/treeterm-core/src/git_status/mod.rs:165 (ParentDiff) | |
| c28-135 | src/renderer/components/ParentReviewCommentsButton.tsx:12 | Hidden unless clean from parent and it has a parent | PORTED | hp:86-87 | |
| c28-136 | src/renderer/components/ParentReviewCommentsButton.tsx:14 | Hidden when the parent's store is not found | PORTED | hp:86 | |
| c28-137 | src/renderer/components/ParentReviewCommentsButton.tsx:16 | `key={parentId}` remount | NA | | React mechanics |
| c28-138 | src/renderer/components/ParentReviewCommentsButton.tsx:22 | Re-renders when the parent's comments change | PORTED | app-rs/crates/treeterm-model/src/session_store.rs:866, av:240 | |
| c28-139 | src/renderer/components/ParentReviewCommentsButton.tsx:27 | Count = parent's comments not addressed | PORTED | hp:63-65, hp:88 | |
| c28-140 | src/renderer/components/ParentReviewCommentsButton.tsx:28 | Hidden when the count is 0 | PORTED | hp:89 | |
| c28-141 | src/renderer/components/ParentReviewCommentsButton.tsx:30 | Click: loading while promptParentReviewComments runs | PORTED | hp:110 (send), av:2406-2411 | `send` is synchronous; a failure shows a notification |
| c28-142 | src/renderer/components/ParentReviewCommentsButton.tsx:33 | Rebases parent paths onto this worktree, pastes the prompt, marks the parent's comments addressed once sent | PORTED | hp:126-150 | |
| c28-143 | src/renderer/components/ParentReviewCommentsButton.tsx:43 | Disabled while loading (no double send) | PORTED | hp:130 | No disabled state; the first click marks the comments addressed, so a second finds none |
| c28-144 | src/renderer/components/ParentReviewCommentsButton.tsx:44 | Tooltip "Prompt AI to address the parent workspace's review comments" | PORTED | hp:46, av:2404 | |
| c28-145 | src/renderer/components/ParentReviewCommentsButton.tsx:46 | Label "Address Parent Comments (N)" | PORTED | hp:36 | |
| c28-146 | src/renderer/components/ParentReviewCommentsButton.tsx:41 | `.review-comments-button` look (floating amber button) | PORTED | av:2390-2403 | |

## src/renderer/components/PdfViewer.tsx

| # | Electron | Behaviour | Status | Rust parallel | Notes |
|---|---|---|---|---|---|
| c28-147 | src/renderer/components/PdfViewer.tsx:4 | `PdfViewerProps` { base64, fileName } | PORTED | bv:600 (AssetPageView::pdf(bytes)) | |
| c28-148 | src/renderer/components/PdfViewer.tsx:12 | Chromium's PDF viewer: page navigation, zoom, print | PARTIAL | bv:600, bv:636-678 (AssetPageView build) | WKWebView draws the PDF (scroll, pinch zoom) without Chromium's toolbar (page box, zoom buttons, print, download). Not checked on Linux/WebKitGTK |
| c28-149 | src/renderer/components/PdfViewer.tsx:13 | Blob URL, not a data: URL (2MB cap) | PORTED | bv:641-656 (custom scheme serves the bytes) | No size cap |
| c28-150 | src/renderer/components/PdfViewer.tsx:20 | `if (!frame) return` | NA | | React ref timing |
| c28-151 | src/renderer/components/PdfViewer.tsx:21 | `base64ToBytes` decode | NA | | IPC encoding; Rust has the raw bytes in-process |
| c28-152 | src/renderer/components/PdfViewer.tsx:21 | Served as `application/pdf` | PORTED | bv:601, bv:650 | |
| c28-153 | src/renderer/components/PdfViewer.tsx:22 | Frame loads the blob URL | PORTED | bv:656 | |
| c28-154 | src/renderer/components/PdfViewer.tsx:23 | Blob released on change / unmount | NA | | Memory management; the Arc drops with the view |
| c28-155 | src/renderer/components/PdfViewer.tsx:24 | New bytes (`[frame, base64]`) reload the viewer | PARTIAL | app-rs/crates/treeterm-app/src/file_browser_view.rs:389-403 (page_for) | The page is keyed by path only and is kept while non-PDF files are shown. Reopening the same PDF after it changed on disk shows the old bytes |
| c28-156 | src/renderer/components/PdfViewer.tsx:26 | `title={fileName}` names the frame for assistive tech | NA | | GPUI has no accessibility tree (app-wide) |
| c28-157 | src/renderer/components/PdfViewer.tsx:26 | `.pdf-viewer` fills the pane, no border | PORTED | bv:689 | |

## Gaps

- c28-18, MISSING (high), src/renderer/components/OpenWorkspaceDialog.tsx:48: opening cannot set `settings.defaultApplicationId`, which in Electron picks the first tab's app, is saved, and is inherited by children. Rust always uses the global default and `WorktreeSettings::default()`: app-rs/crates/treeterm-model/src/session_store.rs:1407,1448,1477
- c28-48, MISSING (medium), src/renderer/components/OpenWorkspaceDialog.tsx:148: no default-application select in the dialog. Nearest: app-rs/crates/treeterm-app/src/tool_views/workspace_settings_view.rs:67
- c28-49, MISSING (medium), src/renderer/components/OpenWorkspaceDialog.tsx:153: "Use Global Default" option. Nearest: app-rs/crates/treeterm-model/src/tool_tabs/workspace_settings.rs:18
- c28-51, MISSING (medium), src/renderer/components/OpenWorkspaceDialog.tsx:155: one option per new-tab-menu app. Nearest: app-rs/crates/treeterm-model/src/tool_tabs/workspace_settings.rs:19
- c28-9, MISSING (low), src/renderer/components/OpenWorkspaceDialog.tsx:22: no default-application choice state in the dialog. Nearest: app-rs/crates/treeterm-model/src/tool_tabs/workspace_settings.rs:17
- c28-12, MISSING (low), src/renderer/components/OpenWorkspaceDialog.tsx:28: the dialog does not list new-tab-menu apps (`default_app_choices` exists but is unused here). app-rs/crates/treeterm-model/src/tool_tabs/workspace_settings.rs:17
- c28-47, MISSING (low), src/renderer/components/OpenWorkspaceDialog.tsx:147: "Default Application" label. Nearest: app-rs/crates/treeterm-app/src/child_dialog.rs:795
- c28-50, MISSING (low), src/renderer/components/OpenWorkspaceDialog.tsx:154: disabled separator option.
- c28-52, MISSING (low), src/renderer/components/OpenWorkspaceDialog.tsx:161: hint "The application to open by default in this workspace. Child worktrees will inherit this setting."
- c28-20, PARTIAL (low), src/renderer/components/OpenWorkspaceDialog.tsx:56: Enter opens only from the folder field. Enter in the remote browser's path field only navigates (Electron also opens). app-rs/crates/treeterm-app/src/open_workspace_dialog.rs:89,167
- c28-32, PARTIAL (low), src/renderer/components/OpenWorkspaceDialog.tsx:93: recent-directory rows have no full-path tooltip, and their path line is truncated. app-rs/crates/treeterm-app/src/open_workspace_dialog.rs:358-383
- c28-80, MISSING (medium), src/renderer/components/PageViews.tsx:113: in-page navigations (hash, pushState) do not update the address, page memory or history, because wry reports only committed/finished full loads. Nearest: app-rs/crates/treeterm-app/src/browser_view.rs:216
- c28-71, PARTIAL (medium), src/renderer/components/PageViews.tsx:87: remembered page on reopen. Browser: `page_url` concatenates the remembered path onto a start page that has a path (`/app/` reopens at `/app/app/`) and drops a trailing slash. Web App tabs have no page memory. app-rs/crates/treeterm-model/src/apps.rs:305, app-rs/crates/treeterm-app/src/app_view.rs:2363
- c28-78, PARTIAL (medium), src/renderer/components/PageViews.tsx:100: Web App tabs never save their page (memory None). app-rs/crates/treeterm-app/src/browser_view.rs:293, app-rs/crates/treeterm-app/src/app_view.rs:2363
- c28-79, PARTIAL (medium), src/renderer/components/PageViews.tsx:101: Back/Forward come from Rust's own stacks, not the webview's history. Page-made history moves do not match, and Back onto a pushState entry leaves `Pending::Back` to mislabel the next load. app-rs/crates/treeterm-app/src/browser_view.rs:250-266,314
- c28-81, PARTIAL (medium), src/renderer/components/PageViews.tsx:103: loading (Stop, progress line) starts only when the page commits (wry `Started` = didCommitNavigation), not when the request starts, so a slow server shows no progress. app-rs/crates/treeterm-app/src/browser_view.rs:250
- c28-159, PARTIAL (medium), src/renderer/components/PageViews.tsx:126: a failed load (server down, refused) shows nothing on macOS: no error page, no message, the previous or blank page stays. app-rs/crates/treeterm-app/src/browser_view.rs:216,310
- c28-82, PARTIAL (low), src/renderer/components/PageViews.tsx:104: a load that fails after committing never sends `Finished` on macOS, so Stop and the progress line stay up. app-rs/crates/treeterm-app/src/browser_view.rs:268
- c28-84, PARTIAL (low), src/renderer/components/PageViews.tsx:110: an unreadable probe result resets content to Web instead of keeping the previous content. app-rs/crates/treeterm-app/src/browser_view.rs:281-287
- c28-97, PARTIAL (low), src/renderer/components/PageViews.tsx:168: Reload is a plain reload, not `reloadIgnoringCache`. app-rs/crates/treeterm-app/src/browser_view.rs:328
- c28-106, PARTIAL (low), src/renderer/components/PageViews.tsx:212: Escape blurs the address bar only while a draft is open. Without edits the bar keeps focus. app-rs/crates/treeterm-app/src/browser_view.rs:561-567
- c28-127, PARTIAL (low), src/renderer/components/PageViews.tsx:291: the port-forward ssh log keeps only the last 1000 lines (Electron has no limit). app-rs/crates/treeterm-model/src/forward_monitor.rs:16
- c28-148, PARTIAL (medium), src/renderer/components/PdfViewer.tsx:12: the PDF shows in WKWebView without Chromium's viewer toolbar (page navigation, zoom buttons, print). Linux not checked. app-rs/crates/treeterm-app/src/browser_view.rs:600
- c28-155, PARTIAL (medium), src/renderer/components/PdfViewer.tsx:24: the PDF page is cached by path, so reopening a changed PDF shows stale bytes. app-rs/crates/treeterm-app/src/file_browser_view.rs:389-403
