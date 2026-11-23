# Differences from the Electron app

`app-rs/` replaced the Electron app, which was removed after this audit; its last version is
commit `f05be955` (`git show f05be955:<path>` reads its sources). These are the places where the
app looks or behaves differently from it: the Appearance section mostly lists what GPUI and GPUI
Kit cannot do, and Deliberate differences lists behaviour changed on purpose (182 audit rows,
`STATUS.md`). They were sections of `app-rs/README.md` while both apps existed, and are kept
here as they were then, so "Electron" below is the Electron app at that commit.

## Appearance

The chrome was matched to Electron's computed styles, measured side by side on one session
(sizes, padding, fonts, colours, icons; Lucide icons through GPUI Kit). What still differs:

- Text: GPUI renders the system font 1–3px wider per label and has no `letter-spacing`;
  Chromium colour-manages saturated colours (same tokens, different pixels).
- GPUI Kit limits: tooltips are not the native macOS ones; dialog shadows are lighter; the
  diff view keeps 19.2px rows, its own fold rows and two columns for added/deleted files,
  and shows a file once, so a partially staged file has one section with both Unstage and
  Stage (Electron lists it twice, each section with the staged hunks);
  the editor has no minimap and cannot open space between lines, so the Files viewer lists
  its file's review comments (and the comment box) in a panel under the editor instead of in
  Monaco's view zones under their lines, each comment with the "Line N" link Electron leaves
  out there; selects open a Kit menu, not the native popup (a focused one has no
  type-ahead); the comment box and the Workspace Settings description have no resize grip,
  and Settings' and Add Child's textareas resize down to one line, where Chromium's go down
  to none; GPUI has no backdrop filter, so the prefix-mode
  overlay does not blur what is behind it; GPUI has no animated scrolling, so the Chat tab
  jumps to its last message where Electron's smooth-scrolls there. Off
  macOS the menu bar is GPUI Kit's, drawn in the window (not GTK's), above dialogs so that it
  stays usable while one is open, as GTK's is; it keeps its size, as GTK's does, when View →
  Zoom In or Zoom Out zooms the window, save for the few lengths GPUI Kit gives in pixels
  (its menus' 2px separators, rounded corners and shadows), which follow the zoom. On macOS
  GPUI greys out a Cut, Copy, Paste or Select All that nothing focused can do (Cut in a
  terminal, Paste with no input focused); Electron's stay enabled and do nothing there. Off
  macOS the Edit menu acts on a web page (Browser and Web App tabs) once the page has been
  clicked, not when it has the keyboard otherwise (an `autofocus` field).
- A press on one of the app's classic scrollbars (search results, the Files tree and its
  file's comments, the Markdown preview, the Comments tab's code, the Raw Workspace JSON)
  clears the current text selection, as GPUI Kit's window selection clears it on every left
  press, before the bar sees it; Chromium keeps the selection.
- In the Review diff, a review comment's text and the diff's code are selected apart: a
  selection running from a comment into the code (or back) copies the comment text only,
  not the code it spans, where Electron copies both, in reading order.
