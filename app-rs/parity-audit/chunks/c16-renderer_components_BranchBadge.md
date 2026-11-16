# c16-renderer_components_BranchBadge

Verified: +4 rows, 16 rows changed

Paths in the Rust column are under `app-rs/crates/`. `app_view.rs` = `treeterm-app/src/app_view.rs`.

## src/renderer/components/BranchBadge.tsx

| # | Electron | Behaviour | Status | Rust parallel | Notes |
|---|---|---|---|---|---|
| c16-1 | src/renderer/components/BranchBadge.tsx:8 | `BRANCH_BADGE_MENU_ID = 'branch-badge'`: id of the badge's context menu in the shared context-menu store | NA | treeterm-app/src/app_view.rs:1611 (`.id("branch-badge")`) | Store-keyed menu plumbing; the Kit's `context_menu` is attached to the element itself. |
| c16-2 | src/renderer/components/BranchBadge.tsx:10 | `BranchBadgeProps` type: branch, worktreePath, workspace, canAutoRename, clipboard | PORTED | treeterm-app/src/app_view.rs:1597-1606 (header `actions`, branch = `file.git_branch`, path = `workspace().path`, `can_rename`) | Inlined into the workspace header instead of a component. Clipboard is GPUI's `cx.write_to_clipboard`. |
| c16-3 | src/renderer/components/BranchBadge.tsx:14 | Only child worktrees may auto-rename (caller passes `isWorktree && !!parentId`) | PORTED | treeterm-app/src/app_view.rs:1606 (`can_rename = file.is_worktree && !root`), :1525 (`root` = parent_id none/empty) | |
| c16-4 | src/renderer/components/BranchBadge.tsx:21 | Context-menu store selectors (open, close, activeMenuId, position) | NA | treeterm-app/src/app_view.rs:1615 (`ContextMenuExt::context_menu`) | React/zustand menu plumbing. |
| c16-5 | src/renderer/components/BranchBadge.tsx:25 | `copied` state | PORTED | treeterm-app/src/app_view.rs:101 (`Connected::branch_copied`) | Rust keys it by branch name, so a renamed branch never shows "Copied!". |
| c16-6 | src/renderer/components/BranchBadge.tsx:26 | `renaming` state: an auto-rename is in progress | MISSING | treeterm-model/src/workspace_store.rs:376 (`refresh_labels`) | (v) Confirmed MISSING. Searched `renaming`/`Renaming`, `refreshing`/`Refreshing`, `in_flight`/`pending`/`busy`, `last_refresh` readers, `Spinner` uses in app_view.rs, and the analyzer's public API: the in-flight `titles`/`renames` maps (treeterm-core/src/analyzer/mod.rs) are private and `refresh_labels` returns as soon as the request is queued. Nothing tracks a manual refresh in progress. |
| c16-7 | src/renderer/components/BranchBadge.tsx:29 | Auto rename closes the context menu first | PORTED | treeterm-app/src/app_view.rs:1622 | The Kit popup menu closes on item click. |
| c16-8 | src/renderer/components/BranchBadge.tsx:30 | `setRenaming(true)` before the request | MISSING | none | (v) Confirmed (see c16-6). Internal; what the user sees is c16-13 and c16-23. |
| c16-9 | src/renderer/components/BranchBadge.tsx:32 | Calls `workspace.refreshBranchName()` (LLM picks a branch name from the AI-harness terminal, renames, marks user-defined) | PARTIAL | treeterm-model/src/workspace_store.rs:376 (`refresh_labels(true)`), treeterm-model/src/analyzer.rs:157 (`refresh_branch_name`), treeterm-core/src/analyzer/mod.rs:634, :713 (rename), :732 (`branchIsUserDefined`) | (v) Was PORTED. The rename itself matches, but the harness choice differs: Electron's `findAiHarnessAnalyzer` (createWorkspaceStore.ts:273) uses the active tab first when it is an AI harness, then the others in `appStates` order. Rust takes the first AI-harness tab in `app_states` order and ignores the active tab, so with two harness tabs and the second active, Rust names the branch from the other terminal. The earlier note ("same lookup") was wrong. |
| c16-10 | src/renderer/components/BranchBadge.tsx:33 | `TitleRefreshStatus.Failure` result -> `alert(result.error)` | PARTIAL | treeterm-app/src/app_view.rs:1622-1625 (`push_notification`), treeterm-model/src/analyzer.rs:272 (`last_refresh`) | (v) Note rewritten. Electron shows every failure in a blocking `alert()`. Rust shows only the synchronous no-harness error, as a Kit toast, and with different text: "No AI Harness tab in this workspace" vs Electron's "No AI Harness tab open — the labeller has no terminal to read". These failures end in `Effect::RefreshFinished` and are stored in `AnalyzerModel::last_refresh`: "Title model not configured", "Terminal is empty — nothing for the labeller to read", LLM error or "LLM returned no title", an invalid branch name, git rename failure. Nothing in treeterm-app reads that field (grep `last_refresh`), so all of them are silent: clicking does nothing visible. |
| c16-11 | src/renderer/components/BranchBadge.tsx:36 | `catch`: a thrown error is logged and `alert`ed (message or `String(err)`) | PARTIAL | treeterm-model/src/analyzer.rs:272 (`last_refresh`) | (v) Was NA, but the user sees this branch. Electron's throws here are IPC or git failures, which Rust returns as `Err` results (`on_branch_renamed`, `on_title`). They land in `last_refresh` and are never shown (same gap as c16-10). |
| c16-12 | src/renderer/components/BranchBadge.tsx:41 | `finally`: clear `renaming` | MISSING | none | (v) Confirmed (see c16-6). Internal; the effect is c16-13 and c16-23. |
| c16-13 | src/renderer/components/BranchBadge.tsx:46 | While renaming the badge shows a spinner + "Renaming..." | MISSING | treeterm-app/src/app_view.rs:1610 | (v) Confirmed. The pill label is only `"Copied!"` or the branch (app_view.rs:1610). Searched "Renaming", `Spinner`/`spinner(` in app_view.rs (none near the badge), and the analyzer status bar (`last_refresh` is documented "for the status bar" but unused). During the LLM call (seconds) nothing shows a rename is under way. |
| c16-14 | src/renderer/components/BranchBadge.tsx:48 | Label "Copied!" while copied, else the branch name | PORTED | treeterm-app/src/app_view.rs:1610 | |
| c16-15 | src/renderer/components/BranchBadge.tsx:53 | `.copied` class: green badge while copied | PORTED | treeterm-app/src/app_view.rs:1598-1603 | fg #2ea043 / bg rgba(0,180,80,.2) vs accent normally. |
| c16-16 | src/renderer/components/BranchBadge.tsx:55 | Click copies the branch name to the clipboard | PORTED | treeterm-app/src/app_view.rs:1614, :889 (`copy_branch`) | |
| c16-17 | src/renderer/components/BranchBadge.tsx:56 | `setCopied(true)`; reset after 1500 ms | PORTED | treeterm-app/src/app_view.rs:891-897 (1500 ms timer) | Rust restarts the 1.5 s on each click (Electron's first timer clears it early on a double click). |
| c16-18 | src/renderer/components/BranchBadge.tsx:59 | Right-click: preventDefault/stopPropagation, open menu at the pointer | PORTED | treeterm-app/src/app_view.rs:1615 (`.context_menu`) | |
| c16-19 | src/renderer/components/BranchBadge.tsx:64 | Tooltip "Copy branch name" | PORTED | treeterm-app/src/app_view.rs:1613 | Kit tooltip, not native (README Appearance). |
| c16-20 | src/renderer/components/BranchBadge.tsx:66 | `<ContextMenu>` wrapper keyed by menu id/position | NA | treeterm-app/src/app_view.rs:1615 | Menu rendering library glue. |
| c16-21 | src/renderer/components/BranchBadge.tsx:67 | "Copy worktree path" item: close menu, copy the worktree path | PORTED | treeterm-app/src/app_view.rs:1617-1619 | |
| c16-22 | src/renderer/components/BranchBadge.tsx:73 | Auto-rename item only when `canAutoRename` | PORTED | treeterm-app/src/app_view.rs:1620 (`if can_rename`) | |
| c16-23 | src/renderer/components/BranchBadge.tsx:73 | Auto-rename item hidden while a rename runs (`!renaming`) | MISSING | treeterm-app/src/app_view.rs:1620 | (v) Confirmed. The item is always offered. Each click queues another title request (`request_title` has no in-flight check), and each completion runs `branch_rename` from the branch current at that moment. So a double click costs two LLM calls and can rename the branch twice. |
| c16-24 | src/renderer/components/BranchBadge.tsx:75 | Item label "Auto rename branch" | PARTIAL | treeterm-app/src/app_view.rs:1622 | Rust label is "Auto-rename branch" (hyphen). |
| c16-136 | src/renderer/components/BranchBadge.tsx:32 | Success: the badge then shows the new branch (the store updates `gitBranch`); no message | PORTED | treeterm-model/src/analyzer.rs:260 (`AnalyzerEvent::BranchRenamed`), treeterm-model/src/workspace_store.rs:995, :1530 (`set_git_branch`) | (v) Added. Success is silent in both; the pill re-renders with the new name. |

## src/renderer/components/BrowserPane.tsx

| # | Electron | Behaviour | Status | Rust parallel | Notes |
|---|---|---|---|---|---|
| c16-25 | src/renderer/components/BrowserPane.tsx:9 | `BrowserPaneProps`: workspace, tabId, ssh, openExternal | PORTED | treeterm-app/src/app_view.rs:2271 (`browser_tab`) | ssh -> `PageHost::Remote(ForwardMonitor)`. |
| c16-26 | src/renderer/components/BrowserPane.tsx:16 | `BrowserSubTab` enum: Browser / PortForward | PORTED | treeterm-app/src/app_view.rs:2062 (`PageSubTab`) | Shared with Web App's Shell sub-tab. |
| c16-27 | src/renderer/components/BrowserPane.tsx:22 | Sub-tab defaults to Browser | PORTED | treeterm-app/src/app_view.rs:2282 (`unwrap_or(PageSubTab::Browser)`) | Rust keeps the choice per (workspace, tab) in `page_subtabs` for the window's life; Electron resets on remount. |
| c16-28 | src/renderer/components/BrowserPane.tsx:23 | `isRemote` from the workspace | PORTED | treeterm-model/src/workspace_store.rs:349 (`TabContent::Browser { forward }`), app_view.rs:2297-2301 | `PageHost::Local` / `Remote`. |
| c16-29 | src/renderer/components/BrowserPane.tsx:24 | `connectionId` for the Port Forward view | PORTED | treeterm-app/src/app_view.rs:2302 (`PageHost::Remote(monitor)`) | The monitor is bound to the connection. |
| c16-30 | src/renderer/components/BrowserPane.tsx:26 | Tab's `PageRef` (created at workspace load, before render) | PORTED | treeterm-model/src/workspace_store.rs:761 (`BrowserPageModel::new`), treeterm-model/src/browser_page.rs:11 | |
| c16-31 | src/renderer/components/BrowserPane.tsx:28 | Subscribe to runtime transitions (re-render on each) | PORTED | treeterm-model/src/browser_page.rs:57 (`phase`), app_view.rs:2287 | GPUI notify. |
| c16-32 | src/renderer/components/BrowserPane.tsx:31 | `BrowserView` renders the runtime phase (page / waiting / error) | PORTED | treeterm-app/src/app_view.rs:2287-2296 (Ready -> `page_view`, Forwarding -> "Starting port forward for port N…", Error -> `page_error`) | Phase rendering details belong to PageViews' chunk. |
| c16-33 | src/renderer/components/BrowserPane.tsx:33 | `onRetry = ref.retry` | PORTED | treeterm-app/src/app_view.rs:2294, treeterm-model/src/browser_page.rs:62 (`retry`) | |
| c16-34 | src/renderer/components/BrowserPane.tsx:34 | Remote: "View port forward" link switching to the Port Forward sub-tab; local: no links | PORTED | treeterm-app/src/app_view.rs:2283-2286, :2115 (`page_link`) | |
| c16-35 | src/renderer/components/BrowserPane.tsx:35 | `openExternal`: open the page in the system browser | PORTED | treeterm-app/src/browser_view.rs:535 ("Open in external browser", `cx.open_url`) | |
| c16-36 | src/renderer/components/BrowserPane.tsx:36 | `memory={ref}`: tab reopens on the page it last showed | PORTED | treeterm-app/src/app_view.rs:2182 (`remember_pages_in`) | |
| c16-37 | src/renderer/components/BrowserPane.tsx:40 | Local session: only the page, no sub-tab bar | PORTED | treeterm-app/src/app_view.rs:2300 (`PageHost::Local => return browser`) | |
| c16-38 | src/renderer/components/BrowserPane.tsx:44 | Sub-tab labels "Browser", "Port Forward" | PORTED | treeterm-app/src/app_view.rs:2069 (`PageSubTab::label`), :2303 | |
| c16-39 | src/renderer/components/BrowserPane.tsx:48 | Both sub-tabs stay mounted; switching never reloads the webview | PORTED | treeterm-app/src/app_view.rs:2009-2017 (`set_shown` only on Browser), :2175 (`browser_views` cache) | Native webview hidden, not destroyed. |
| c16-40 | src/renderer/components/BrowserPane.tsx:54 | Sub-tab buttons: click selects, active one styled | PORTED | treeterm-app/src/app_view.rs:2079 (`page_subtab_bar`), :2104 (`show_page_subtab`) | |
| c16-41 | src/renderer/components/BrowserPane.tsx:66 | `PortForwardView` for forward `webapp-<tabId>` | PORTED | treeterm-app/src/app_view.rs:2193 (`port_forward_pane`), treeterm-model/src/web_app.rs:44 (`forward_id_for_tab`) | Same id format (test web_app.rs:316). |
| c16-42 | src/renderer/components/BrowserPane.tsx:70 | Restart -> `ref.restartForward()` | PORTED | treeterm-app/src/app_view.rs:2307, treeterm-model/src/browser_page.rs:67 (`restart_forward`) | |

## src/renderer/components/BrowserSettingsSection.tsx

| # | Electron | Behaviour | Status | Rust parallel | Notes |
|---|---|---|---|---|---|
| c16-43 | src/renderer/components/BrowserSettingsSection.tsx:4 | Props: `instances: BrowserInstance[]`, `onChange` | PORTED | treeterm-app/src/settings_view/instance_list.rs:61 (`InstanceList<BrowserInstance>`), treeterm-core/src/settings/mod.rs:291 (`BrowserInstance`) | Rust also keeps unknown keys (`extra`). |
| c16-44 | src/renderer/components/BrowserSettingsSection.tsx:9 | `urlProblem`: valid local URL -> no message | PORTED | treeterm-app/src/settings_view/instance.rs:584 (`BrowserInstance::problem` -> None) | |
| c16-45 | src/renderer/components/BrowserSettingsSection.tsx:13 | Invalid URL -> message "URL must be http(s)://localhost or 127.0.0.1: <url>" | PORTED | treeterm-app/src/settings_view/instance.rs:586 | Same text (test instance.rs:662). |
| c16-46 | src/renderer/components/BrowserSettingsSection.tsx:11 | Validation rule (`parseLocalUrl`): parseable URL, protocol http:/https:, host localhost or 127.0.0.1 | PARTIAL | treeterm-model/src/apps.rs:203 (`is_allowed_webview_url`) | (v) Note extended. Edge cases differ because Electron parses with WHATWG `URL` and Rust splits the string. Electron accepts what Rust rejects: an upper-case scheme (`HTTP://localhost/`, scheme compared case-sensitively in Rust), leading/trailing spaces (stripped by WHATWG), shorthand IPv4 for loopback (`http://127.1/`, `http://0x7f.0.0.1/`, normalised to 127.0.0.1), and `http:localhost:8080` without slashes. Rust accepts what Electron rejects: unparsable ports (`:99999`, `:abc`), since Rust never parses the port. |
| c16-47 | src/renderer/components/BrowserSettingsSection.tsx:19 | `updateAt`: patch one instance, keep the others | PORTED | treeterm-app/src/settings_view/instance_list.rs:50 (`Row::current`), :124 (`toggle_mut`) | |
| c16-48 | src/renderer/components/BrowserSettingsSection.tsx:25 | Intro hint "Open a page that is already being served…" | PORTED | treeterm-app/src/settings_view/instance.rs:504 (`INTRO`) | |
| c16-49 | src/renderer/components/BrowserSettingsSection.tsx:30 | One `.application-item` per instance | PORTED | treeterm-app/src/settings_view/instance_list.rs:100 | |
| c16-50 | src/renderer/components/BrowserSettingsSection.tsx:35 | Icon input, `maxLength={2}`, edits `icon` | PARTIAL | treeterm-app/src/settings_view/instance.rs:47 (`icon_fits`, UTF-16 units), instance_list.rs:29 (`InputState::validate`) | (v) Was PORTED. Same 2-UTF-16-unit cap, but the Kit's `validate` checks the whole resulting text and rejects the edit (gpui-base `is_valid_input`). Pasting 3+ units (e.g. "abc") leaves the field unchanged in Rust; HTML `maxLength` truncates it to the first 2 ("ab"). |
| c16-51 | src/renderer/components/BrowserSettingsSection.tsx:44 | Name input, placeholder "Name (e.g., Grafana)" | PORTED | treeterm-app/src/settings_view/instance.rs:554 | |
| c16-52 | src/renderer/components/BrowserSettingsSection.tsx:51 | URL input, placeholder "URL (e.g., http://localhost:8080/)" | PORTED | treeterm-app/src/settings_view/instance.rs:555 | |
| c16-53 | src/renderer/components/BrowserSettingsSection.tsx:58 | Problem shown under the fields as you type (`.webapp-error`) | PORTED | treeterm-app/src/settings_view/instance_list.rs:106 (`widgets::error_text`), :41 (re-render on Change) | |
| c16-54 | src/renderer/components/BrowserSettingsSection.tsx:58 | The problem is a warning only: Save still writes the invalid URL | PARTIAL | treeterm-app/src/settings_view/form.rs:281, :287 (`check`) | Rust refuses Save with "<name>: URL must be …" in the Browsers section (test form.rs:678). Not listed as deliberate in README. |
| c16-55 | src/renderer/components/BrowserSettingsSection.tsx:60 | "Default" checkbox toggles `isDefault` | PORTED | treeterm-app/src/settings_view/instance.rs:571 (`toggle_label` "Default"), instance_list.rs:84 (beside the fields) | |
| c16-56 | src/renderer/components/BrowserSettingsSection.tsx:68 | "Delete" removes the instance | PORTED | treeterm-app/src/settings_view/instance_list.rs:141-161 | No confirmation in either. |
| c16-57 | src/renderer/components/BrowserSettingsSection.tsx:78 | "+ Add Browser" appends an instance | PORTED | treeterm-app/src/settings_view/instance.rs:506 (`ADD_LABEL`), instance_list.rs:75 (`add`), :201 | |
| c16-58 | src/renderer/components/BrowserSettingsSection.tsx:84 | New id `browser-<Date.now()>` | PORTED | treeterm-app/src/settings_view/instance.rs:503 (`ID_PREFIX`), form.rs:546 (`new_id`) | Rust also skips an id already in use. |
| c16-59 | src/renderer/components/BrowserSettingsSection.tsx:85 | Defaults: "New Browser", icon 🧭, `http://localhost:8080/`, not default | PORTED | treeterm-app/src/settings_view/instance.rs:511 (`added`) | |
| c16-60 | src/renderer/components/BrowserSettingsSection.tsx:95 | Outro hint "Default browsers open automatically in new workspaces." | PORTED | treeterm-app/src/settings_view/instance.rs:507 (`OUTRO`) | |
| c16-137 | src/renderer/components/BrowserSettingsSection.tsx:14 | `catch` fallback: a non-`Error` throw is shown as `String(err)` | NA | treeterm-app/src/settings_view/instance.rs:584 (`problem` returns a `String`) | (v) Added. `parseLocalUrl` only throws `Error`, so the branch never runs; Rust's check returns a message directly. |

## src/renderer/components/Chat.tsx

| # | Electron | Behaviour | Status | Rust parallel | Notes |
|---|---|---|---|---|---|
| c16-61 | src/renderer/components/Chat.tsx:11 | Module-level LLM client (`createLlmClient`) | PORTED | treeterm-model/src/workspace_store.rs:872 (`self.llm` -> `ChatModel::new`), treeterm-model/src/tool_tabs/chat.rs:48 | |
| c16-62 | src/renderer/components/Chat.tsx:13 | Chat tab component (tab, workspace, isVisible) | PORTED | treeterm-model/src/tool_tabs/chat.rs:46 (`ChatModel`), treeterm-app/src/tool_views/chat_view.rs:14 (`ChatView`) | |
| c16-63 | src/renderer/components/Chat.tsx:14 | LLM endpoint settings read live from the settings store | PORTED | treeterm-model/src/tool_tabs/chat.rs:94 (`llm_settings`) | |
| c16-64 | src/renderer/components/Chat.tsx:16 | Messages start from the tab's saved `ChatState.messages` | PORTED | treeterm-model/src/tool_tabs/chat.rs:58-65 | Rust also shows "The saved conversation could not be read: …" on malformed state. |
| c16-65 | src/renderer/components/Chat.tsx:17 | Input text state | PORTED | treeterm-app/src/tool_views/chat_view.rs:27 (`TextareaState`) | |
| c16-66 | src/renderer/components/Chat.tsx:18 | `isStreaming` state | PORTED | treeterm-model/src/tool_tabs/chat.rs:73 (`is_streaming`) | |
| c16-67 | src/renderer/components/Chat.tsx:19 | `error` state | PORTED | treeterm-model/src/tool_tabs/chat.rs:54, :80 | |
| c16-68 | src/renderer/components/Chat.tsx:20 | Reasoning defaults to Off | PORTED | treeterm-model/src/tool_tabs/chat.rs:66 | Neither persists it. |
| c16-69 | src/renderer/components/Chat.tsx:21 | `activeRequestId`: only the current request's events count | PORTED | treeterm-model/src/tool_tabs/chat.rs:40-44, :145 | One task per request; cancel drops it, so stale deltas cannot arrive. |
| c16-70 | src/renderer/components/Chat.tsx:25 | Persist messages to tab state on every change | PORTED | treeterm-model/src/tool_tabs/chat.rs:98 (`persist`), :172, :183; workspace_store.rs:875 (`merge_tab_state`) | |
| c16-71 | src/renderer/components/Chat.tsx:32 | onDelta: ignore other request ids | PORTED | treeterm-model/src/tool_tabs/chat.rs:145-163 | Structural (see c16-69). |
| c16-72 | src/renderer/components/Chat.tsx:36 | Delta appended to the last message when it is the assistant's | PORTED | treeterm-model/src/tool_tabs/chat.rs:178-180 (`append`) | |
| c16-73 | src/renderer/components/Chat.tsx:39 | Delta dropped when the last message is not the assistant's | PORTED | treeterm-model/src/tool_tabs/chat.rs:181 | |
| c16-74 | src/renderer/components/Chat.tsx:43 | onDone: streaming off | PORTED | treeterm-model/src/tool_tabs/chat.rs:164-169 | |
| c16-75 | src/renderer/components/Chat.tsx:49 | onError: show the error, streaming off | PORTED | treeterm-model/src/tool_tabs/chat.rs:152-157 | |
| c16-76 | src/renderer/components/Chat.tsx:56 | Unsubscribe LLM events on unmount | NA | treeterm-model/src/tool_tabs/chat.rs:43 | Task dropped with the model. |
| c16-77 | src/renderer/components/Chat.tsx:65 | Send ignored for blank (trimmed) input or while streaming | PORTED | treeterm-model/src/tool_tabs/chat.rs:105-107 | |
| c16-78 | src/renderer/components/Chat.tsx:67 | No API key -> "LLM API key not configured. Go to Settings > LLM to set it up." | PORTED | treeterm-model/src/tool_tabs/chat.rs:30, :112-116 | Rust adds "No LLM client is available in this session" (:117). |
| c16-79 | src/renderer/components/Chat.tsx:72 | Clear the error on send | PORTED | treeterm-model/src/tool_tabs/chat.rs:122 | |
| c16-80 | src/renderer/components/Chat.tsx:73 | User message (trimmed) + empty assistant placeholder, UUID ids | PORTED | treeterm-model/src/tool_tabs/chat.rs:123-124 | |
| c16-81 | src/renderer/components/Chat.tsx:85 | Append both, clear the input, start streaming | PORTED | treeterm-model/src/tool_tabs/chat.rs:171-174, treeterm-app/src/tool_views/chat_view.rs:52-54 (`clear_input`) | |
| c16-82 | src/renderer/components/Chat.tsx:89 | Fresh request id per send | PORTED | treeterm-model/src/tool_tabs/chat.rs:144-171 | One stream/task per send. |
| c16-83 | src/renderer/components/Chat.tsx:93 | API history = messages with non-empty content (skip the empty reply) | PORTED | treeterm-model/src/tool_tabs/chat.rs:126-137 | |
| c16-84 | src/renderer/components/Chat.tsx:98 | Send with baseUrl, apiKey, model, reasoning | PORTED | treeterm-model/src/tool_tabs/chat.rs:138-144 (`ChatSettings`) | |
| c16-85 | src/renderer/components/Chat.tsx:104 | Send failure -> error (message, else "Failed to send message"), streaming off | PORTED | treeterm-model/src/tool_tabs/chat.rs:152-156 | Rust errors are typed, so the fallback text has no case. |
| c16-86 | src/renderer/components/Chat.tsx:111 | Cancel: stop the request if one is active, streaming off, partial reply kept | PORTED | treeterm-model/src/tool_tabs/chat.rs:188 (`cancel`) | Dropping the task cancels the request. |
| c16-87 | src/renderer/components/Chat.tsx:121 | Enter sends, Shift+Enter inserts a newline | PORTED | treeterm-app/src/tool_views/chat_view.rs:30, :34 | |
| c16-88 | src/renderer/components/Chat.tsx:129 | Hidden tab renders nothing | NA | treeterm-app/src/tool_views/mod.rs:43 | React render guard; only the active tab's view is rendered. |
| c16-89 | src/renderer/components/Chat.tsx:134 | Empty state "Start a conversation" + "Using {model} at {baseUrl}" | PORTED | treeterm-app/src/tool_views/chat_view.rs:69, :83-94 | |
| c16-90 | src/renderer/components/Chat.tsx:144 | Role label You / Assistant (upper-cased by CSS) | PORTED | treeterm-app/src/tool_views/chat_view.rs:121 ("YOU" / "ASSISTANT") | |
| c16-91 | src/renderer/components/Chat.tsx:147 | Assistant reply with content rendered as GFM markdown | PORTED | treeterm-app/src/tool_views/chat_view.rs:100 (`TextView::markdown`, Kit parses with `ParseOptions::gfm()`) | |
| c16-92 | src/renderer/components/Chat.tsx:150 | Empty reply while streaming -> "Thinking..." | PORTED | treeterm-app/src/tool_views/chat_view.rs:104-106 | |
| c16-93 | src/renderer/components/Chat.tsx:150 | Empty reply not streaming -> nothing | PORTED | treeterm-app/src/tool_views/chat_view.rs:107 | |
| c16-94 | src/renderer/components/Chat.tsx:153 | User message as plain text | PARTIAL | treeterm-app/src/tool_views/chat_view.rs:108 | (v) Was PORTED. Electron puts it in a `<p>` with normal white-space, so line breaks from Shift+Enter and runs of spaces collapse onto one line. Rust's text element keeps the line breaks. Rust's user text is also not selectable (plain `div`), while Electron's can be selected and copied (see c16-139). |
| c16-95 | src/renderer/components/Chat.tsx:158 | Error banner under the messages | PORTED | treeterm-app/src/tool_views/chat_view.rs:136-148 | |
| c16-96 | src/renderer/components/Chat.tsx:206 | `ScrollAnchor`: smooth-scroll to the bottom on every render | PARTIAL | treeterm-app/src/tool_views/chat_view.rs:70-74 (`scroll_to_bottom`) | (v) Note extended. Rust scrolls, without animation, only when the total message text length changes. Electron also scrolls on any other re-render (typing, an error appearing, reasoning change) and whenever the hidden tab is shown again. In a long conversation, an error such as the missing-API-key message is added below the visible area and Rust does not scroll to it, so Send looks like it did nothing. |
| c16-97 | src/renderer/components/Chat.tsx:163 | "Reasoning" select: Off / Low / Medium / High | PORTED | treeterm-model/src/tool_tabs/chat.rs:195 (`REASONING_CHOICES`), treeterm-app/src/tool_views/chat_view.rs:150-169, :224 | Kit menu, not a native popup (README Appearance). |
| c16-98 | src/renderer/components/Chat.tsx:167 | Choosing an effort sets it | PORTED | treeterm-model/src/tool_tabs/chat.rs:88 (`set_reasoning`) | |
| c16-99 | src/renderer/components/Chat.tsx:168 | Select disabled while streaming | PORTED | treeterm-app/src/tool_views/chat_view.rs:152-155 | |
| c16-100 | src/renderer/components/Chat.tsx:177 | Textarea: 3 rows, placeholder "Type a message... (Enter to send, Shift+Enter for newline)" | PORTED | treeterm-app/src/tool_views/chat_view.rs:28-31 | |
| c16-101 | src/renderer/components/Chat.tsx:183 | Textarea disabled while streaming | PORTED | treeterm-app/src/tool_views/chat_view.rs:231-232 | |
| c16-102 | src/renderer/components/Chat.tsx:187 | While streaming: "Cancel" button | PORTED | treeterm-app/src/tool_views/chat_view.rs:183-188 | |
| c16-103 | src/renderer/components/Chat.tsx:192 | Otherwise "Send", disabled while the input is blank | PORTED | treeterm-app/src/tool_views/chat_view.rs:170, :189-194 | |
| c16-138 | src/renderer/components/Chat.tsx:143 | Message bubbles (`.chat-message-user/-assistant`): user right-aligned on the accent with white text, assistant left-aligned on bg-secondary, max 80% wide, 8px 12px padding, 8px radius | PORTED | treeterm-app/src/tool_views/chat_view.rs:110-133 | (v) Added. |
| c16-139 | src/renderer/components/Chat.tsx:145 | Message and error text can be selected and copied (DOM default; only `.chat-reasoning-label` is `user-select: none`) | PARTIAL | treeterm-app/src/tool_views/chat_view.rs:100-101 (`TextView::markdown(..).selectable(true)`), :108, :136-148 | (v) Added. Only assistant replies are selectable in Rust. User messages and the error banner are plain `div` text and cannot be selected or copied. |

## src/renderer/components/CloseConfirmDialog.tsx

| # | Electron | Behaviour | Status | Rust parallel | Notes |
|---|---|---|---|---|---|
| c16-104 | src/renderer/components/CloseConfirmDialog.tsx:4 | Props: unmerged workspaces, onConfirm, onCancel | PORTED | treeterm-app/src/active_processes_view.rs:205 (`confirm_close`), treeterm-model/src/session_store.rs:518 (`unmerged_sub_workspaces`), treeterm-app/src/app_view.rs:823 (`request_close`) | |
| c16-105 | src/renderer/components/CloseConfirmDialog.tsx:16 | Escape cancels | PORTED | app-rs/vendor/gpui-component/src/dialog/dialog.rs:226, :737 (`keyboard` default true) | (v) Note added. Electron's `onKeyDown` sits on the unfocusable overlay, so Escape only works once focus is inside the dialog (e.g. after Tab). Rust's Kit dialog closes on Escape straight away. Same intent, fewer failure cases. |
| c16-106 | src/renderer/components/CloseConfirmDialog.tsx:22 | Click on the overlay cancels | PORTED | app-rs/vendor/gpui-component/src/dialog/dialog.rs:229, :738 (`overlay_closable` default true) | |
| c16-107 | src/renderer/components/CloseConfirmDialog.tsx:23 | Clicks inside the dialog do not cancel | PORTED | app-rs/vendor/gpui-component/src/dialog/dialog.rs:738 | Kit backdrop only. |
| c16-108 | src/renderer/components/CloseConfirmDialog.tsx:25 | Title "Unmerged Workspaces" | PORTED | treeterm-app/src/active_processes_view.rs:270 | |
| c16-109 | src/renderer/components/CloseConfirmDialog.tsx:26 | "x" close button cancels | PORTED | treeterm-app/src/ui.rs:267-299 (`dialog_header`, `close_dialog`) | |
| c16-110 | src/renderer/components/CloseConfirmDialog.tsx:32 | "The following sub-workspaces have not been merged:" | PORTED | treeterm-app/src/active_processes_view.rs:248 | |
| c16-111 | src/renderer/components/CloseConfirmDialog.tsx:37 | One list item per workspace with its name | PORTED | treeterm-app/src/active_processes_view.rs:218-227 | List capped at 200px and scrolls, per CSS. |
| c16-112 | src/renderer/components/CloseConfirmDialog.tsx:40 | "(branch)" only when the workspace has a branch | PARTIAL | treeterm-app/src/active_processes_view.rs:228 (`when_some`) | (v) Was PORTED. Electron tests truthiness, so an empty `gitBranch` hides the branch; Rust tests `Option`, so `Some("")` shows "name ()". This happens in practice: Electron writes `gitBranch: result.branch ?? ''` for child worktrees (createSessionStore.ts:1047), e.g. detached ones. |
| c16-113 | src/renderer/components/CloseConfirmDialog.tsx:47 | Note "Closing the application will not delete these workspaces, but their work remains unmerged." | PORTED | treeterm-app/src/active_processes_view.rs:257 | |
| c16-114 | src/renderer/components/CloseConfirmDialog.tsx:54 | "Cancel" button | PORTED | treeterm-app/src/active_processes_view.rs:261-263 | |
| c16-115 | src/renderer/components/CloseConfirmDialog.tsx:57 | "Close Anyway" confirms the close | PORTED | treeterm-app/src/active_processes_view.rs:265-266 (`cx.quit()`) | |

## src/renderer/components/CommentDisplay.tsx

| # | Electron | Behaviour | Status | Rust parallel | Notes |
|---|---|---|---|---|---|
| c16-116 | src/renderer/components/CommentDisplay.tsx:4 | Props: comment, onDelete, `hideLineRef?` | PORTED | treeterm-app/src/comment_ui.rs:67 (`comment_display`), :30 (`Placement::Diff` = hideLineRef, `Placement::Viewer` = line ref) | `isOutdated` -> treeterm-core/src/review/comments.rs:55 (`Freshness`). |
| c16-117 | src/renderer/components/CommentDisplay.tsx:11 | Header shown when the line ref is shown or the comment is outdated | PORTED | treeterm-app/src/comment_ui.rs:75 | |
| c16-118 | src/renderer/components/CommentDisplay.tsx:13 | Outdated card: amber border, tinted background | PORTED | treeterm-app/src/comment_ui.rs:83-86 | |
| c16-119 | src/renderer/components/CommentDisplay.tsx:16 | "Line N (side)" line ref when not hidden | PORTED | treeterm-app/src/comment_ui.rs:100-111 | Every Electron caller passes `hideLineRef`; Rust uses this branch in the Files viewer's comment panel (README deliberate panel), clickable to reveal the line. |
| c16-120 | src/renderer/components/CommentDisplay.tsx:21 | "Outdated" badge with tooltip "This comment may be outdated" | PARTIAL | treeterm-app/src/comment_ui.rs:113-125 | Badge ("OUTDATED", as the CSS upper-cases it) present; no tooltip. |
| c16-121 | src/renderer/components/CommentDisplay.tsx:26 | Header × deletes the comment; tooltip "Delete comment" | PARTIAL | treeterm-app/src/comment_ui.rs:42-63, :127 | Delete works; no "Delete comment" tooltip (only comments_view.rs:189 has one). |
| c16-122 | src/renderer/components/CommentDisplay.tsx:35 | Comment text | PARTIAL | treeterm-app/src/comment_ui.rs:137 | (v) Was PORTED. The text and its line breaks (`white-space: pre-wrap`) are kept, but Rust draws it as a plain `div`, so it cannot be selected or copied. Electron's comment text is selectable. |
| c16-123 | src/renderer/components/CommentDisplay.tsx:37 | No header -> × inline beside the text; tooltip "Delete comment" | PARTIAL | treeterm-app/src/comment_ui.rs:138 | Inline × present; no tooltip. |
| c16-124 | src/renderer/components/CommentDisplay.tsx:48 | Created time via `toLocaleString()` | PARTIAL | treeterm-app/src/comment_ui.rs:140, treeterm-core/src/clock.rs:64 (`locale_date_time`) | Rust always formats en-US ("11/11/2026, 5:57:12 PM"); Electron follows the system locale. |

## src/renderer/components/CommentInput.tsx

| # | Electron | Behaviour | Status | Rust parallel | Notes |
|---|---|---|---|---|---|
| c16-125 | src/renderer/components/CommentInput.tsx:4 | Props: lineNumber, `side?` ('original' / 'modified'), onSubmit, onCancel | PORTED | treeterm-app/src/comment_ui.rs:146 (`comment_input`), :22 (`CommentActions`) | |
| c16-126 | src/renderer/components/CommentInput.tsx:17 | Text state | PORTED | treeterm-app/src/review_view/stacked.rs:295 (`draft_box`), treeterm-app/src/file_browser_view.rs:1323 | |
| c16-127 | src/renderer/components/CommentInput.tsx:20 | Submit only non-blank text, trimmed | PORTED | treeterm-model/src/review.rs:1639-1646, treeterm-model/src/file_browser_comments.rs:153-160 (`submit_comment`) | |
| c16-128 | src/renderer/components/CommentInput.tsx:22 | Text cleared after submit | PORTED | treeterm-model/src/review.rs:1676 (draft closed on success) | On a failed save Rust keeps the text and shows "Failed to add comment: …". |
| c16-129 | src/renderer/components/CommentInput.tsx:27 | Cmd+Enter or Ctrl+Enter submits | PARTIAL | treeterm-app/src/review_view/stacked.rs:309, treeterm-app/src/file_browser_view.rs:1334 (`PressEnter { secondary: true }`) | (v) Note extended. gpui-base binds the secondary Enter to cmd-enter on macOS and ctrl-enter elsewhere (input/base/state.rs:200-209). So Ctrl+Enter does not submit on macOS, though the placeholder says "Cmd/Ctrl+Enter", and Meta+Enter does not submit on Linux. Electron accepts either modifier everywhere. Also, the comment textarea is not `submit_on_enter`, so Cmd+Enter first inserts a newline and then submits. The saved text is trimmed, but after a failed save the box keeps the extra newline (Electron calls `preventDefault`). |
| c16-130 | src/renderer/components/CommentInput.tsx:30 | Escape cancels | PORTED | treeterm-app/src/comment_ui.rs:168-171 | |
| c16-131 | src/renderer/components/CommentInput.tsx:39 | Header "Comment on line N", with " (side)" when given | PORTED | treeterm-app/src/review_view/stacked.rs:393 (with side), treeterm-app/src/file_browser_view.rs:1400 (without, as FileViewer) | |
| c16-132 | src/renderer/components/CommentInput.tsx:46 | Placeholder "Add your comment... (Cmd/Ctrl+Enter to submit, Esc to cancel)" | PORTED | treeterm-app/src/review_view/stacked.rs:306, treeterm-app/src/file_browser_view.rs:1331 | |
| c16-133 | src/renderer/components/CommentInput.tsx:47 | Textarea focused on open | PORTED | treeterm-app/src/review_view/stacked.rs:321, treeterm-app/src/file_browser_view.rs:1346 | |
| c16-134 | src/renderer/components/CommentInput.tsx:50 | "Cancel" button | PORTED | treeterm-app/src/comment_ui.rs:191-205, treeterm-model/src/review.rs:1630 (`cancel_comment`) | |
| c16-135 | src/renderer/components/CommentInput.tsx:53 | "Add Comment" button, disabled while the text is blank | PORTED | treeterm-app/src/comment_ui.rs:206-224 | Also disabled while saving. |

## Gaps

- c16-6 MISSING src/renderer/components/BranchBadge.tsx:26 — no "renaming in progress" state for the branch badge's auto rename (searched renaming/refreshing/in_flight/pending/busy, `last_refresh` readers, spinners in app_view.rs). Nearest: treeterm-model/src/workspace_store.rs:376 (`refresh_labels`).
- c16-8 MISSING src/renderer/components/BranchBadge.tsx:30 — rename start is not tracked (internal; see c16-6, c16-13, c16-23). Nearest: none.
- c16-9 PARTIAL src/renderer/components/BranchBadge.tsx:32 — the branch name is read from the first AI-harness tab in `app_states` order; Electron uses the active tab first when it is an AI harness. Nearest: treeterm-model/src/workspace_store.rs:376 (`refresh_labels`).
- c16-10 PARTIAL src/renderer/components/BranchBadge.tsx:33 — only the no-harness failure is shown (a toast, with different text: "No AI Harness tab in this workspace" vs "No AI Harness tab open — the labeller has no terminal to read"). The analyzer's failures (title model unset, empty terminal, LLM error or no title, invalid branch name, git rename failure) go to `AnalyzerModel::last_refresh`, which nothing reads, so they are silent. Electron shows each in an `alert()`. Nearest: treeterm-model/src/analyzer.rs:272, treeterm-app/src/app_view.rs:1622.
- c16-11 PARTIAL src/renderer/components/BranchBadge.tsx:36 — unexpected failures (IPC/git) that Electron alerts are `Err` results in Rust and land in the unread `last_refresh` (same gap as c16-10). Nearest: treeterm-model/src/analyzer.rs:272.
- c16-12 MISSING src/renderer/components/BranchBadge.tsx:41 — rename end is not tracked (internal; see c16-6). Nearest: none.
- c16-13 MISSING src/renderer/components/BranchBadge.tsx:46 — the badge does not show a spinner + "Renaming..." during an auto rename. Nearest: treeterm-app/src/app_view.rs:1610.
- c16-23 MISSING src/renderer/components/BranchBadge.tsx:73 — "Auto rename branch" is not hidden while a rename runs, so repeated clicks start parallel LLM requests and can rename the branch twice. Nearest: treeterm-app/src/app_view.rs:1620.
- c16-24 PARTIAL src/renderer/components/BranchBadge.tsx:75 — menu label is "Auto-rename branch" (Electron: "Auto rename branch"). Nearest: treeterm-app/src/app_view.rs:1622.
- c16-46 PARTIAL src/renderer/components/BrowserSettingsSection.tsx:11 — local-URL check differs on edge cases. Rust rejects what WHATWG accepts: upper-case scheme, surrounding spaces, `127.1`/hex loopback, `http:localhost`. Rust accepts invalid ports (`:99999`, `:abc`). Nearest: treeterm-model/src/apps.rs:203 (`is_allowed_webview_url`).
- c16-50 PARTIAL src/renderer/components/BrowserSettingsSection.tsx:35 — pasting more than 2 UTF-16 units into the icon field is rejected outright instead of truncated to 2. Nearest: treeterm-app/src/settings_view/instance_list.rs:29 (`validate(icon_fits)`).
- c16-54 PARTIAL src/renderer/components/BrowserSettingsSection.tsx:58 — an invalid browser URL blocks Settings Save in Rust; Electron only warns and saves. Nearest: treeterm-app/src/settings_view/form.rs:287 (`check`).
- c16-94 PARTIAL src/renderer/components/Chat.tsx:153 — user messages keep their line breaks in Rust (Electron's `<p>` collapses them) and cannot be selected. Nearest: treeterm-app/src/tool_views/chat_view.rs:108.
- c16-96 PARTIAL src/renderer/components/Chat.tsx:206 — chat scrolls to the bottom only when message text grows, without animation. It does not scroll on other re-renders, so an error banner added to a long conversation can stay below the visible area. Nearest: treeterm-app/src/tool_views/chat_view.rs:70.
- c16-112 PARTIAL src/renderer/components/CloseConfirmDialog.tsx:40 — a workspace whose `gitBranch` is "" is listed as "name ()" instead of just its name. Nearest: treeterm-app/src/active_processes_view.rs:228.
- c16-120 PARTIAL src/renderer/components/CommentDisplay.tsx:21 — Outdated badge lacks the "This comment may be outdated" tooltip. Nearest: treeterm-app/src/comment_ui.rs:113.
- c16-121 PARTIAL src/renderer/components/CommentDisplay.tsx:26 — header × lacks the "Delete comment" tooltip. Nearest: treeterm-app/src/comment_ui.rs:42.
- c16-122 PARTIAL src/renderer/components/CommentDisplay.tsx:35 — comment text cannot be selected or copied. Nearest: treeterm-app/src/comment_ui.rs:137.
- c16-123 PARTIAL src/renderer/components/CommentDisplay.tsx:37 — inline × lacks the "Delete comment" tooltip. Nearest: treeterm-app/src/comment_ui.rs:138.
- c16-124 PARTIAL src/renderer/components/CommentDisplay.tsx:48 — comment time is always en-US formatted, not the system locale. Nearest: treeterm-core/src/clock.rs:64 (`locale_date_time`).
- c16-129 PARTIAL src/renderer/components/CommentInput.tsx:27 — Ctrl+Enter does not submit on macOS and Meta+Enter does not on Linux (only the Kit's platform "secondary" Enter). Cmd+Enter inserts a newline before submitting. Nearest: treeterm-app/src/review_view/stacked.rs:309, treeterm-app/src/file_browser_view.rs:1334.
- c16-139 PARTIAL src/renderer/components/Chat.tsx:145 — user messages and the error banner cannot be selected or copied (only assistant replies can). Nearest: treeterm-app/src/tool_views/chat_view.rs:108, :136.
