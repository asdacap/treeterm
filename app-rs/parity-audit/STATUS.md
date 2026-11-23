# Parity fix campaign: status

> The Electron app was removed after this campaign; its last version is commit `f05be955`. The `src/…` paths in this folder name its files: the audit's line numbers are at commit 517086c (`git show 517086c:src/main/menu.ts`). The README sections cited below ("Known differences", "Appearance") are now in `DIFFERENCES.md`.

The audit (`SUMMARY.md`, `gaps.md`) found 1,210 MISSING or PARTIAL gaps between the Electron app and `app-rs/`, at commit 517086c. This file records what the fix campaign on branch `electron-rust-parity-audit` did about them (2026-11-16 to 2026-11-17, 408 commits on top of master f16f87c, not pushed). `status-by-row.md` gives the outcome of every gap, with the commits that name it and the re-audit's evidence.

## Outcome

| Outcome | Rows | high | medium | low |
|---|---|---|---|---|
| Fixed (independently re-audited) | 985 | 42 | 459 | 484 |
| Different on purpose (the native behaviour is safer or better; listed in `DIFFERENCES.md`, "Deliberate differences") | 182 | 3 | 25 | 154 |
| Deferred: needs something GPUI or GPUI Kit cannot do (`DIFFERENCES.md`, "Appearance") | 21 | 0 | 4 | 17 |
| Partly fixed | 7 | 0 | 6 | 1 |
| Not a gap on re-check (audit error or no user-visible effect) | 15 | 0 | 2 | 13 |

All 19 top gaps in `SUMMARY.md` are fixed. Two high rows (c12-73, c67-197) count as "different on purpose" for one detail only: their write flush is fixed, but after a reconnect the session keeps the workspace it had open rather than jumping to its first one. The third high row is c36-68, a lead decision listed below.

## How it was done

1. Planning. 24 planners re-checked every row against the code and decided per row: fix, keep the Rust behaviour, defer, not a gap, or needs a decision. They grouped the fixes into 173 work packages, merged to 138.
2. Implementation. Each package was implemented in its own git worktree, then reviewed by a skeptical agent, then given a fix round if the review asked for changes. A single serialized integrator cherry-picked the packages onto the branch and, before advancing, ran the full test suite, clippy and the automation build.
3. Re-audit. 24 independent verifiers checked every fixed row against the final code: 948 fixed, 20 deliberate, 44 partial, 0 missing, plus 7 regressions introduced by the campaign.
4. Follow-up rounds. Ten packages handled the 44 partial rows, the 7 regressions, two flaky tests and the README. A second re-audit then left 7 partial rows and found 5 new regressions, which a third round fixed.

## Lead decisions (reversible; tell Claude to flip any of them)

- Remove or close on a top-level workspace keeps its child worktrees, branches and registry entries on disk (c36-68, c48-41). Electron force-removes child worktrees and deletes their branches behind a plain "Remove workspace?" confirm, which loses uncommitted work.
- The attention ding stays the system bell (c13-*, c54-63, c54-69, c65-152). It is already a documented deliberate difference.
- A corrupt `ui-state.json` is moved aside as `ui-state.json.corrupt-<timestamp>` and the app starts fresh, so renames and order persist again (c64-116).
- Browser and Web App tabs no longer cancel in-tab navigation away from localhost, as in Electron. The start page, address bar and markdown links stay localhost-only, popups still open in the system browser, and the tab's proxy scheme refuses requests while the top-level page is not local (c10-141).
- Commands running on a connection that is lost are cancelled, so the daemon kills them, as in Electron (c03-15).

## Still partly fixed (7)

- c26-56, c70-35 (Review image diffs): an animated or grid AVIF, and AVIF rotate, mirror and crop. Still AVIFs match. The cause is avif-parse 2.1.0, not GPUI.
- c22-82, c22-83, c54-97, c54-98, c54-99 (Editor tab language features): the HTML, CSS and JSON language services were partly ported (completion, hover, highlights, some diagnostics, JSON formatting), and word-based suggestions match. Still missing is Monaco's TypeScript service (semantic diagnostics, IntelliSense, signature help, references, rename, formatting), which would need an embedded tsserver. CSS/HTML rename and format, and the Command Palette, are also missing. The README lists them.

## Found along the way (fixed)

- A daemon race that leaked PTY masters: a session dropped without kill kept its master open, so an in-process test daemon's shells never got SIGHUP and tests hung (48652a2c). The real daemon binary was not affected.
- A daemon bug: a killed PTY's stream could end with "Stream buffer overflow" instead of its exit (e488807e).
- Two flaky tests, the remote-connection race and the attach race, were fixed in the follow-up round. The app_store tests' leaked temp HOMEs were fixed in 53240ae9.

## Notes for the owner

- GPUI (`gpui-pre` 0.3.12) and `gpui-webview` are now vendored under `app-rs/vendor/`, like `gpui-component` already was. This was for the whole-window zoom: 10 and 4 `TreeTerm patch` markers. `avif-decode` is vendored to build rav1d without NASM. README "Vendored crates" describes each patch.
- "Enable Sandbox" is still a no-op in both apps, because the daemon ignores `CreatePtyRequest.sandbox`. The native app now sends the tab's sandbox config like Electron.
- Known remaining flake: `a_child_whose_worktree_git_has_forgotten_still_closes` (from master's f16f87c) timed out once under full-suite load. It passed 30 of 30 alone and 12 of 12 binary runs.
- About 20 orphaned `zsh -l` test shells from runs about 5 days ago (cwd `/private/tmp/tt*`, ppid 1) predate this campaign and were left alone.

## Final verification (at 6c3cf199; 53240ae9 only changes the app_store test helper, and those tests pass)

- app-rs: `cargo test --workspace`, 2,491 passed in one run. The other run had one failure, the flake above.
- daemon-rs: `cargo test`, 124 passed.
- `cargo clippy --workspace --all-targets` is clean apart from the daemon's existing tokio `AsyncFd::new` deprecation.
- `cargo build -p treeterm-app --features automation` builds.
- End-to-end: 12 of 12 passed.
- A manual sandbox smoke test showed the workspace selected at launch, the LOCAL session name, header, tab strip, Review with per-file lazy loading and diffs, and git status refresh.