- View → Actual Size, Zoom In and Zoom Out (in Chromium's steps and 25%–500% limits, for every
  window) restore their level at the next start from the native app's own `zoom.json`
  (Electron keeps its level in Chromium's `Preferences`, which Chromium rewrites whole), so
  each app starts at its own last level. The macOS window buttons stay where they are, as in
  Electron.
- Review's diff is coloured as Electron's (VS Code's Dark+), but through GPUI Kit's
  tree-sitter grammars, not shiki's TextMate ones: control keywords (`if`, `return`,
  `import`) are purple (`#C586C0`) in Rust, JavaScript, TypeScript, Python and Go only,
  and blue like every other keyword elsewhere, as the other grammars' queries capture all
  keywords alike; a few names are classed differently (Rust's all-caps
  constants and `macro_rules!` are blue, `Self` teal, TypeScript's class names in
  expressions and JSX tags light blue); and code inside Markdown (inline code, links,
  emphasis, a fenced block's language) is plain, and every name in a Rust macro's arguments
  light blue (a call there is not yellow, nor a module teal), as the diff skips injected
  grammars.
- The code editors (Files viewer, Editor tab), through GPUI Kit: Tab inserts a whole indent
  step, not spaces up to the next tab stop; 8px of padding stays under the last line; the
  current line's outline is drawn over the text, so it stops at the scrollbar; a scrollbar
  fades 2 s after scrolling stops (Monaco: 0.5 s). The Kit has no R, XML, Less or Dockerfile
  grammar: `.r` files are plain, and `.xml`, `.less` and `.dockerfile` files are coloured
  with the HTML, CSS and shell grammars. The word underlined under Cmd/Ctrl keeps the
  text's colour (Monaco's link is `#4E94CE`).
- There are no language servers behind the code editors. Monaco's TypeScript / JavaScript
  service (semantic errors such as unresolved imports, IntelliSense, hover types, signature
  help, references, rename, formatting), JSON schemas (validation, completion, hover: a file
  whose root object names a `$schema` gets no JSON completions, where Monaco's completes
  the keywords of the JSON Schema drafts it has built in), the CSS worker's hover over
  selectors (their specificity), at-rules and pseudo-classes, CSS value and selector
  completion, the CSS parser's errors and value lint, HTML tag rename
  (F2), and the CSS and HTML formatters are missing. A problem's hover shows its message
  without the source and code Monaco adds after it (`json(513)`, `(1005)`). Suggestions have
  no details pane: Monaco's (its Read More button, or Ctrl+Space while they show) holds the
  selected one's description and MDN reference. TypeScript and JavaScript
  syntax errors are read from GPUI Kit's tree-sitter grammars, not TypeScript's parser: they
  are marked where the parse fails, as "';' expected."-style messages or "Unexpected
  token.", and a `.tsx` file is read with JSX (Monaco's worker reads it as plain TypeScript
  and marks the JSX). CSS files get the lint warnings that read rules and declarations, not
  the CSS parser's errors; SCSS and Less files get none. JSON whose arrays and objects nest
  more than 512 deep, and style sheets whose blocks do, get no marks or colours (Monaco's
  workers give up only where JavaScript's stack runs out, a few thousand and about a
  thousand levels deep). Colours are painted behind their text, not as a swatch before it,
  and have no colour picker. A picked CSS property leaves the cursor before its `;` without
  asking for its values, as there is no CSS value completion (Monaco lists them there). A
  picked snippet leaves the cursor at its first tab stop, but Tab does not go on to its
  later ones, as Monaco's snippet session does: after `data-` (`data-$1="$2"`) Tab indents
  there instead of moving between the quotes, and inside an attribute's quotes
  (`name="$1"`) it indents instead of moving past the closing one. Other suggestions are the
  words of the file and of the Editor tabs and Files viewers of the workspace with a file of
  its language open; what is a comment or a string, where typing suggests nothing, is read
  from GPUI Kit's tree-sitter grammars, not Monaco's tokenizers. The Editor tab's right-click menu has Format Document
  and Format Selection for JSON only, and no Go to References, Peek References, Rename
  Symbol or Command Palette.
- Peek Definition (Alt+F12, Ctrl+Shift+F10 on Linux, or the right-click menu's Peek
  submenu) lists the definitions found above the code, as go to definition lists several,
  not in Monaco's peek widget under the line (GPUI Kit's editor has no view zones to show a
  peek in; Monaco's has no preview of grep's results either). Open Definition to the Side
  (Cmd/Ctrl+K F12) goes to the definition as F12 does, as in Electron, whose editor opener
  opened it in a new Editor tab (there is no split editor in either app).
- The Files viewer's right-click Search reads the word under the cursor in the language
  there, as Monaco does (JavaScript or CSS in the `<script>` and `<style>` blocks of an HTML,
  Vue, Svelte or PHP file, a Markdown code fence's language), except in Less's backquoted
  JavaScript, read as Less, and in the `<script>` and `<style>` blocks of a Handlebars,
  Razor, Twig or MDX block (a code fence, or a script block of that `type`), read as that
  language.
- With the sidebar collapsed the header keeps clear of the macOS window buttons (Electron's
  title runs under them).
- GPUI cannot decode AVIF, so the Files viewer shows `.avif` images in a webview (WebKit
  draws them, centred and scaled down as Electron's `<img>`). Review's image diffs, which
  scroll in a list, decode an `.avif` change's sides with rav1d (`vendor/avif-decode`) in a
  child process, the app run with `--decode-avif`: rav1d 1.1.0 aborts the process on some
  corrupt frames, so such a side shows as a broken image with its alt text
  (`Before: <path>`), as Chromium shows an image it cannot decode, and the app carries on.
  avif-decode's parser (avif-parse 2.1.0) reads still, single-image AVIFs only: an animated
  AVIF (`avis`) or one stored as a grid of tiles also shows in Review as a broken image with
  its alt text, where Electron shows the picture (animated, for an `avis`), and an AVIF's
  rotation, mirroring and crop (`irot`, `imir`, `clap`) are not applied.
- The Markdown preview's images show as broken with their alt text, as in Electron (where
  none loads), with the grey image-off icon in place of Chromium's broken-picture icon.
- PDFs show in Chromium's PDF viewer rebuilt on pdf.js (its legacy build, for older
  WebKits): the toolbar's Menu (a sidebar of thumbnails, and the document outline when there
  is one), page box, zoom presets and level, Fit to page / Fit to width, Rotate
  counterclockwise, Download and Print, and Chromium's "Password required" dialog for a PDF
  with a password. Chromium's ⋮ menu (two-page view, annotations, document properties) and
  find in the PDF are missing; Print lays the pages out as 150 dpi images, as pdf.js prints.
