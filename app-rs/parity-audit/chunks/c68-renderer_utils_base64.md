# c68-renderer_utils_base64

Verified: +2 rows, 12 rows changed

Electron files: src/renderer/utils/{base64,definitionSearch,detectMonacoLanguage,diffSearch,downloadFile,favouriteFiles,findRunningHarnessPtyId,fitTerminal,formatBytes,idleDetector,keybindingConverter,keyboardFocus}.ts (682 lines).
Rust paths are relative to the repo root. Regex alternations below are written with commas instead of pipes.

## src/renderer/utils/base64.ts

| # | Electron | Behaviour | Status | Rust parallel | Notes |
|---|---|---|---|---|---|
| c68-1 | src/renderer/utils/base64.ts:2 | `base64ToBytes`: decode the base64 that main sends raw-bytes files as (atob + charCodeAt loop) into bytes for the PDF viewer (PdfViewer.tsx:21) and 3D model viewer (ModelViewer.tsx:145) | NA | app-rs/crates/treeterm-model/src/file_browser.rs:600 (FileView::Pdf / FileView::Model from `fs.read_file`) | IPC transport encoding. Capability is ported: the native fs client receives raw bytes over gRPC and hands them to `AssetPageView::pdf` / the model viewer (file_browser_view.rs:1216) with no base64 hop. |
| c68-2 | src/renderer/utils/base64.ts:3 | Malformed base64 throws (atob) | NA | app-rs/crates/treeterm-model/src/review.rs:1383 (decode closure) | No decode step for viewer bytes in Rust. The one place Rust decodes base64 (Review image sides from `git show \| base64`) maps a decode error to `ImageSides::Failed`. |
| c68-3 | src/renderer/utils/base64.ts:15 | `decodeBase64Utf8`: a raw-bytes file (3D model) read for a text diff side is decoded back to the UTF-8 text `git show` would print (gitClient.ts:377) | PORTED | app-rs/crates/treeterm-core/src/git/mod.rs:196 (read_diff_side, DiffSideEncoding::Text) | Rust reads raw bytes and decodes with `String::from_utf8_lossy`; test git/tests.rs:1542 `diffs_a_working_tree_3d_model_as_text...`. |
| c68-4 | src/renderer/utils/base64.ts:16 | `ignoreBOM: true`: a leading BOM is kept, as Node's `toString('utf-8')` | PORTED | app-rs/crates/treeterm-core/src/git/mod.rs:197 | `from_utf8_lossy` never strips U+FEFF. |
| c68-5 | src/renderer/utils/base64.ts:16 | Invalid UTF-8 sequences become U+FFFD | PORTED | app-rs/crates/treeterm-core/src/git/mod.rs:197 | `from_utf8_lossy` replacement semantics; the Files viewer does the same (file_browser.rs:311 text_view). |

## src/renderer/utils/definitionSearch.ts