- While a tab is dragged, the outline of where it would dock jumps to its place (flex-layout
  slides it there in 0.3 s), the edge docks' arrows are text triangles, and a pane showing a
  web page (Browser, Web App, a PDF) draws its page over the outline and edge docks: native
  webviews draw above the window's content.
- A session dragged in the sidebar or the rail, or a workspace dragged in the tree, dims as
  in Electron, but GPUI has no drag image of its section or row: a chip with its name follows
  the pointer instead.

## Deliberate differences

### Start-up, connections and the daemon

- There is no separate splash window: the main window opens at once and shows Electron's
  splash ("TreeTerm", the spinner and "Connecting to daemon...") in its pane until the local
  session has loaded. Electron showed it in a small always-on-top window while it started the
  daemon, then opened the main window on "Select a workspace to get started" while it
  connected.
- The window title and the menus name the product "TreeTerm"; Electron's window title was
  "Treeterm" (its page title) and its app menu "treeterm" (from `package.json`).
- If the local daemon cannot be started, the window stays open on "Cannot reach the TreeTerm
  daemon" and the reason, with Reconnect in the sidebar; Electron showed a "Startup Error" box
  and quit.
- A daemon that cannot be reached says why (its socket cannot be connected to, the gRPC
  handshake failed, or it timed out) instead of grpc-js's "Failed to connect before the
  deadline".
- A stale local daemon socket is replaced at once (Electron waited up to 5 s for it to
  answer). Connections to use still wait up to 5 s for the daemon, as in Electron.
- Exit and Kill Daemon asks about unmerged workspaces before it stops the daemon, so Cancel
  leaves everything running; Electron stopped the daemon first and asked after, so Cancel left
  the window on a dead daemon. Pressed while Quit's confirmation is up, it makes that
  confirmation's Close Anyway stop the daemon too.
- A remote daemon that has no SessionWatch at all (`UNIMPLEMENTED`) is reported as outdated
  too; Electron only recognised a `NOT_FOUND` error about the session. A Reconnect from the
  error waits for the session again, so it stops at the error once more instead of
  reconnecting in a loop.
- While a connection reconnects, its banner keeps the reason it was lost; Electron could
  replace it with the old tunnel's ssh exit message ("SSH tunnel closed (exit 255)") when that
  process exited during the backoff.
- A workspace write still in flight when the connection drops is written after the reconnect
  too (Electron's flush waited for it to fail and then dropped it).
- Retry on a failed local connection connects again; Electron's did nothing for the local
  session.
- Disconnecting a session removes it from the app, its session pane with it; Electron's other
  windows kept showing it as "Disconnected", with Retry and Remove.
- The session pane's Info tab shows the daemon's session id once the session has loaded;
  Electron's "Session ID" was its own per-window store key.
- SSH Upload's "Browse..." is disabled while the connection is down; the form keeps its file
  and directory under "Connection is … — uploads are disabled until connected.", as in
  Electron. Electron opened the browser, which could only show its failed listing.
- An Upload pane whose connection is gone says "The SSH connection is closed."; Electron kept
  spinning on "Loading connection..." when its status watch failed.
- SSH Monitor shows the latest run's result: a run that finishes after a later one is
  dropped (Electron showed whichever finished last). A run whose result the daemon never
  sends reads "No result from the daemon after 40s" when the client gives up on it, unless a
  later run's result is already shown; Electron left it pending.
- The System Monitor shows macOS load averages; Electron's script glued them into one number,
  so it showed 0.00 0.00 0.00 on macOS.
- A daemon command whose result never arrives is given up 10 s after its timeout (Electron:
  5 s), so the daemon's own SIGKILL, 5 s after its SIGTERM, still reports first. The error
  names the command: a lost home-directory lookup reads "sh -c echo $HOME timed out after
  40000ms" (Electron: "resolveHomedir timed out after 35000ms").
- `treeterm ssh <target>` refuses a target that is not `user@host[:port]` with "Invalid SSH
  target: <target>" and exit status 1, also one whose host contains `@` or whose port is outside
  1–65535 (`me@a@box`, `me@box:0`, `me@box:70000`). Electron opened the app without connecting
  for a target it could not parse, and passed those on to ssh, which cannot use such a port and
  splits `me@a@box` on its last `@`, connecting to `box` as user `me@a`.
- Daemon fix: each gRPC connection now gets its own id, so the session lock actually excludes
  other connections (it was shared by all connections before).
- Daemon fix: a PTY's child process is fork-safe (environment, cwd and argv built before
  `forkpty`; the child only calls `chdir` + `execve`). Before, `std::env::set_var` in the
  child of the multi-threaded daemon could deadlock it, and the stuck child kept other
  commands' pipes open (a git command started at the same time never finished).
- A `logLevel` in `ui-state.json` that names no level is logged as a warning and ignored;
  Electron ignored an unknown `treeterm:logLevel` silently.
- A panic on the UI thread ends the app process: GPUI cannot recover inside the process, so
  there are no per-workspace or per-tab error fallbacks (Electron's "Workspace Error" and "Tab
  Error" screens, with Reload Workspace, Reload Tab and Close Tab). A new process shows
  Electron's "Application Error" screen, with the panic's message and a Show Stack Trace toggle
  (Electron's app-level screen showed no error details; the panicked process and its console
  are gone), and its "Reload Application" starts the app again: terminals survive in the
  daemon, SSH sessions must be reconnected, and a directory given on the command line is not
  reopened (as after Electron's reload). The report stays in `crash.json` beside
  `settings.json`.

### Sessions and the sidebar

- Custom session names and the sidebar order are kept by connection target (`local`,
  `ssh:user@host:port`) in `ui-state.json`, so they survive reconnects and restarts and never
  expire, and renaming a session leaves it in its place. Electron keyed them by its per-connect
  session id: names were lost at a restart, a reconnected session moved to the bottom, unused
  entries were dropped after 7 days, and renaming a never-dragged session moved it above the
  other never-dragged ones.
- A corrupt `ui-state.json` (session names and order, Recent Directories) is logged and kept as
  `ui-state.json.corrupt-<ms since the epoch>`; the state starts empty and is saved again as
  usual. Electron dropped a corrupt stored value silently and overwrote it.
- After a reconnect a session keeps the workspace it had active (its first one if that is
  gone), and the window stays on the session it shows; Electron made the reconnected
  session's first workspace active and switched the window to it, even from another
  session's terminal in use.
- A session's fold and its workspaces' expand state survive a reconnect (Electron rebuilt
  the session panel on reconnect, unfolding it and expanding every workspace).
- The session's force-unlock button hides once the lock has expired (the daemon ignores an
  expired lock); Electron shows it as long as the daemon still reports the lock.
- Placeholders for refs another window removed are dropped with the ref.
- A collapsed workspace stays collapsed until it gains a child or is expanded (Electron
  re-expanded every parent whenever any workspace changed).
- Dragging the sidebar's divider, or the Files tab's tree divider, goes on resizing until the
  button is released, also with the pointer outside the window or the tab; Electron ended the
  drag when the pointer left the window (sidebar) or the Files tab (tree).
- Workspace focus (the prefix, then W) walks the workspaces in sidebar order and reaches those
  whose parent is gone; Electron walked them in load order and skipped those.

### Workspaces and tabs

- A workspace opened from a path with a trailing slash is named after its last directory
  (`/a/b/` gives `b`); Electron named it after the whole path.
- Selecting a workspace whose creation or loading failed shows why, with Dismiss; Electron
  showed "No workspace selected".
- The Settings item of a workspace row's or a favourite's menu opens or focuses that
  workspace's single Settings tab and shows the workspace (Electron added another Settings
  tab each time, without showing the workspace).
- Tab labels follow the tab's current title (an Editor tab's file name and unsaved marker
  among them); Electron's tab strip kept the title a tab had when it was added.
- A tab dropped in a strip, or docked into or beside a pane or at the layout's edge, becomes
  the active tab (`activeTabId`). Electron showed the dropped tab, but its `activeTabId`,
  which its tab shortcuts act on, stayed on the tab that was active before.
- A tab opened with a pane's "+" opens in that pane (Electron opened it in the last active
  pane).
- In a strip too full to show every tab, tabs scrolled out of view are no drop target: a tab
  dropped past the last tab shown lands after it. flex-layout also counted the hidden tabs'
  places past the strip's edge, so a drop over "+" could land among them, out of sight.
- A tab whose saved state is malformed opens anyway, where Electron's renderer left it blank: a
  Review tab whose state is not an object or whose `parentWorkspaceId` is not a string shows
  the review, a Comments tab whose state is null or not an object lists the comments, a
  Browser tab without a `localPort` object opens its start page, and a Web App tab without
  `port` or `localPort` objects finds a port and starts its server, as a new one does. An
  AI-harness tab is the exception (see below).
- A Run action that cannot start (its file cannot be read, or the daemon refuses the terminal)
  shows "Failed to run <action>: <error>" beside ▶ Run; Electron did nothing.
- Terminal, AI-harness and custom-runner tabs do not record `connectionId` in their state: it
  is a per-process id that another app cannot use, and without it Electron kills the PTY
  through its own connection.
- A PTY that finishes starting after its workspace's store was disposed is not written to the
  workspace (Electron wrote it anyway); the next store finds it again by the tab's `ptyHandle`.
- Unknown top-level keys in a workspace file are kept and written back; Electron's schema
  drops them on its next write.

### Git and worktrees

- Remove on a top-level workspace (its row's menu) and Close workspace (the header's ×) take it
  and its child workspaces out of TreeTerm only: the children's worktrees, branches and
  worktree-registry entries stay on disk. Electron force-removed the child worktrees (their
  uncommitted changes with them) and deleted their branches and registry entries, behind the
  same plain confirmation.