| # | Electron | Behaviour | Status | Rust parallel | Notes |
|---|---|---|---|---|---|
| c68-6 | src/renderer/utils/definitionSearch.ts:5 | `DefinitionLocation { filePath, lineNumber }` type | PORTED | app-rs/crates/treeterm-core/src/search/definition.rs:18 (DefinitionLocation) | `file_path` workspace-relative, `line_number: u64`. |
| c68-7 | src/renderer/utils/definitionSearch.ts:12 | Escape regex metacharacters `.*+?^${}()\|[]\` in the symbol | PORTED | app-rs/crates/treeterm-core/src/search/definition.rs:28 (escape_regex) | Same character set; test :367. |
| c68-8 | src/renderer/utils/definitionSearch.ts:15 | typescript / javascript pattern: (function, const, let, var, class, interface, type, enum)\s+SYM\b | PORTED | app-rs/crates/treeterm-core/src/search/definition.rs:43 (definition_pattern) | Verbatim. |
| c68-9 | src/renderer/utils/definitionSearch.ts:18 | python pattern: (def, class)\s+SYM\b | PORTED | app-rs/crates/treeterm-core/src/search/definition.rs:46 | Verbatim. |
| c68-10 | src/renderer/utils/definitionSearch.ts:20 | rust pattern: (fn, struct, enum, trait, type, const, static, mod, impl)\s+SYM\b | PORTED | app-rs/crates/treeterm-core/src/search/definition.rs:47 | Verbatim. |
| c68-11 | src/renderer/utils/definitionSearch.ts:22 | go pattern: (func, type, var, const)\s+SYM\b | PORTED | app-rs/crates/treeterm-core/src/search/definition.rs:48 | Verbatim. |
| c68-12 | src/renderer/utils/definitionSearch.ts:24 | java / kotlin pattern: (class, interface, enum, void, public, private, protected)\s+.*SYM\b | PORTED | app-rs/crates/treeterm-core/src/search/definition.rs:49 | Verbatim. |
| c68-13 | src/renderer/utils/definitionSearch.ts:27 | ruby pattern: (def, class, module)\s+SYM\b | PORTED | app-rs/crates/treeterm-core/src/search/definition.rs:52 | Verbatim. |
| c68-14 | src/renderer/utils/definitionSearch.ts:29 | php pattern: (function, class, interface, trait)\s+SYM\b | PORTED | app-rs/crates/treeterm-core/src/search/definition.rs:53 | Verbatim. |
| c68-15 | src/renderer/utils/definitionSearch.ts:31 | default pattern: (function, class, def, fn, type, interface, struct, enum)\s+SYM\b | PORTED | app-rs/crates/treeterm-core/src/search/definition.rs:54 | Verbatim. Which languages reach the search at all is gated by the caller (file_browser.rs:51 DEFINITION_LANGUAGES, mirroring monaco-config.ts registration). |
| c68-16 | src/renderer/utils/definitionSearch.ts:39 | typescript globs: *.ts, *.tsx | PORTED | app-rs/crates/treeterm-core/src/search/definition.rs:61 (file_globs) | |
| c68-17 | src/renderer/utils/definitionSearch.ts:41 | javascript globs: *.js, *.jsx, *.mjs, *.cjs | PORTED | app-rs/crates/treeterm-core/src/search/definition.rs:62 | |
| c68-18 | src/renderer/utils/definitionSearch.ts:43 | python globs: *.py | PORTED | app-rs/crates/treeterm-core/src/search/definition.rs:63 | |
| c68-19 | src/renderer/utils/definitionSearch.ts:45 | rust globs: *.rs | PORTED | app-rs/crates/treeterm-core/src/search/definition.rs:64 | |
| c68-20 | src/renderer/utils/definitionSearch.ts:47 | go globs: *.go | PORTED | app-rs/crates/treeterm-core/src/search/definition.rs:65 | |
| c68-21 | src/renderer/utils/definitionSearch.ts:49 | java globs: *.java | PORTED | app-rs/crates/treeterm-core/src/search/definition.rs:66 | |
| c68-22 | src/renderer/utils/definitionSearch.ts:51 | kotlin globs: *.kt, *.kts | PORTED | app-rs/crates/treeterm-core/src/search/definition.rs:67 | |
| c68-23 | src/renderer/utils/definitionSearch.ts:53 | ruby globs: *.rb | PORTED | app-rs/crates/treeterm-core/src/search/definition.rs:68 | |
| c68-24 | src/renderer/utils/definitionSearch.ts:55 | php globs: *.php | PORTED | app-rs/crates/treeterm-core/src/search/definition.rs:69 | |
| c68-25 | src/renderer/utils/definitionSearch.ts:57 | css / scss / less globs: *.css, *.scss, *.less | PORTED | app-rs/crates/treeterm-core/src/search/definition.rs:70 | |
| c68-26 | src/renderer/utils/definitionSearch.ts:61 | html globs: *.html, *.htm | PORTED | app-rs/crates/treeterm-core/src/search/definition.rs:71 | |
| c68-27 | src/renderer/utils/definitionSearch.ts:63 | default: no globs (grep every file) | PORTED | app-rs/crates/treeterm-core/src/search/definition.rs:72 | Test `unknown_language_uses_no_include_globs`. |
| c68-28 | src/renderer/utils/definitionSearch.ts:72 | Skip blank grep output lines | PORTED | app-rs/crates/treeterm-core/src/search/definition.rs:99 (parse_grep_output) | |
| c68-29 | src/renderer/utils/definitionSearch.ts:74 | Line regex `^(.+?):(\d+):` (file:line:content) | PORTED | app-rs/crates/treeterm-core/src/search/definition.rs:92 (GREP_LINE) | Same regex. |
| c68-30 | src/renderer/utils/definitionSearch.ts:75 | Lines that do not match are skipped | PORTED | app-rs/crates/treeterm-core/src/search/definition.rs:101 | `captures(line)?` in filter_map. |
| c68-31 | src/renderer/utils/definitionSearch.ts:78 | Strip leading `./` from the path | PORTED | app-rs/crates/treeterm-core/src/search/definition.rs:104 | Test `strips_prefix_from_file_paths`. |
| c68-32 | src/renderer/utils/definitionSearch.ts:82 | Line number parsed base 10 | PORTED | app-rs/crates/treeterm-core/src/search/definition.rs:105 | A number overflowing u64 drops the line (JS would keep a float); unreachable in practice. |
| c68-33 | src/renderer/utils/definitionSearch.ts:103 | grep args start `-rnE <pattern>` | PORTED | app-rs/crates/treeterm-core/src/search/definition.rs:77 (grep_args) | |
| c68-34 | src/renderer/utils/definitionSearch.ts:104 | One `--include=<glob>` per language glob | PORTED | app-rs/crates/treeterm-core/src/search/definition.rs:78 | |
| c68-35 | src/renderer/utils/definitionSearch.ts:108 | Exclude dirs node_modules, .git, dist, build, target | PORTED | app-rs/crates/treeterm-core/src/search/definition.rs:25 (EXCLUDED_DIRS), :83 | Same order; test `excludes_non_source_directories_and_searches_the_workspace`. |
| c68-36 | src/renderer/utils/definitionSearch.ts:109 | Search root `.` | PORTED | app-rs/crates/treeterm-core/src/search/definition.rs:88 | |
| c68-37 | src/renderer/utils/definitionSearch.ts:114 | Run `grep` through the streaming exec API on the workspace's connection, cwd = workspace path, timeout DEFAULT_EXEC_TIMEOUT_MS (30 s) | PORTED | app-rs/crates/treeterm-core/src/search/definition.rs:112 (search_definition); app-rs/crates/treeterm-core/src/exec.rs:26 (DEFAULT_EXEC_TIMEOUT = 30 s) | (v) Ref moved to the fn line. Exec client is bound to the connection, so no connectionId argument. Rust's only caller is the Files viewer (treeterm-model/src/file_browser.rs:1034, F12); Electron also wires it into the Editor tab, see c68-170. |
| c68-38 | src/renderer/utils/definitionSearch.ts:115 | exec start reports failure: resolve `[]` (no definitions, silent) | PARTIAL | app-rs/crates/treeterm-model/src/file_browser.rs:1044 (on_definitions, DefinitionLookup::Failed); app-rs/crates/treeterm-app/src/file_browser_view.rs:1001 | Rust returns `Err(ExecError::Api)` and shows "Could not look up {symbol}: {error}" in red instead of nothing / Monaco's no-definition result. Called out as intended in definition.rs:4, not in README's deliberate list. |
| c68-39 | src/renderer/utils/definitionSearch.ts:121 | Stdout events accumulated | PORTED | app-rs/crates/treeterm-core/src/exec.rs:239 (collect_output) | |
| c68-40 | src/renderer/utils/definitionSearch.ts:123 | Exit event: parse whatever stdout was printed, whatever the exit code | PORTED | app-rs/crates/treeterm-core/src/search/definition.rs:126 | Exit 1 (no match) and 2 (unreadable file, matches still printed) both parse; tests :321, :327. |
| c68-41 | src/renderer/utils/definitionSearch.ts:126 | Error event: resolve `[]` (silent) | PARTIAL | app-rs/crates/treeterm-model/src/file_browser.rs:1044; app-rs/crates/treeterm-app/src/file_browser_view.rs:1001 | Rust surfaces it as DefinitionLookup::Failed ("Could not look up ..."); test `fails_on_error_event`. |
| c68-42 | src/renderer/utils/definitionSearch.ts:131 | exec start rejects (throws): resolve `[]` (silent) | PARTIAL | app-rs/crates/treeterm-model/src/file_browser.rs:1044 | Rust shows the error; a stream ending without an exit is also an error (`ExecError::EndedWithoutResult`, test `fails_when_the_stream_ends_without_a_result`). |
| c68-43 | src/renderer/utils/definitionSearch.ts:120 | Unsubscribe the exec listener on Exit / Error | NA | app-rs/crates/treeterm-core/src/exec.rs:231 | Listener bookkeeping; the Rust stream is consumed and dropped. |
| c68-44 | src/renderer/utils/definitionSearch.ts:121 | Stderr output ignored (only Stdout handled) | PORTED | app-rs/crates/treeterm-core/src/search/definition.rs:126 | Stderr collected but unused. |
| c68-170 | src/renderer/utils/definitionSearch.ts:93 | searchDefinition is the go-to-definition backend of both Monaco views: the Files viewer (FileViewer.tsx:294) and the editable Editor tab (FileEditor.tsx:160, F12 / Cmd+click; another file opens in an Editor tab at the line) | PARTIAL | app-rs/crates/treeterm-model/src/file_browser.rs:1014 (go_to_definition); app-rs/crates/treeterm-app/src/file_browser_view.rs:55 (F12 in the viewer) | (v) Added. Rust calls search_definition only from the Files viewer; the Editor tab (treeterm-app/src/text_editor_view.rs, treeterm-model/src/text_editor.rs) can be opened at a definition's line but has no go-to-definition action of its own. Searched `go_to_definition`, `GoToDefinition`, `definition` in text_editor*.rs. Also belongs to the FileEditor chunk. |

## src/renderer/utils/detectMonacoLanguage.ts

| # | Electron | Behaviour | Status | Rust parallel | Notes |
|---|---|---|---|---|---|
| c68-45 | src/renderer/utils/detectMonacoLanguage.ts:2 | ts, tsx -> typescript; js, jsx -> javascript | PORTED | app-rs/crates/treeterm-core/src/files/language.rs:10 (detect_monaco_language) | |
| c68-46 | src/renderer/utils/detectMonacoLanguage.ts:6 | json; md -> markdown; css; scss; less; html, htm, vue, svelte -> html; xml | PORTED | app-rs/crates/treeterm-core/src/files/language.rs:12 | `markdown` also drives the Markdown preview (file_browser_view.rs:1108). |
| c68-47 | src/renderer/utils/detectMonacoLanguage.ts:16 | py -> python; rs -> rust; go; java; cs -> csharp; c, h -> c; cpp, hpp -> cpp | PORTED | app-rs/crates/treeterm-core/src/files/language.rs:19 | |
| c68-48 | src/renderer/utils/detectMonacoLanguage.ts:25 | yaml, yml -> yaml; toml; sh, bash, zsh -> shell; sql; graphql, gql -> graphql | PORTED | app-rs/crates/treeterm-core/src/files/language.rs:26 | |
| c68-49 | src/renderer/utils/detectMonacoLanguage.ts:34 | rb -> ruby; php; swift; kt, kts -> kotlin; scala; r; lua; dockerfile | PORTED | app-rs/crates/treeterm-core/src/files/language.rs:31 | Test `detects_each_extension` covers every entry. |
| c68-50 | src/renderer/utils/detectMonacoLanguage.ts:46 | Basename taken after the last `/` or `\` | PORTED | app-rs/crates/treeterm-core/src/files/language.rs:5 | Test with a Windows path. |
| c68-51 | src/renderer/utils/detectMonacoLanguage.ts:46 | `?? filePath` fallback when there is no basename | PORTED | app-rs/crates/treeterm-core/src/files/language.rs:5 | `unwrap_or(file_path)`. |
| c68-52 | src/renderer/utils/detectMonacoLanguage.ts:48 | No `.` in the name -> plaintext | PORTED | app-rs/crates/treeterm-core/src/files/language.rs:6 | `rsplit_once('.')` None. |
| c68-53 | src/renderer/utils/detectMonacoLanguage.ts:50 | Extension compared lowercased | PORTED | app-rs/crates/treeterm-core/src/files/language.rs:9 | |
| c68-54 | src/renderer/utils/detectMonacoLanguage.ts:51 | Unknown extension -> plaintext | PORTED | app-rs/crates/treeterm-core/src/files/language.rs:39 | |

## src/renderer/utils/diffSearch.ts

Live path in Rust: Review's find bar searches the review patch with `review_find::find_in_patch` (rows laid out as GPUI Kit's diff shows them); `treeterm_core::search::diff` holds the direct port (`SearchableDiff`, `find_lines_in_diff`), whose `LineMatcher`, `find_in_text` and `word_at` are used by the live code.

| # | Electron | Behaviour | Status | Rust parallel | Notes |
|---|---|---|---|---|---|
| c68-55 | src/renderer/utils/diffSearch.ts:11 | `DiffSearchSide` Additions / Deletions (values match Pierre's SelectionSide) | PORTED | app-rs/crates/treeterm-core/src/search/diff.rs:23 (DiffSearchSide); app-rs/crates/treeterm-app/src/review_view/find.rs:31 (Side -> Kit DiffSide) | Live review uses `review::Side` Modified / Original. |
| c68-56 | src/renderer/utils/diffSearch.ts:16 | `DiffSearchHit { side, lineNumber }` | PORTED | app-rs/crates/treeterm-model/src/review_find.rs:29 (DiffMatch) | DiffMatch also carries path and preview line. |
| c68-57 | src/renderer/utils/diffSearch.ts:22 | Option `diffStyle` split / unified | PORTED | app-rs/crates/treeterm-model/src/review_find.rs:22 (DiffLayout.style); app-rs/crates/treeterm-app/src/review_view/find.rs:48 | From the view's split toggle. |
| c68-58 | src/renderer/utils/diffSearch.ts:23 | Option `expandUnchanged` | PORTED | app-rs/crates/treeterm-model/src/review_find.rs:22 (DiffLayout.unchanged); app-rs/crates/treeterm-app/src/review_view/find.rs:49 | From `hide_unchanged`. |
| c68-59 | src/renderer/utils/diffSearch.ts:24 | Option `ignoreWhitespace` (rows parsed with whitespace ignored) | PORTED | app-rs/crates/treeterm-model/src/review.rs:601 (set_ignore_whitespace), :988 (diff_request `-w`) | The searched patch is reloaded with `-w`; direct port has `Whitespace::Ignore` (search/line_diff.rs:14). |
| c68-60 | src/renderer/utils/diffSearch.ts:33 | Rows cached per contents object (WeakMap) and reused across keystrokes; a refreshed file gets a new object | NA | app-rs/crates/treeterm-model/src/review.rs:1514 (find_matches); app-rs/crates/treeterm-core/src/search/diff.rs:158 (SearchableDiff) | Performance cache, no user-visible effect. Rust's live path re-scans git's patch text per render (no diff computation); SearchableDiff is the keep-and-reuse form. |
| c68-61 | src/renderer/utils/diffSearch.ts:90 | Cache key `filePath, diffStyle, expandUnchanged, ignoreWhitespace`; build rows on miss | NA | app-rs/crates/treeterm-core/src/search/diff.rs:163 | Same as c68-60. |
| c68-62 | src/renderer/utils/diffSearch.ts:38 | Build rows from the same hunks the viewer shows (`parseDiffFromFile`, jsdiff grouping, 4 context lines) | PORTED | app-rs/crates/treeterm-model/src/review_find.rs:87 (parse), :152 (visible_pairs), :15 (DIFF_CONTEXT = 4); app-rs/crates/treeterm-core/src/search/line_diff.rs:11 (CONTEXT_LINES = 4) | Review patch is `-U1000000` (review.rs:281) and folded as the Kit folds it: 4 lines around changes; equivalent to Pierre's hunks. |
| c68-63 | src/renderer/utils/diffSearch.ts:43 | Parse throws -> no rows | PORTED | app-rs/crates/treeterm-model/src/review_find.rs:87 | Neither `line_diff` nor `review_find::parse` can fail (an unparsable hunk start defaults to 0). |
| c68-64 | src/renderer/utils/diffSearch.ts:46 | No hunks (identical sides) -> no rows | PORTED | app-rs/crates/treeterm-core/src/search/diff.rs:165; app-rs/crates/treeterm-model/src/review_find.rs:257 | A file absent from the patch yields nothing. |
| c68-65 | src/renderer/utils/diffSearch.ts:49 | pushAddition: skip an index with no line | PORTED | app-rs/crates/treeterm-core/src/search/diff.rs:175 | `lines.get(index)`. |
| c68-66 | src/renderer/utils/diffSearch.ts:53 | pushDeletion: skip an index with no line | PORTED | app-rs/crates/treeterm-core/src/search/diff.rs:175 | |
| c68-67 | src/renderer/utils/diffSearch.ts:60 | Gap between hunks skipped when collapsed and longer than DEFAULT_COLLAPSED_CONTEXT_THRESHOLD (1): folded lines are not found | PORTED | app-rs/crates/treeterm-core/src/search/diff.rs:19, :188; app-rs/crates/treeterm-model/src/review_find.rs:18 (MIN_COLLAPSED_LINES = 2), :190 | Pierre's threshold is 1 (checked in node_modules/@pierre/diffs constants.js); the Kit shows a fold of fewer than 2 lines; same rows. |
| c68-68 | src/renderer/utils/diffSearch.ts:61 | Shown gap lines (expanded, or gap too small) are searched on the additions side | PORTED | app-rs/crates/treeterm-core/src/search/diff.rs:191; app-rs/crates/treeterm-model/src/review_find.rs:160, :209 | Unchanged rows -> Side::Modified. |
| c68-69 | src/renderer/utils/diffSearch.ts:69 | Context block lines searched on the additions side | PORTED | app-rs/crates/treeterm-core/src/search/diff.rs:202; app-rs/crates/treeterm-model/src/review_find.rs:209 | |
| c68-70 | src/renderer/utils/diffSearch.ts:72 | Split style: nth deletion then nth addition, row by row | PORTED | app-rs/crates/treeterm-core/src/search/diff.rs:217; app-rs/crates/treeterm-model/src/review_find.rs:220 | |
| c68-71 | src/renderer/utils/diffSearch.ts:78 | Unified style: a change's deletions, then its additions | PORTED | app-rs/crates/treeterm-core/src/search/diff.rs:227; app-rs/crates/treeterm-model/src/review_find.rs:228 | |
| c68-72 | src/renderer/utils/diffSearch.ts:66 | Gap before each hunk handled; cursor advances past each hunk | PORTED | app-rs/crates/treeterm-core/src/search/diff.rs:199, :238 | |
| c68-73 | src/renderer/utils/diffSearch.ts:85 | Trailing gap after the last hunk | PORTED | app-rs/crates/treeterm-core/src/search/diff.rs:240; app-rs/crates/treeterm-model/src/review_find.rs:181 | visible_pairs covers the tail. |
| c68-74 | src/renderer/utils/diffSearch.ts:107 | Smart case: a query with any capital matches exactly (substring) | PORTED | app-rs/crates/treeterm-core/src/search/diff.rs:72 (case_of), :97 (LineMatcher::matches) | |
| c68-75 | src/renderer/utils/diffSearch.ts:108 | All-lowercase query matches case-insensitively | PORTED | app-rs/crates/treeterm-core/src/search/diff.rs:98 | |
| c68-76 | src/renderer/utils/diffSearch.ts:111 | `TextSearchHit { lineNumber, column }`, 1-based (Monaco UTF-16 columns) | PORTED | app-rs/crates/treeterm-core/src/search/diff.rs:104 | Rust columns are 1-based bytes, matching its editor's byte offsets. |
| c68-77 | src/renderer/utils/diffSearch.ts:120 | findInText: empty query -> no hits | PORTED | app-rs/crates/treeterm-core/src/search/diff.rs:114 | |
| c68-78 | src/renderer/utils/diffSearch.ts:121 | findInText smart case (lowercase each line when the query is lowercase) | PORTED | app-rs/crates/treeterm-core/src/search/diff.rs:117 | Rust maps lowered offsets back to the original line (a length-changing lowercase still yields correct columns). |
| c68-79 | src/renderer/utils/diffSearch.ts:123 | Every non-overlapping occurrence on every line (resume after the match) | PORTED | app-rs/crates/treeterm-core/src/search/diff.rs:136; app-rs/crates/treeterm-model/src/file_browser.rs:817 | Used by the Files viewer's "in file" search, as FileViewer.tsx:629. |
| c68-80 | src/renderer/utils/diffSearch.ts:139 | findLinesInDiff: empty query -> no hits | PORTED | app-rs/crates/treeterm-model/src/review_find.rs:250; app-rs/crates/treeterm-core/src/search/diff.rs:269 | |
| c68-81 | src/renderer/utils/diffSearch.ts:139 | Image and binary diffs have no lines to search | PORTED | app-rs/crates/treeterm-core/src/search/diff.rs:272 | Live path: such files have no hunks in the patch. |
| c68-82 | src/renderer/utils/diffSearch.ts:140 | Keep rows matching the query, in on-screen order, as `{ side, lineNumber }` | PORTED | app-rs/crates/treeterm-model/src/review_find.rs:255 (find_in_patch) | |
| c68-83 | src/renderer/utils/diffSearch.ts:146 | Word character class `[\p{L}\p{N}_$]` (Unicode) | PORTED | app-rs/crates/treeterm-core/src/search/diff.rs:278 (WORD_CHAR) | |
| c68-84 | src/renderer/utils/diffSearch.ts:151 | wordAt scans back over word characters | PORTED | app-rs/crates/treeterm-core/src/search/diff.rs:291 (word_at) | |
| c68-85 | src/renderer/utils/diffSearch.ts:153 | wordAt scans forward over word characters | PORTED | app-rs/crates/treeterm-core/src/search/diff.rs:297 | |
| c68-86 | src/renderer/utils/diffSearch.ts:154 | Offset outside a word -> empty string | PORTED | app-rs/crates/treeterm-core/src/search/diff.rs:288 | Also empty off a char boundary. Rust uses it for the search panel's word at cursor and go to definition (file_browser.rs:735, :1018). |
| c68-87 | src/renderer/utils/diffSearch.ts:148 | wordAt's Electron use: the word under the pointer becomes the diff's right-click "Search" term and, with no selection, the "Copy" text (StackedDiffList.tsx:52-80 contextTarget) | PARTIAL | app-rs/crates/treeterm-app/src/review_view/find.rs:38 (search_text), :172 (with_diff_menu) | (v) Note widened: without a selection Rust shows no diff menu at all (neither Search nor Copy of the word under the pointer); a one-line selection (double-click a word) is needed. Deliberate (README, Appearance: "Review's find"). |

## src/renderer/utils/downloadFile.ts

| # | Electron | Behaviour | Status | Rust parallel | Notes |
|---|---|---|---|---|---|
| c68-88 | src/renderer/utils/downloadFile.ts:9 | `downloadBytes`: Save dialog, file lands on this computer even for a remote workspace | PORTED | app-rs/crates/treeterm-app/src/file_browser_view.rs:1087 (prompt_for_new_path); app-rs/crates/treeterm-model/src/file_browser.rs:430 (download) | Bytes read through the daemon fs, written locally with std::fs::write. |
| c68-89 | src/renderer/utils/downloadFile.ts:13 | Suggested file name = the file's name | PORTED | app-rs/crates/treeterm-app/src/file_browser_view.rs:1085 | Basename of the path; dialog opens in Downloads (else home), file_browser_view.rs:1086. |
| c68-90 | src/renderer/utils/downloadFile.ts:10 | Typed `application/octet-stream` so the name is never adjusted (no `.txt` added to a Makefile) | PORTED | app-rs/crates/treeterm-model/src/file_browser.rs:438 | Written verbatim to the chosen path. |
| c68-91 | src/renderer/utils/downloadFile.ts:16 | Object URL revoked right after the click | NA | | Browser blob lifecycle. |

## src/renderer/utils/favouriteFiles.ts

| # | Electron | Behaviour | Status | Rust parallel | Notes |
|---|---|---|---|---|---|
| c68-92 | src/renderer/utils/favouriteFiles.ts:4 | `FavouriteFile { path, relativePath }` | PORTED | app-rs/crates/treeterm-core/src/files/favourites.rs:19 (FavouriteFile) | |
| c68-93 | src/renderer/utils/favouriteFiles.ts:12 | `favouriteFileName`: list label is the basename of the relative path | PORTED | app-rs/crates/treeterm-core/src/files/favourites.rs:54 (favourite_file_name) | Used by file_browser_view.rs:935 and review_view/files.rs:76. |
| c68-94 | src/renderer/utils/favouriteFiles.ts:13 | Root-level favourite (no `/`) kept whole | PORTED | app-rs/crates/treeterm-core/src/files/favourites.rs:58 | Test `keeps_a_root_level_path_unchanged`. |
| c68-95 | src/renderer/utils/favouriteFiles.ts:9 | Full relative path kept as the row's tooltip | PARTIAL | app-rs/crates/treeterm-app/src/file_browser_view.rs:937 (Files tree tooltip); app-rs/crates/treeterm-app/src/review_view/files.rs:76 | (v) Re-checked both sides: Files tree has the tooltip; Review's favourites rows (files.rs:76-107) have none, while Electron ReviewBrowser.tsx:1513 sets `title={file.relativePath}`. Two favourites with the same basename (e.g. two `index.ts`) cannot be told apart in Review. |
| c68-96 | src/renderer/utils/favouriteFiles.ts:21 | `isCancelled` callback (default never) | PORTED | app-rs/crates/treeterm-core/src/files/favourites.rs:79 | Cancellation = dropping the future (file_browser.rs:675 replaces `_favourites`); test `stops_scheduling_directory_reads_after_cancellation`. |
| c68-97 | src/renderer/utils/favouriteFiles.ts:23 | Files keyed by relative path: overlapping file and directory favourites listed once | PORTED | app-rs/crates/treeterm-core/src/files/favourites.rs:86 (IndexMap) | Test `deduplicates_overlapping_file_and_directory_favourites`. |
| c68-98 | src/renderer/utils/favouriteFiles.ts:24 | Visited-directory set | PORTED | app-rs/crates/treeterm-core/src/files/favourites.rs:87 | |
| c68-99 | src/renderer/utils/favouriteFiles.ts:27 | visitEntry stops when cancelled | PORTED | app-rs/crates/treeterm-core/src/files/favourites.rs:79 | Via drop. |
| c68-100 | src/renderer/utils/favouriteFiles.ts:28 | A file entry is recorded with its relative path normalized (Windows separators -> `/`) | PORTED | app-rs/crates/treeterm-core/src/files/favourites.rs:100, :36 (normalize_file_entry_relative_path) | Test `windows_relative_paths_use_forward_slashes`. |
| c68-101 | src/renderer/utils/favouriteFiles.ts:33 | A directory already visited is skipped | PORTED | app-rs/crates/treeterm-core/src/files/favourites.rs:112 | |
| c68-102 | src/renderer/utils/favouriteFiles.ts:35 | Read the directory; stop if cancelled meanwhile | PORTED | app-rs/crates/treeterm-core/src/files/favourites.rs:115 | |
| c68-103 | src/renderer/utils/favouriteFiles.ts:37 | Read failure throws the daemon's error, else `Failed to load <relative path>` | PORTED | app-rs/crates/treeterm-core/src/files/favourites.rs:118, :68 (load_message) | Test `fails_loudly_when_a_favourite_directory_cannot_be_read`. |
| c68-104 | src/renderer/utils/favouriteFiles.ts:41 | Children walked one at a time, depth first in listing order (no RPC flood); cancellation checked between children | PORTED | app-rs/crates/treeterm-core/src/files/favourites.rs:98 | VecDeque with reversed push_front keeps listing order. |
| c68-105 | src/renderer/utils/favouriteFiles.ts:48 | resolveEntry: split the favourite on `/` and walk down from the workspace root | PORTED | app-rs/crates/treeterm-core/src/files/favourites.rs:134 (resolve_entry) | (v) Ref fixed to the fn line (was :139). |
| c68-106 | src/renderer/utils/favouriteFiles.ts:51 | Cancellation checked before and after each read | PORTED | app-rs/crates/treeterm-core/src/files/favourites.rs:142 | Via drop. |
| c68-107 | src/renderer/utils/favouriteFiles.ts:54 | Read failure throws the daemon's error, else `Failed to load <favourite>` | PORTED | app-rs/crates/treeterm-core/src/files/favourites.rs:145 | |
| c68-108 | src/renderer/utils/favouriteFiles.ts:56 | Child with that name not found -> no entry | PORTED | app-rs/crates/treeterm-core/src/files/favourites.rs:149 | |
| c68-109 | src/renderer/utils/favouriteFiles.ts:58 | Last segment -> that entry (file or directory) | PORTED | app-rs/crates/treeterm-core/src/files/favourites.rs:156 | |
| c68-110 | src/renderer/utils/favouriteFiles.ts:59 | Intermediate segment that is not a directory -> no entry | PORTED | app-rs/crates/treeterm-core/src/files/favourites.rs:159 | |
| c68-111 | src/renderer/utils/favouriteFiles.ts:66 | Favourite loop stops when cancelled | PORTED | app-rs/crates/treeterm-core/src/files/favourites.rs:89 | Via drop. |
| c68-112 | src/renderer/utils/favouriteFiles.ts:67 | Favourite path normalized (`./`, leading `/`, trailing `/`); empty -> skipped | PORTED | app-rs/crates/treeterm-core/src/files/favourites.rs:90, :27 (normalize_favourite_path) | Test `normalizes_favourite_paths_and_skips_empty_ones`. |
| c68-113 | src/renderer/utils/favouriteFiles.ts:69 | A favourite absent here (inherited by a child branch) is skipped; the rest still load | PORTED | app-rs/crates/treeterm-core/src/files/favourites.rs:94 | Test `ignores_inherited_paths_whose_file_or_parent_directory_does_not_exist...`. |
| c68-114 | src/renderer/utils/favouriteFiles.ts:75 | Result sorted by `relativePath.localeCompare` (ICU collation) | PARTIAL | app-rs/crates/treeterm-core/src/files/favourites.rs:169 (locale_compare) | (v) Confirmed by running both orders: ICU gives `src_old/a.ts`, `src-x/a.ts`, `src/a.ts`, `src1/a.ts` and `éa` before `fa`; Rust's case-insensitive code-point order gives `src-x`, `src/`, `src1`, `src_old` and puts `éa` after `zed`. Case handling (lowercase first on ties) matches. |
| c68-115 | src/renderer/utils/favouriteFiles.ts:37 | Errors reject the promise so the list shows the failure | PORTED | app-rs/crates/treeterm-model/src/file_browser.rs:678 (Favourites::Failed); app-rs/crates/treeterm-model/src/review.rs:1055 | |

## src/renderer/utils/findRunningHarnessPtyId.ts

| # | Electron | Behaviour | Status | Rust parallel | Notes |
|---|---|---|---|---|---|
| c68-116 | src/renderer/utils/findRunningHarnessPtyId.ts:4 | `RunningHarness { ptyId, ptyHandle, tabId }` | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:424 (running_harness -> (tab id, TerminalModel)) | |
| c68-117 | src/renderer/utils/findRunningHarnessPtyId.ts:15 | First match in tab (appStates) order | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:425 | `app_states.iter().find_map`. |
| c68-118 | src/renderer/utils/findRunningHarnessPtyId.ts:17 | Tab's applicationId starts with `aiharness-` | PARTIAL | app-rs/crates/treeterm-model/src/workspace_store.rs:426 (running_harness) | (v) Note rewritten: Rust looks the id up in the app registry and requires AppKind::AiHarness. A tab whose harness instance was removed from settings is never attached in Rust at all (workspace_store.rs:702 init_tab: "not ported: the tab stays as the other app left it"), so Review hides the prompt buttons for it; Electron matched the prefix and would prompt that tab's still-running PTY. |
| c68-119 | src/renderer/utils/findRunningHarnessPtyId.ts:18 | Tab state is an AI-harness state (terminal state with `sandbox`) | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:427 | Decided by app kind; the state's shape is not inspected. |
| c68-120 | src/renderer/utils/findRunningHarnessPtyId.ts:19 | Harness has a PTY (`ptyId !== null`) | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:430 (TerminalTab::Attached) | Rust requires the terminal attached in this window; Creating / Failed tabs do not count. |
| c68-121 | src/renderer/utils/findRunningHarnessPtyId.ts:21 | `ptyHandle ?? null` returned for writing to the PTY | NA | app-rs/crates/treeterm-model/src/workspace_store.rs:400 (prompt_harness) | Rust writes through the TerminalModel (paste + Enter), no handle needed. |
| c68-122 | src/renderer/utils/findRunningHarnessPtyId.ts:24 | No running harness -> null (Review hides harness prompting) | PORTED | app-rs/crates/treeterm-model/src/workspace_store.rs:414 (has_running_harness), :401 | prompt_harness errs "No AI harness is running in this workspace"; review.rs:536 reads has_running_harness. |

## src/renderer/utils/fitTerminal.ts

| # | Electron | Behaviour | Status | Rust parallel | Notes |
|---|---|---|---|---|---|
| c68-123 | src/renderer/utils/fitTerminal.ts:16 | Terminal not mounted -> nothing | PORTED | app-rs/crates/treeterm-app/src/terminal_element.rs:299 | A size is only proposed while painting with real bounds. Electron used this for PtyViewer; Rust's TTY viewer is a full TerminalView (session_info_view/ttys.rs:64). |
| c68-124 | src/renderer/utils/fitTerminal.ts:18 | Propose cols/rows from container size and cell size | PORTED | app-rs/crates/treeterm-term/src/fit.rs:11 (propose_dimensions); app-rs/crates/treeterm-app/src/terminal_element.rs:300 | |
| c68-125 | src/renderer/utils/fitTerminal.ts:19 | No proposal (unmeasured font, no layout) -> nothing | PORTED | app-rs/crates/treeterm-term/src/fit.rs:12 | Returns None. |
| c68-126 | src/renderer/utils/fitTerminal.ts:21 | Only send a resize when cols or rows differ from the terminal's | PORTED | app-rs/crates/treeterm-model/src/terminal.rs:433 (request_size) | Also deduplicated against the pending request. |
| c68-127 | src/renderer/utils/fitTerminal.ts:5 | Does not resize locally; the daemon echoes the applied size | PORTED | app-rs/crates/treeterm-term/src/fit.rs:4; app-rs/crates/treeterm-model/src/terminal.rs:439 | |

## src/renderer/utils/formatBytes.ts

| # | Electron | Behaviour | Status | Rust parallel | Notes |
|---|---|---|---|---|---|
| c68-128 | src/renderer/utils/formatBytes.ts:2 | Under 1024 -> `N B` | PORTED | app-rs/crates/treeterm-core/src/traffic.rs:76 (format_bytes) | |
| c68-129 | src/renderer/utils/formatBytes.ts:3 | Under 1 MiB -> `x.x KB` | PORTED | app-rs/crates/treeterm-core/src/traffic.rs:77 | |
| c68-130 | src/renderer/utils/formatBytes.ts:4 | Under 1 GiB -> `x.x MB` | PORTED | app-rs/crates/treeterm-core/src/traffic.rs:78 | |
| c68-131 | src/renderer/utils/formatBytes.ts:5 | Otherwise `x.x GB` | PORTED | app-rs/crates/treeterm-core/src/traffic.rs:79 | Callers: traffic_badge.rs:27, session_info_view/monitor.rs:232. Electron's disk gauge tooltip detail (SystemMonitor.tsx:388, 468) has no Rust counterpart (monitor.rs:149); belongs to the SystemMonitor chunk. |
| c68-132 | src/renderer/utils/formatBytes.ts:9 | formatRate: round to whole bytes, then `<size>/s` | PORTED | app-rs/crates/treeterm-core/src/traffic.rs:83 (format_rate) | Negative rates clamp to 0 (cannot occur). |
| c68-169 | src/renderer/utils/formatBytes.ts:3 | `toFixed(1)` rounds an exact half up (1280 B -> `1.3 KB`, 1.25 MiB -> `1.3 MB`) | PARTIAL | app-rs/crates/treeterm-core/src/traffic.rs:70 (format_bytes) | (v) Added. Rust's `{:.1}` rounds exact ties to even: 1280 B -> `1.2 KB`, 2304 B -> `2.2 KB` (Electron `2.3 KB`); verified by running both. Only exact .x5 values differ (traffic badge text and tooltip, session monitor memory detail). |

## src/renderer/utils/idleDetector.ts

| # | Electron | Behaviour | Status | Rust parallel | Notes |
|---|---|---|---|---|---|
| c68-133 | src/renderer/utils/idleDetector.ts:3 | `IdleEdge { snapshot, idleTimeoutMs }` | PORTED | app-rs/crates/treeterm-term/src/idle.rs:12 (IdleEdge) | |
| c68-134 | src/renderer/utils/idleDetector.ts:12 | `initialSnapshot`: what is on screen at creation; first differing frame is the first change | PORTED | app-rs/crates/treeterm-term/src/idle.rs:54 (IdleDetector::new); app-rs/crates/treeterm-model/src/terminal.rs:190 | |
| c68-135 | src/renderer/utils/idleDetector.ts:14 | Timeout read at every (re)arm, so settings and unread state apply live | PORTED | app-rs/crates/treeterm-term/src/idle.rs:74; app-rs/crates/treeterm-model/src/terminal.rs:703 (detect_activity); app-rs/crates/treeterm-model/src/workspace_store.rs:606 (sync_idle_timeouts) | Workspace pushes the current timeout to its terminals on metadata/settings change. |
| c68-136 | src/renderer/utils/idleDetector.ts:22 | onActivity callback (quiet -> active) | PORTED | app-rs/crates/treeterm-model/src/terminal.rs:736 (emit_idle -> TerminalEvent::Activity) | |
| c68-137 | src/renderer/utils/idleDetector.ts:24 | onIdle callback | PORTED | app-rs/crates/treeterm-model/src/terminal.rs:736 (TerminalEvent::Idle) | |
| c68-138 | src/renderer/utils/idleDetector.ts:41 | idleTimeoutMs: workspace unread -> `terminalAnalyzer.idleDebounceUnreadMs` | PORTED | app-rs/crates/treeterm-core/src/analyzer/mod.rs:54 (idle_timeout); app-rs/crates/treeterm-model/src/workspace_store.rs:594 | Unread from workspace attention metadata (unreadable -> treated as read, logged). |
| c68-139 | src/renderer/utils/idleDetector.ts:41 | Otherwise `terminalAnalyzer.idleDebounceMs` | PORTED | app-rs/crates/treeterm-core/src/analyzer/mod.rs:58 | |
| c68-140 | src/renderer/utils/idleDetector.ts:51 | Initial state: no timer, not settled, baseline = initial screen, not reported, no forced edge, lastTimeout = current | PORTED | app-rs/crates/treeterm-term/src/idle.rs:55 | |
| c68-141 | src/renderer/utils/idleDetector.ts:63 | report(): mark burst reported, emit activity with the frame | PORTED | app-rs/crates/treeterm-term/src/idle.rs:149 (report) | |
| c68-142 | src/renderer/utils/idleDetector.ts:69 | Identical frame -> ignored (not activity) | PORTED | app-rs/crates/treeterm-term/src/idle.rs:76 | |
| c68-143 | src/renderer/utils/idleDetector.ts:70 | Burst running: cancel timer; still changing one timeout after it began -> report (long-burst rule, also for the first burst) | PORTED | app-rs/crates/treeterm-term/src/idle.rs:79 | |
| c68-144 | src/renderer/utils/idleDetector.ts:73 | No burst running: start one (baseline = previous screen, start time now, not reported) | PORTED | app-rs/crates/treeterm-term/src/idle.rs:83 | |
| c68-145 | src/renderer/utils/idleDetector.ts:78 | Record the frame, capture the timeout at arming, arm the timer | PORTED | app-rs/crates/treeterm-term/src/idle.rs:88; app-rs/crates/treeterm-model/src/terminal.rs:714 (schedule_idle_timer) | |
| c68-146 | src/renderer/utils/idleDetector.ts:82 | Timer fires: clear it, remember whether settled, settle, consume the forced flag | PORTED | app-rs/crates/treeterm-term/src/idle.rs:107 | Early or stale fires ignored (idle.rs:101). |
| c68-147 | src/renderer/utils/idleDetector.ts:88 | Blip (burst ended on its starting screen), not forced -> nothing (no ring, no reclassify) | PORTED | app-rs/crates/treeterm-term/src/idle.rs:112 | |
| c68-148 | src/renderer/utils/idleDetector.ts:90 | Blip but a forced edge was requested -> still onIdle | PORTED | app-rs/crates/treeterm-term/src/idle.rs:114 | |
| c68-149 | src/renderer/utils/idleDetector.ts:91 | Screen changed, detector already settled -> report activity (short-burst rule), then idle | PORTED | app-rs/crates/treeterm-term/src/idle.rs:117 | |
| c68-150 | src/renderer/utils/idleDetector.ts:91 | Screen changed on the first burst (attach replay, not settled) -> idle only, no activity | PORTED | app-rs/crates/treeterm-term/src/idle.rs:117 | Test `does_not_report_the_first_burst_as_activity...`. |
| c68-151 | src/renderer/utils/idleDetector.ts:95 | onIdle with the last frame and the armed timeout | PORTED | app-rs/crates/treeterm-term/src/idle.rs:122 | |
| c68-152 | src/renderer/utils/idleDetector.ts:100 | requestIdleEdge during a burst: force the next edge | PORTED | app-rs/crates/treeterm-term/src/idle.rs:133; app-rs/crates/treeterm-model/src/terminal.rs:367 | Analyzer Effect::RequestIdleEdge (model/analyzer.rs:247). |
| c68-153 | src/renderer/utils/idleDetector.ts:104 | requestIdleEdge with no burst: onIdle now with the last timeout | PORTED | app-rs/crates/treeterm-term/src/idle.rs:137 | |
| c68-154 | src/renderer/utils/idleDetector.ts:107 | destroy: clear the timer and the forced flag | PORTED | app-rs/crates/treeterm-term/src/idle.rs:144 (cancel); app-rs/crates/treeterm-model/src/terminal.rs:357 | In practice the detector and timer task drop with the model or on `set_idle_timeout(None)`. |

## src/renderer/utils/keybindingConverter.ts

| # | Electron | Behaviour | Status | Rust parallel | Notes |
|---|---|---|---|---|---|
| c68-155 | src/renderer/utils/keybindingConverter.ts:7 | `CommandOrControl` -> `$mod` (Cmd on macOS, Ctrl elsewhere) | PORTED | app-rs/crates/treeterm-core/src/keybinding/key.rs:392 (Shortcut::parse) | Rust also accepts `CmdOrCtrl` and any case (superset). Used for `prefixMode.prefixKey`: keybinding/settings.rs:169. |
| c68-156 | src/renderer/utils/keybindingConverter.ts:11 | Last part, when a single character, lowercased (`B` -> `b`) | PORTED | app-rs/crates/treeterm-core/src/keybinding/key.rs:202 (Key::parse) | Test key.rs:562 `lowercases_the_last_key_part_when_it_is_a_single_character`. |
| c68-157 | src/renderer/utils/keybindingConverter.ts:14 | Other parts (modifiers, named keys) passed through | PORTED | app-rs/crates/treeterm-core/src/keybinding/key.rs:387, :209 | (v) Note added: parsed case-insensitively. Electron passed unknown names to tinykeys unchanged (`Ctrl+B` or `CmdOrCtrl+B` silently never match); Rust accepts those aliases and reports an unparseable prefix key as a settings error (keybinding/settings.rs:171, defaults used, main.rs:59 keybinding_settings). Superset, not a regression. |
| c68-158 | src/renderer/utils/keybindingConverter.ts:16 | Re-joined with `+` into a tinykeys string | NA | app-rs/crates/treeterm-core/src/keybinding/key.rs:443 (to_gpui) | tinykeys format glue. |

## src/renderer/utils/keyboardFocus.ts

| # | Electron | Behaviour | Status | Rust parallel | Notes |
|---|---|---|---|---|---|
| c68-159 | src/renderer/utils/keyboardFocus.ts:2 | `WindowFocusTarget`: window focus events injected (only main.tsx touches `window`) | NA | app-rs/crates/treeterm-app/src/terminal_view.rs:107 (observe_window_activation) | Dependency-injection glue; the capability is mapped in c68-166. |
| c68-160 | src/renderer/utils/keyboardFocus.ts:9 | hasKeyboardFocus: window has OS focus and the element contains the focused element | PORTED | app-rs/crates/treeterm-app/src/terminal_view.rs:151 (sync_focus); app-rs/crates/treeterm-app/src/main.rs:49 (window_counts_as_active) | (v) Refs fixed (sync_focus :151, write_input :321). Gates terminal writes: treeterm-model/src/terminal.rs:321 (write_input drops input unless focused). |
| c68-161 | src/renderer/utils/keyboardFocus.ts:9 | No element -> not focused | PORTED | app-rs/crates/treeterm-app/src/terminal_view.rs:151 (sync_focus) | (v) Ref fixed (:151). No view, no focus handle. |
| c68-162 | src/renderer/utils/keyboardFocus.ts:23 | A focus event claims the PTY's input only when keyboard focus is confirmed | PORTED | app-rs/crates/treeterm-model/src/terminal.rs:398 (set_focused) | Claim (`input.focus()`) only when focused becomes true. |
| c68-163 | src/renderer/utils/keyboardFocus.ts:24 | Focus moving into the terminal claims its input | PORTED | app-rs/crates/treeterm-app/src/terminal_view.rs:105 (on_focus -> sync_focus); app-rs/crates/treeterm-model/src/terminal.rs:405 | |
| c68-164 | src/renderer/utils/keyboardFocus.ts:25 | A key press inside an already-focused terminal claims its input, in capture phase so the claim reaches the daemon before the key's write | MISSING | app-rs/crates/treeterm-app/src/terminal_view.rs:178 (on_key_down); app-rs/crates/treeterm-model/src/terminal.rs:398 (set_focused) | (v) Re-confirmed. The only claim (`attachment.input.focus()`, treeterm-core/src/pty.rs:179) is sent from set_focused, which returns early when the gate is already focused; its only caller is terminal_view.rs:151 sync_focus (on_focus / on_blur / window activation / render reconcile). on_key_down encodes and calls write_input with no claim. Daemon gate: daemon-rs/crates/treeterm-daemon/src/pty_manager.rs:374 write_from drops gated writes from a non-owner. Scenario: window and terminal stay focused on machine A, the user types into the same PTY on machine B (claims), returns to A and types: every keystroke is dropped until focus leaves and re-enters the terminal. Searched: `claim`, `input.focus`, `.focus()` on PtyInputHandle, `set_focused` callers, key/mouse handlers in terminal_view.rs and terminal_element.rs, git log for claim/focus. |
| c68-165 | src/renderer/utils/keyboardFocus.ts:26 | A pointer press inside the terminal claims its input (unconditionally, capture phase) | MISSING | app-rs/crates/treeterm-app/src/terminal_view.rs:256 (on_mouse_down) | (v) Re-confirmed: on_mouse_down only calls `window.focus(&self.focus)`, which fires no focus event when the terminal already has focus, so nothing is claimed; mouse reports to a TUI (also gated writes) and the click a user tries as a fix are both dropped in the c68-164 scenario. Same searches as c68-164. |
| c68-166 | src/renderer/utils/keyboardFocus.ts:27 | Window regaining OS focus with the terminal focused claims its input | PORTED | app-rs/crates/treeterm-app/src/terminal_view.rs:107 (observe_window_activation) | Deactivation marks unfocused; reactivation claims again. |
| c68-167 | src/renderer/utils/keyboardFocus.ts:28 | Claim once at setup if the terminal already has focus | PORTED | app-rs/crates/treeterm-app/src/terminal_view.rs:583 (render reconcile via defer_in) | (v) Ref fixed (was :579). |
| c68-168 | src/renderer/utils/keyboardFocus.ts:29 | dispose() removes every listener | NA | app-rs/crates/treeterm-app/src/terminal_view.rs:105 (`_subscriptions`) | Subscriptions drop with the view. |

## Gaps

- c68-164, MISSING, src/renderer/utils/keyboardFocus.ts:25 (high, Terminal): a key press in an already-focused terminal re-claims PTY input before writing; Rust claims only on focus transitions, so after another machine's attachment claimed, keystrokes in a still-focused terminal are dropped by the daemon. Nearest: app-rs/crates/treeterm-app/src/terminal_view.rs:178 (on_key_down), app-rs/crates/treeterm-model/src/terminal.rs:398 (set_focused).
- c68-165, MISSING, src/renderer/utils/keyboardFocus.ts:26 (medium, Terminal): a pointer press in the terminal re-claims PTY input; Rust's mouse-down only focuses, which is a no-op when already focused. Nearest: app-rs/crates/treeterm-app/src/terminal_view.rs:256 (on_mouse_down).
- c68-170, PARTIAL, src/renderer/utils/definitionSearch.ts:93 (medium, Files/editor): go to definition is offered in the Files viewer and the Editor tab; Rust has it only in the Files viewer. Nearest: app-rs/crates/treeterm-model/src/file_browser.rs:1014 (go_to_definition).
- c68-95, PARTIAL, src/renderer/utils/favouriteFiles.ts:9 (medium, Review/diff): favourite rows show the full relative path as tooltip; Rust's Review favourites list has none (Files tree has it). Nearest: app-rs/crates/treeterm-app/src/review_view/files.rs:76.
- c68-38, PARTIAL, src/renderer/utils/definitionSearch.ts:115 (low, Files/editor): exec start failure resolved silently to no definitions; Rust shows "Could not look up {symbol}: {error}". Nearest: app-rs/crates/treeterm-model/src/file_browser.rs:1044 (on_definitions), app-rs/crates/treeterm-app/src/file_browser_view.rs:1001. Intended per definition.rs:4, not in README.
- c68-41, PARTIAL, src/renderer/utils/definitionSearch.ts:126 (low, Files/editor): exec Error event resolved silently to no definitions; Rust shows an error. Nearest: same as c68-38.
- c68-42, PARTIAL, src/renderer/utils/definitionSearch.ts:131 (low, Files/editor): exec start rejection resolved silently; Rust shows an error (also when the stream ends without an exit or the backstop fires). Nearest: same as c68-38.
- c68-87, PARTIAL, src/renderer/utils/diffSearch.ts:148 (low, Review/diff): the word under the pointer seeds the diff's right-click Search and Copy; Rust needs a one-line selection and shows no menu without one. Nearest: app-rs/crates/treeterm-app/src/review_view/find.rs:172 (with_diff_menu). Deliberate (README, Appearance).
- c68-114, PARTIAL, src/renderer/utils/favouriteFiles.ts:75 (low, Files/editor): favourites sorted with ICU `localeCompare`; Rust's `locale_compare` orders `_`, `-`, `/`, digits and non-ASCII letters differently. Nearest: app-rs/crates/treeterm-core/src/files/favourites.rs:169.
- c68-118, PARTIAL, src/renderer/utils/findRunningHarnessPtyId.ts:17 (low, AI analyzer/harness): harness tab found by `aiharness-` id prefix; Rust requires the app in the registry (and never attaches an unregistered app's tab), so a tab of a removed harness instance is not found. Nearest: app-rs/crates/treeterm-model/src/workspace_store.rs:424 (running_harness).
- c68-169, PARTIAL, src/renderer/utils/formatBytes.ts:3 (low, Traffic/monitor): exact half values round up in Electron (`1.3 KB`) and to even in Rust (`1.2 KB`). Nearest: app-rs/crates/treeterm-core/src/traffic.rs:70 (format_bytes).