- A workspace whose `git worktree remove` fails stays, with the error; Electron dropped it and
  left the worktree on disk, untracked. A worktree git has already forgotten still goes.
- Close and Clean keeps the branches of the workspace's children too, as its confirmation says
  the branch is kept; Electron deleted the branches of the children that were not detached.
- An entry of the worktree registry (`~/.treeterm/worktrees.json`) that cannot be read, such as
  a hand-edited one with no `branch` or a fractional `lastUsedAt`, is left out of Auto Open
  and the recent workspaces (logged) and kept in the file as it is. Electron listed it with
  whatever fields it had.
- A git info refresh that git cannot answer (the daemon unreachable, git killed by the
  daemon's 30s timeout, the workspace's directory gone) leaves the workspace's git repo,
  branch and root as they were, and the git status button's tooltip says why. Electron
  recorded the workspace as no git repo then.
- Merge-and-remove stages untracked files before its WIP commit (Electron's `commit -am`
  skips them, failing the merge or losing new files with the deleted worktree).
- Merging a workspace that records no branch is refused ("Workspace has no branch to merge");
  Electron ran `git merge ''` and showed git's error.

### Terminals

- A terminal takes keyboard focus when it comes into view as the active tab, but not from an
  open dialog, and not again on every workspace change while it stays shown (Electron
  focused it on each, taking focus from the sidebar's inputs, from dialogs and from a
  terminal clicked in another pane).
- A terminal's error or "Terminal disconnected" banner stays until it is clicked away, also
  across tab and workspace switches, and a program that fails within 1 s of starting keeps its
  tab with "Process exited immediately with code N" even if the tab is not on screen. Electron
  dropped a banner once its tab was hidden, ignored a stream that ended meanwhile, and closed
  such a hidden tab without a word.
- A keep-on-exit tab whose program exited shows only its "Process exited with exit code N"
  line; Electron also raised a red "Terminal disconnected" banner when the daemon then closed
  the stream.
- Paste in a terminal's context menu is a bracketed paste with line endings normalised, as
  Cmd/Ctrl+V is; Electron's wrote the clipboard raw, so a program relying on bracketed paste
  got the lines as typed keys.
- An AI harness set to Disable Scrollbar draws no scrollbar on its terminals. Electron's
  `disable-scrollbar` rule targets `.xterm-viewport`, which no longer holds the scrollbar
  since xterm.js 6, so the bar still shows there. In both apps the wheel scrolls the
  scrollback (or goes to a program that asked for mouse events).
- An AI harness's "Background color" is read as CSS Color 3 (`#rgb`/`#rrggbb` with or without
  alpha, `rgb()`, `hsl()`, `hwb()`, colour names) and drawn opaque; anything else gives
  xterm.js's black, as an unreadable colour does in Electron, also Chromium's newer `lab()`,
  `oklch()` or `color()`. A change reaches open harness tabs at once; Electron kept the colour
  a tab's view was mounted with until it was mounted again.
- URLs in a terminal's text are links in every terminal tab, with the schemes ghostty-web
  links (`http(s)`, `mailto`, `ftp`, `ssh`, `git`, `tel`, `magnet`, `gemini`, `gopher`,
  `news`): Cmd/Ctrl+click opens one, and it is underlined only while Cmd/Ctrl is held, so a
  plain click still selects its text. Electron's terminals (xterm.js) linked only OSC 8
  hyperlinks; its Ghostty tabs opened URLs on a plain click. An OSC 8 hyperlink is a link
  only to an `http(s)` URI, as in xterm.js, also where Electron's Ghostty tabs opened any.
- A double-click, or a right-click on macOS, on an OSC 8 hyperlink selects the word under the
  pointer, as on any other text. xterm.js selected the hyperlink's whole text.
- "Terminal (Ghostty)" tabs (`ghostty-terminal`, 👻, offered in the new-tab menu as in
  Electron, which draws them with ghostty-web) run in the same native terminal as
  "Terminal". They copy a selection to the clipboard as a drag, double-click or triple-click
  makes it, as ghostty-web does (it has no triple-click), but otherwise select as xterm.js
  does (its word separators, Shift+click, Alt+drag and Alt+click, the macOS right-click). A
  plain click copies nothing; ghostty-web copied the character under it. They honour "Allow
  OSC 52 clipboard" as every terminal does; Electron's Ghostty engine had no OSC 52 handler.
- A replayed OSC 52 copy or bell is ignored (it already happened when it first ran).
- An OSC 52 copy whose selection is empty or starts with `c` (`52;;…` or `52;cs;…`, as tmux
  sends) reaches the clipboard, as in xterm and alacritty; Electron took only exactly `c`.
- The activity detector ignores spaces at the end of a line, as Electron's Ghostty tabs did:
  a repaint that only adds or removes trailing spaces is no activity.
- The live view of a PTY (Active Processes, and Open in the session pane's TTY list) is the
  app's terminal: the terminal settings' font, cursor style and blink, the terminal palette
  (ANSI colours, cursor, selection), 50,000 lines of scrollback, and a tab's scroll badge,
  ALT SCREEN badge and refresh button. Electron's `PtyViewer` drew 13px `monospace` in
  xterm.js's default colours (it set only the background and foreground), with an
  always-blinking block cursor and 5000 lines. When its stream ends or fails without the
  program exiting, the view says so ("Terminal disconnected", or the error) in a banner;
  Electron's viewer showed nothing.
- A PTY whose start time is ahead of this machine's clock (a remote daemon's clock skew) shows
  "0s ago"; Electron showed a negative age ("-5s ago").
- A workspace's download badge counts each terminal's output once; Electron also counted the
  copies its analyzer and its prompt writer received through their own attachments to the
  terminal, so an AI-harness workspace read two to three times higher there.
- Show Raw Characters logs its `[RAW]` lines to the app log (stdout), with the PTY's id, and
  its hint says so; Electron logged them to the DevTools console. They are info lines, so a
  `logLevel` above `info` hides them, where Electron's `console.log` always printed. As in
  Electron, only the terminals of the shown workspace log, starting over each time it is
  shown again; while a session pane is up none do, where Electron kept logging the session's
  active workspace's terminals behind it.

### Files and the code editors

- The Files tab's "Show hidden files" checkbox, under its "Search files..." box, shows entries
  whose name starts with `.` in the tree (saved as `showHidden` in the tab state, which
  Electron keeps but ignores). The daemon's `ReadDirectory` lists every entry, so the checkbox
  only picks the rows, without reading again; a daemon from before that leaves them out
  regardless. The file name search still skips them. Electron's tree never shows them.
- The Editor tab reads its file afresh and whole, whatever its size: Open in Tab does not take
  the Files viewer's copy, so the tab starts from what is on disk now, and a file opened anyway
  in the viewer past its size limit can be edited. Electron handed over the viewer's copy,
  which could be long out of date and which Save then wrote over the disk.
- The Editor tab refuses a file that is not valid UTF-8 ("Binary files cannot be edited");
  Electron opened the viewer's lossily decoded text, so saving replaced the file's
  undecodable bytes with U+FFFD.
- An Editor tab whose file could not be read stays on its error until it is opened again;
  Electron read the file again in a tight loop for as long as it failed.
- Download asks where to save before it reads the file, so cancelling transfers nothing, and
  it saves the whole file even if it grew past the viewer's size limit since it was shown.
  Electron read the file first ("Downloading…"), and failed with "File too large to open" for
  one that had grown.
- Go to definition moves within the Files viewer or the Editor tab when the definition is in
  the file it shows; Electron opened that file again in a new Editor tab.
- Go to definition reports a search that could not start, failed or never finished as "Could
  not look up <symbol>: <error>"; Electron reported no definition then, or never settled.
- An Editor tab keeps what was typed in it when another window, saving something else at the
  same moment, wins the race to write the workspace with the tab's text from a moment before
  (up to 5 s): the typed text stays and is written again. Electron's editor took the older
  text, losing the keystrokes typed meanwhile. Text another window or Electron types is taken
  as it arrives, as in Electron.
- The code editors colour files Electron shows as plain text: Makefile, Dockerfile,
  CMakeLists.txt, `.toml`, `.markdown`, `.mjs`/`.cjs`/`.mts`/`.cts`, `.jsonc`,
  `.cc`/`.cxx`/`.hh`, `.zig`, `.ex`/`.exs`, `.proto` and `.diff`/`.patch`.

### Review

- Review follows the workspace's current parent, whatever parent its tab recorded: by default
  it compares against that parent's branch, and it checks conflicts against it, also after the
  workspace is dragged under another parent. Electron used the parent recorded in the tab when
  it was opened (none for a tab that recorded none), though its merge went to the current
  parent.
- Review comments made in the Files tab are saved with the workspace-relative path, as the
  Review tab saves them (Electron's Files tab saves the absolute path), so they also show in
  Review and can be pushed to GitHub. Comments with Electron's absolute paths still show in
  the Files viewer and the Comments tab; Electron's Files tab does not show the relative ones.
- A review comment submitted in the Files viewer while `HEAD` is still being read is refused
  with "Still reading HEAD; try again", keeping the draft; Electron's Files tab saved it
  without a commit, so it could never go outdated.
- Review comment and viewed-file metadata (`reviewComments`, `reviewViewedFiles`) that is not
  valid JSON of the expected shape (an entry missing a field Electron writes, or of the wrong
  type) is an error shown where it is used (Review, the Comments tab, the Files viewer), and
  changes to it are refused until it is fixed. Electron read a value it could not parse (or
  comments that were not a list) as empty ("No review comments yet") and overwrote it on its
  next change, and it took entries of any shape as they were.
- A review comment that cannot be saved shows "Failed to add comment: …" under its box,
  keeping the draft, and a failed merge is reported in the Review tab's merge output (and, when
  git failed, the workspace's "Operation failed" panel); Electron also opened an alert for each.
- A failed merge's output ends with `Merge failed: <error>` once (Electron doubled the prefix
  when `git merge` itself failed), or `Merge succeeded but cleanup failed: <error>` when only
  the worktree removal after it failed (Electron's merge output said `Merge failed: <error>`
  there too, although the merge had gone through).
- Review's Refresh also reloads the Commits list once it was shown, keeping as many commits
  as are listed and the picked commit (Electron never reloaded the list after its first load).
- Review reads the diffs of the files it has read again with each reload (Refresh, its
  workspace shown again, staging), keeping what they show until then; Electron kept a file's
  contents once loaded until its own refresh button was pressed.
- The Commits list loads its next page once it is scrolled within 200px of its end, as
  Electron's sentinel was meant to; there the 200px margin never applied, so a page loaded only
  at the very end.
- Review's find in the diff searches only files whose rows are drawn: a diff too large to draw
  is searched once "Load anyway" draws it. Electron listed its matches but could not show
  them (choosing one opened the file on the size notice).
- Review's find reads every file of the view's list, in list order, also the files the file
  filter or a folder filter hides; Electron read only the files shown, in the tree's order. Its
  count can read "…" a little longer while hidden files are read.
- Review shows a renamed file of the branch or of the index as a rename (its changes since
  its old path); Electron read both sides at the new path, so the whole file showed as added.
- Review measures a diff's size from git's whole-file patch (`git diff -U1000000`); Electron
  read both files. With "Ignore whitespace changes" on, unchanged lines count as the new side
  has them. A file whose patch has no hunks is never too large: a pure rename or a mode-only
  change. Electron measured both files, so a large file in either case showed "This diff is
  large (...)" and "Load anyway".
- Review's "Ignore whitespace changes" diffs git's patch again over trimmed lines, as jsdiff
  does (a change of indentation, trailing spaces or line ending is unchanged; one of spacing
  within a line still shows). A line that is unchanged only once trimmed shows its new text
  on both sides of the split view; Pierre showed each side's own.
- Review's fresh conflict check before a merge shows "Checking for conflicts..." and disables
  the header's actions while it runs (Electron showed nothing and left them clickable).

### AI harness, attention and tool tabs

- A workspace counts as viewed (it gets no unread marker, and its marker is cleared) only
  while it is the one on screen. Electron counted the one its navigation last named (the
  one last clicked, or a session's first), also after a quick fork or a new workspace put
  another one on screen.
- A workspace's attention flash plays once per change of its activity, on every row that shows
  it. Electron also replayed it whenever a row was drawn anew (a folded session unfolded, the
  sidebar collapsed or expanded, a workspace newly marked favourite) while the workspace was
  in an attention state and its unread marker was not showing.
- An unread marker appears or clears on screen as soon as it is recorded or acknowledged;
  Electron showed the change only once the daemon echoed its write.
- Work that finishes while the session reconciles its workspace list still marks the workspace
  unread; Electron paused attention for the whole reconcile and missed it.
- The attention ding is the system bell, so it follows the OS's alert sound, volume and mute
  (Electron synthesised an 880 Hz tone). The bell cannot fail, so there is no "Workspace
  notification sound failed" banner.
- An AI-harness tab whose state is not a harness state (no `sandbox` object, from a corrupt
  workspace file) shows "Error: Invalid AI harness state" in red, the message Electron's
  `AiHarness` view has for it; Electron's renderer checked first and left such a tab blank.
  In both apps its terminal and analyzer run on behind it.
- The analyzer reads the tab's own terminal; Electron attached a second, headless one.
- A change of the analyzer's classification settings (provider, model, reasoning effort,
  endpoint, system prompt, criteria, safe paths) reaches an AI harness tab at once and counts
  as a change of its screen even if changed back within the idle debounce: the tab goes
  Working and its screen is classified again (from the remembered answer). Electron compared
  the settings themselves every 500 ms, so a change undone in time went unnoticed.
- A Prompt Description that sends nothing (no harness running yet, or the write failed)
  shows why in a notification; Electron failed silently. In both, the description is no
  longer offered.
- A workspace's title or branch re-label is under way for the whole workspace: one started
  from the tree's Refresh Title & Description also spins and disables the header's refresh
  button, and one from its Refresh Branch Name also shows "Renaming..." on the branch badge
  and leaves Auto rename branch out of its menu, until it finishes (Electron's button and
  badge knew only their own clicks). A re-label whose AI Harness tab closes (or whose
  terminal is replaced) before the LLM answers applies nothing and alerts "The AI Harness
  terminal went away before the labeller finished" (Electron applied the late answer).
- A title, description or branch name the LLM gives as anything but a string counts as
  missing; Electron passed it on (a number, say) into workspace metadata.
- An LLM request that cannot connect says why ("Connection error: …"); Electron's SDK said
  only "Connection error.".
- Analyzer History's list of LLM calls updates live; Electron's was a snapshot until Refresh.
- Your own Chat messages keep the line breaks and spaces typed in them (Shift+Enter);
  Electron's paragraph ran them together on one line.
- Chat skips streamed chunks without `choices[0].delta`, such as usage-only ones; Electron
  failed the reply with a TypeError.
- When a PR's reviews and checks cannot be loaded (a GraphQL error, or a token without access),
  the GitHub tab says "Reviews and checks are unavailable: <reason>" and the tree shows the PR
  number grey. Electron reported the PR as open with no reviews or checks, so a merged or
  closed PR could show as open.
- Leaving Workspace Settings' Name or Description unchanged saves nothing; Electron saved the
  workspace again, which moved its worktree up Add Child Workspace's Recent list.

### Settings

- Corrupt `settings.json` is an error (logged; defaults used) instead of being overwritten.
  Only Save in Settings and the sound toggle write over it; changes made in passing (a folder
  picked with Browse..., a saved or removed SSH connection, its port forwards, the prompt
  debugger's Update) are logged and not saved until it reads again. The new-tab menu still
  offers the default applications meanwhile (the Claude AI harness among them); Electron
  registered none until the next save.
- A `settings.json` value of the wrong type or out of range falls back to its default on its
  own (logged); Electron type-checked only the classifier settings and passed the rest on.
- Keys of `settings.json` this version does not know (at the top level, in sections, among
  keybindings and classifier criteria) are kept and written back, so both apps can share the
  file, and a saved `workspaceFocus` keybinding is kept. Electron dropped unknown keys on its
  next write and reset `workspaceFocus` to `w` at every start.
- Settings > Save writes only the fields the dialog edits over the current settings, so recent
  directories, saved SSH connections, the sound toggle and prefix mode turned on or off
  meanwhile are kept; Electron wrote back the whole snapshot it took when it opened.
- Settings' number fields are checked on Save, which refuses an empty, negative, fractional or
  too small value with a message naming the field ("Font size must be a positive number") on
  its section, and saves an idle debounce of 0. Electron's `parseInt(value) || <default>`
  refilled the field as it was typed, stored negative values and turned 0 into the default.
- Settings > Default Application for New Worktrees also offers the variants added in the open
  dialog before Save; Electron offered only saved ones.
- A default-application id this app has no application for shows as "<id> (not available)", in
  Settings and in Workspace Settings, where Use Global Default clears it. Electron's Settings
  showed Terminal for it (keeping the id), and its Workspace Settings showed Use Global Default
  and could not clear it.

### Dialogs

- Auto Open All Recent Worktrees and Add Child Workspace's Open Existing and Recent lists leave
  out every path already open in the session, the main checkout and worktrees opened as plain
  workspaces included. Electron left out only open worktree workspaces: Auto Open listed the
  others checked and skipped them on Ok, and in Add Child opening one failed with "This
  worktree is already open".
- The Open Branch and Open Remote searches ignore spaces around the query; branch names have no
  spaces, so Electron's untrimmed query with a stray space found nothing.
- The SSH picker trims the host and user, refusing blank ones with "Host and user are
  required", and refuses a port that is not a number from 1 to 65535 with "Invalid port “…”".
  Electron tried a blank host or user as it was, and took `parseInt(port) || 22` for the port
  (`abc` and `0` gave 22). Its Add trims the forward's remote host, so a blank one is
  ignored like an empty one (Electron added a forward to " "). Enter on a focused button there
  presses only that button; Electron's dialog then also ran Connect with the form as it was.
- In Add Child Workspace, Enter on a focused tab or on Settings that starts opening the picked
  existing or recent worktree does only that; in Electron the button then also pressed itself
  (switching the tab or toggling Settings).
- In Open Workspace's remote directory browser, Enter in the path field or on a focused
  browser button ("..", a breadcrumb, an entry, Cancel or Select) acts on the browser only;
  Electron also opened the folder field's path, when it held one, and closed the dialog. Enter
  in the path field reads the directory again even when the path is unchanged, so a failed
  listing can be retried; Electron ignored it.

### Web tabs and port forwards

- Web App port-probe failures are prefixed `curl: ` (for example when curl is missing), so they
  name the failing command; Electron showed the daemon's message alone.
- A port forward whose ssh was killed by a signal reports "Port forward process exited (code
  signal N)"; Electron printed Node's null exit code, "(code null)".
- A Web App or Browser tab's Port Forward sub-tab keeps the last 1000 lines of ssh output;
  Electron's grew without limit while the sub-tab was open.
- Opening the log of a port forward the connection no longer has shows "Failed to watch output:
  …"; Electron showed an empty log.
