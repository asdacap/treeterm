//! TerminalModel against a real in-process daemon and a real shell.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::{AppContext as _, Entity, Subscription, TestAppContext};
use treeterm_core::daemon::DaemonConnection;
use treeterm_core::pty::CreatePty;
use treeterm_core::settings::TerminalSettings;
use treeterm_model::terminal::{
    ALT_CLICK_MOVE_CURSOR_TIME, BADGE_DOUBLE_CLICK, CellPoint, DRAG_SCROLL_INTERVAL, IdleSnapshots, LinkPress, Overlay,
    Pointer, ScrollbarVisibility, SelectionCopy, SelectionKind, TerminalEvent, TerminalModel, TerminalOptions,
    TerminalRole,
};
use treeterm_term::keys::Modifiers;
use treeterm_term::core::GridSize;
use treeterm_term::fit::WidthLimit;
use treeterm_term::idle::IdleEdge;
use treeterm_term::links::{Link, LinkKind};
use treeterm_term::palette::DEFAULT_BACKGROUND;
use treeterm_term::view::ScrollPosition;

mod common;
use common::{Harness, Proxy, harness, refusing_daemon, wait_until};

/// A login shell with an empty config dir, so the user's dotfiles stay out of the test.
fn spawn_shell(h: &Harness) -> String {
    spawn_shell_sized(h, (80, 24))
}

fn spawn_shell_sized(h: &Harness, size: (u16, u16)) -> String {
    spawn(h, size, None)
}

/// A shell that execs `cat`: nothing redraws when the PTY is resized.
fn spawn_cat(h: &Harness) -> String {
    spawn(h, (80, 24), Some("cat".to_owned()))
}

fn spawn(h: &Harness, size: (u16, u16), startup_command: Option<String>) -> String {
    let mut env = HashMap::new();
    env.insert("ZDOTDIR".to_owned(), h.daemon.root().to_string_lossy().into_owned());
    env.insert("PS1".to_owned(), "$ ".to_owned());
    futures::executor::block_on(h.conn.create_pty(CreatePty {
        cwd: h.daemon.root().to_string_lossy().into_owned(),
        env,
        size: Some(size),
        startup_command,
        ..CreatePty::default()
    }))
    .unwrap()
    .unwrap()
}

fn options(pty_id: String) -> TerminalOptions {
    TerminalOptions {
        pty_id,
        background: DEFAULT_BACKGROUND,
        settings: TerminalSettings::default(),
        role: TerminalRole::Tab { keep_on_exit: false },
        width_limit: WidthLimit::Capped(160),
        scrollbar: ScrollbarVisibility::Shown,
        selection_copy: SelectionCopy::OnCopy,
        idle_timeout: None,
        traffic: treeterm_core::traffic::ByteCounter::default(),
        show_debug_badge: false,
        shown: true,
    }
}

fn record(cx: &mut TestAppContext, model: &Entity<TerminalModel>) -> (Rc<RefCell<Vec<TerminalEvent>>>, Subscription) {
    let events = Rc::new(RefCell::new(Vec::new()));
    let sink = events.clone();
    let subscription = cx.update(|cx| {
        cx.subscribe(model, move |_, event: &TerminalEvent, _| sink.borrow_mut().push(event.clone()))
    });
    (events, subscription)
}

fn open(cx: &mut TestAppContext, h: &Harness, options: TerminalOptions) -> Entity<TerminalModel> {
    let conn = h.conn.clone();
    let model = cx.new(|cx| TerminalModel::new(conn, options, cx));
    // The prompt is whatever the system zshrc/bashrc makes it; any output means the shell is up.
    wait_until(cx, &model, |m| *m.overlay() == Overlay::Hidden && !m.screen_text().trim().is_empty());
    model
}

fn type_line(cx: &mut TestAppContext, model: &Entity<TerminalModel>, line: &str) {
    model.update(cx, |m, cx| m.write_input(format!("{line}\r").as_bytes(), cx));
}

#[gpui_kit::test]
fn shows_the_shell_and_sends_focused_input(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let pty = spawn_shell(&h);
    let model = open(cx, &h, options(pty));

    // Unfocused input is dropped, as in the Electron app.
    type_line(cx, &model, "echo unfocused-$((1+1))");
    model.update(cx, |m, cx| m.set_focused(true, cx));
    type_line(cx, &model, "echo tt-$((40+2))");
    wait_until(cx, &model, |m| m.screen_text().contains("tt-42"));
    assert!(!model.read_with(cx, |m, _| m.screen_text().contains("unfocused-2")));
}

#[gpui_kit::test]
fn output_bytes_count_toward_the_workspace_meter_and_the_connection(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let pty = spawn_shell(&h);
    let counter = treeterm_core::traffic::ByteCounter::default();
    let connection_before = h.conn.received().total();
    let model = open(cx, &h, TerminalOptions { traffic: counter.clone(), ..options(pty) });
    let after_prompt = counter.total();
    assert!(after_prompt > 0, "the prompt counts");

    model.update(cx, |m, cx| m.set_focused(true, cx));
    type_line(cx, &model, "printf 'x%.0s' $(seq 1 2000); echo done-$((1+2))");
    wait_until(cx, &model, |m| m.screen_text().contains("done-3"));
    assert!(counter.total() >= after_prompt + 2000);
    assert!(h.conn.received().total() >= connection_before + counter.total());

    let meter = cx.new(|cx| {
        treeterm_model::traffic::TrafficMeter::new(treeterm_model::traffic::MeterKind::Workspace, counter.clone(), cx)
    });
    cx.executor().advance_clock(treeterm_core::traffic::TRAFFIC_TICK);
    cx.run_until_parked();
    assert_eq!(meter.read_with(cx, |m, _| m.rate().total_bytes), counter.total());
}

#[gpui_kit::test]
fn the_grid_resizes_only_when_the_daemon_echoes(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let pty = spawn_shell(&h);
    let model = open(cx, &h, options(pty));

    let target = GridSize { cols: 100, rows: 30 };
    model.update(cx, |m, _| {
        m.fit(target);
        assert_eq!(m.grid_size(), GridSize { cols: 80, rows: 24 }, "no local resize");
    });
    wait_until(cx, &model, |m| m.grid_size() == target);
    assert_eq!(model.read_with(cx, |m, _| m.size_mismatch()), None);
}

/// Type a command into `model` and wait for its output: anything the daemon sent before
/// it has arrived by then.
fn round_trip(cx: &mut TestAppContext, model: &Entity<TerminalModel>) {
    model.update(cx, |m, cx| m.set_focused(true, cx));
    type_line(cx, model, "echo trip-$((20+3))");
    wait_until(cx, model, |m| m.screen_text().contains("trip-23"));
}

fn mismatch(cx: &mut TestAppContext, model: &Entity<TerminalModel>) -> Option<(GridSize, GridSize)> {
    model.read_with(cx, |m, _| m.size_mismatch().map(|s| (s.requested, s.actual)))
}

const A_SIZE: GridSize = GridSize { cols: 100, rows: 30 };
const B_SIZE: GridSize = GridSize { cols: 90, rows: 20 };

/// Two views A and B of one shell: A fitted it to [`A_SIZE`], then B to [`B_SIZE`].
fn resized_by_another(cx: &mut TestAppContext, h: &Harness) -> (Entity<TerminalModel>, Entity<TerminalModel>) {
    let pty = spawn_shell(h);
    let a = open(cx, h, options(pty.clone()));
    let b = open(cx, h, options(pty));
    a.update(cx, |m, _| m.fit(A_SIZE));
    wait_until(cx, &a, |m| m.grid_size() == A_SIZE);
    wait_until(cx, &b, |m| m.grid_size() == A_SIZE);
    assert_eq!(mismatch(cx, &a), None);
    b.update(cx, |m, _| m.fit(B_SIZE));
    wait_until(cx, &a, |m| m.grid_size() == B_SIZE);
    wait_until(cx, &b, |m| m.grid_size() == B_SIZE);
    (a, b)
}

#[gpui_kit::test]
fn no_fit_is_asked_for_before_the_daemon_reports_the_size(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let pty = spawn_shell_sized(&h, (100, 30));
    let conn = h.conn.clone();
    let model = cx.new(|cx| TerminalModel::new(conn, options(pty), cx));

    // The grid is still the 80x24 placeholder: a fit compared with it would ask for nothing,
    // and the view would keep the PTY's 100x30 for good.
    let fits_the_placeholder = GridSize { cols: 80, rows: 24 };
    model.update(cx, |m, _| m.fit(fits_the_placeholder));
    wait_until(cx, &model, |m| m.grid_size() == GridSize { cols: 100, rows: 30 });
    model.update(cx, |m, _| m.fit(fits_the_placeholder));
    wait_until(cx, &model, |m| m.grid_size() == fits_the_placeholder);
}

#[gpui_kit::test]
fn a_foreign_resize_is_kept_and_flagged(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let (a, b) = resized_by_another(cx, &h);

    // A flags the size another attachment set, with the one it had asked for.
    assert_eq!(mismatch(cx, &a), Some((A_SIZE, B_SIZE)));
    assert_eq!(mismatch(cx, &b), None);

    // A's fit is unchanged, so A does not take the PTY back however often it lays out.
    a.update(cx, |m, _| m.fit(A_SIZE));
    round_trip(cx, &b);
    assert_eq!(a.read_with(cx, |m, _| m.grid_size()), B_SIZE);
    assert_eq!(b.read_with(cx, |m, _| m.grid_size()), B_SIZE);

    // A fit of its own changing does, and then B is the one flagging.
    let wider = GridSize { cols: 110, rows: 30 };
    a.update(cx, |m, _| m.fit(wider));
    wait_until(cx, &a, |m| m.grid_size() == wider);
    wait_until(cx, &b, |m| m.grid_size() == wider);
    assert_eq!(mismatch(cx, &a), None);
    assert_eq!(mismatch(cx, &b), Some((B_SIZE, wider)));
}

#[gpui_kit::test]
fn a_view_shown_again_takes_the_pty_back(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let (a, b) = resized_by_another(cx, &h);

    // The badge goes, and the unchanged fit is asked for again.
    a.update(cx, |m, cx| m.refit(cx));
    assert_eq!(mismatch(cx, &a), None);
    a.update(cx, |m, _| m.fit(A_SIZE));
    wait_until(cx, &a, |m| m.grid_size() == A_SIZE);
    wait_until(cx, &b, |m| m.grid_size() == A_SIZE);
    assert_eq!(mismatch(cx, &a), None);
    assert_eq!(mismatch(cx, &b), Some((B_SIZE, A_SIZE)));

    // Shown again with a fit the grid already has: nothing is asked for, so a size another
    // attachment sets later is not flagged (Electron's requestedSize starts empty).
    a.update(cx, |m, cx| m.refit(cx));
    a.update(cx, |m, _| m.fit(A_SIZE));
    b.update(cx, |m, cx| m.refit(cx));
    b.update(cx, |m, _| m.fit(B_SIZE));
    wait_until(cx, &a, |m| m.grid_size() == B_SIZE);
    assert_eq!(mismatch(cx, &a), None);
}

#[gpui_kit::test]
fn flipping_the_width_limit_takes_the_pty_back(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    // A's 100 columns are under the 160 cap, so flipping it leaves A's fit unchanged.
    let (a, b) = resized_by_another(cx, &h);

    // Setting the limit A already has is no flip.
    a.update(cx, |m, cx| m.set_width_limit(WidthLimit::Capped(160), cx));
    a.update(cx, |m, _| m.fit(A_SIZE));
    round_trip(cx, &a);
    assert_eq!(a.read_with(cx, |m, _| m.grid_size()), B_SIZE);
    assert_eq!(mismatch(cx, &a), Some((A_SIZE, B_SIZE)));

    // A flip compares the fit with the grid again, as Electron's applyFit does.
    a.update(cx, |m, cx| m.set_width_limit(WidthLimit::Off, cx));
    a.update(cx, |m, _| m.fit(A_SIZE));
    wait_until(cx, &a, |m| m.grid_size() == A_SIZE);
    wait_until(cx, &b, |m| m.grid_size() == A_SIZE);
    assert_eq!(mismatch(cx, &a), None);
    assert_eq!(mismatch(cx, &b), Some((B_SIZE, A_SIZE)));
}

#[gpui_kit::test]
fn a_settings_change_or_a_refresh_fits_again(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let (a, b) = resized_by_another(cx, &h);

    // Electron mounts its terminals again on a settings change.
    a.update(cx, |m, cx| m.apply_settings(TerminalSettings::default(), cx));
    a.update(cx, |m, _| m.fit(A_SIZE));
    wait_until(cx, &b, |m| m.grid_size() == A_SIZE);

    // A refresh starts over from the replay's size, whatever the placeholder grid was.
    b.update(cx, |m, cx| m.refresh(cx));
    assert_eq!(mismatch(cx, &b), None);
    b.update(cx, |m, _| m.fit(B_SIZE));
    wait_until(cx, &b, |m| *m.overlay() == Overlay::Hidden && m.grid_size() == A_SIZE);
    b.update(cx, |m, _| m.fit(B_SIZE));
    wait_until(cx, &a, |m| m.grid_size() == B_SIZE);
}

#[gpui_kit::test]
fn show_raw_characters_logs_the_last_output_escaped_while_on(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let logs = treeterm_testkit::logs::capture();
    let raw_lines = || logs.text().lines().filter(|line| line.contains("[RAW] ")).count();
    let h = harness();
    let pty = spawn_shell(&h);
    let on = TerminalSettings { show_raw_chars: true, ..TerminalSettings::default() };
    let model = open(cx, &h, TerminalOptions { settings: on.clone(), ..options(pty) });
    assert!(raw_lines() > 0, "the replayed prompt is logged: {}", logs.text());

    // The echoed command line has a backslash and a `t`; only printf's output has the tab.
    // The sleep keeps the next prompt out of the output's chunk, whose line then ends with it
    // (the shell's prompt redraw alone is longer than the tail).
    model.update(cx, |m, cx| m.set_focused(true, cx));
    type_line(cx, &model, "printf 'raw\\tchars-%d' $((40+2)); sleep 0.5");
    wait_until(cx, &model, |m| m.screen_text().contains("chars-42"));
    let line = logs.line("raw\\tchars-42").unwrap_or_else(|| panic!("{}", logs.text()));
    assert!(line.contains("INFO") && line.contains("[RAW] "), "{line}");

    // Turned off: nothing more is logged.
    model.update(cx, |m, cx| m.apply_settings(TerminalSettings::default(), cx));
    let before = raw_lines();
    type_line(cx, &model, "echo off-$((1+2))");
    wait_until(cx, &model, |m| m.screen_text().contains("off-3"));
    assert_eq!(raw_lines(), before, "{}", logs.text());

    // On again: from the new output only.
    model.update(cx, |m, cx| m.apply_settings(on, cx));
    type_line(cx, &model, "echo on-$((2+3))");
    wait_until(cx, &model, |m| m.screen_text().contains("on-5"));
    assert!(raw_lines() > before, "{}", logs.text());
    assert!(logs.text().lines().filter(|line| line.contains("[RAW] ")).all(|line| !line.contains("off-3")));
}

/// Electron mounts only the shown workspace's terminals, and only a mounted one logs.
#[gpui_kit::test]
fn show_raw_characters_logs_only_while_shown_and_starts_over_when_shown_again(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let logs = treeterm_testkit::logs::capture();
    let raw_lines = || -> Vec<String> {
        logs.text().lines().filter(|line| line.contains("[RAW] ")).map(str::to_owned).collect()
    };
    let h = harness();
    // `cat` echoes what is typed and prints nothing else, so each `[RAW]` line is the tail.
    let pty = spawn_cat(&h);
    let on = TerminalSettings { show_raw_chars: true, ..TerminalSettings::default() };
    let model = open(cx, &h, TerminalOptions { settings: on, shown: false, ..options(pty) });
    wait_until(cx, &model, |m| m.screen_text().contains("exec cat"));
    model.update(cx, |m, cx| m.set_focused(true, cx));
    type_line(cx, &model, "cat-ready");
    wait_until(cx, &model, |m| m.screen_text().contains("cat-ready\ncat-ready"));
    // A tab of a workspace the window does not show: nothing is logged, its replay included.
    assert_eq!(raw_lines(), Vec::<String>::new());

    // Shown: the output from then on is logged.
    model.update(cx, |m, _| m.set_shown(true));
    model.update(cx, |m, cx| m.write_input(b"seen", cx));
    wait_until(cx, &model, |_| logs.text().contains("[RAW] seen pty="));

    // Hidden: nothing is logged.
    model.update(cx, |m, _| m.set_shown(false));
    let before = raw_lines();
    model.update(cx, |m, cx| m.write_input(b"-unseen", cx));
    wait_until(cx, &model, |m| m.screen_text().contains("seen-unseen"));
    assert_eq!(raw_lines(), before);

    // Shown again: from no output, as Electron's remount empties `rawChars`.
    model.update(cx, |m, _| m.set_shown(true));
    model.update(cx, |m, cx| m.write_input(b"+again", cx));
    wait_until(cx, &model, |_| logs.text().contains("+again pty="));
    let after = raw_lines();
    assert!(after[before.len()..].iter().all(|line| !line.contains("seen")), "{after:#?}");
    assert!(after.last().is_some_and(|line| line.contains("[RAW] +again pty=")), "{after:#?}");
}

#[gpui_kit::test]
fn the_debug_badge_follows_its_setting(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (_daemon, conn) = refusing_daemon(tonic::Status::unavailable("Connection refused"));
    let shown = TerminalOptions { show_debug_badge: true, ..options("p1".into()) };
    let model = cx.new(|cx| TerminalModel::new(conn, shown, cx));
    assert!(model.read_with(cx, |m, _| m.show_debug_badge()));
    model.update(cx, |m, cx| m.set_debug_badge(false, cx));
    assert!(!model.read_with(cx, |m, _| m.show_debug_badge()));
    model.update(cx, |m, cx| m.set_debug_badge(true, cx));
    assert!(model.read_with(cx, |m, _| m.show_debug_badge()));
}

#[gpui_kit::test]
fn an_exit_requests_closing_the_tab(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let pty = spawn_shell(&h);
    let model = open(cx, &h, options(pty));
    let (events, _subscription) = record(cx, &model);

    model.update(cx, |m, cx| m.set_focused(true, cx));
    type_line(cx, &model, "exit 0");
    wait_until(cx, &model, |m| m.exit_code().is_some());
    assert!(events.borrow().contains(&TerminalEvent::CloseRequested { exit_code: 0 }));
}

#[gpui_kit::test]
fn keep_on_exit_prints_the_exit_code_instead(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let pty = spawn_shell(&h);
    let model = open(cx, &h, TerminalOptions { role: TerminalRole::Tab { keep_on_exit: true }, ..options(pty) });
    let (events, _subscription) = record(cx, &model);

    // Past the "exited immediately" window.
    cx.executor().advance_clock(std::time::Duration::from_secs(2));
    model.update(cx, |m, cx| m.set_focused(true, cx));
    type_line(cx, &model, "exit 3");
    wait_until(cx, &model, |m| m.screen_text().contains("Process exited with exit code 3"));
    assert!(events.borrow().iter().all(|e| !matches!(e, TerminalEvent::CloseRequested { .. })));
}

#[gpui_kit::test]
fn a_failure_right_after_start_is_explained(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let pty = spawn_shell(&h);
    let model = open(cx, &h, options(pty));
    model.update(cx, |m, cx| m.set_focused(true, cx));
    type_line(cx, &model, "exit 7");
    wait_until(cx, &model, |m| *m.overlay() == Overlay::ExitedImmediately(7));
}

/// The foreground of the cell where `text` starts on screen.
fn foreground_of(model: &TerminalModel, text: &str) -> Option<treeterm_term::alacritty_terminal::vte::ansi::Color> {
    use treeterm_term::alacritty_terminal::grid::Dimensions as _;
    use treeterm_term::alacritty_terminal::index::{Column, Line};
    let term = model.term().lock();
    (0..term.screen_lines()).find_map(|line| {
        let row = &term.grid()[Line(line as i32)];
        let chars: String = (0..term.columns()).map(|column| row[Column(column)].c).collect();
        chars.find(text).map(|column| row[Column(column)].fg)
    })
}

#[gpui_kit::test]
fn a_viewer_notes_any_exit_in_grey_and_stays(cx: &mut TestAppContext) {
    use treeterm_term::alacritty_terminal::vte::ansi::{Color, NamedColor};
    cx.executor().allow_parking();
    let h = harness();
    let pty = spawn_shell(&h);
    let model = open(cx, &h, TerminalOptions { role: TerminalRole::Viewer, ..options(pty) });
    let (events, _subscription) = record(cx, &model);

    // Right after attaching, which a tab would explain as a failure to start.
    model.update(cx, |m, cx| m.set_focused(true, cx));
    type_line(cx, &model, "exit 7");
    wait_until(cx, &model, |m| m.screen_text().contains("[Process exited]"));
    model.read_with(cx, |m, _| {
        assert_eq!(m.exit_code(), Some(7));
        assert_eq!(*m.overlay(), Overlay::Hidden);
        assert_eq!(foreground_of(m, "[Process exited]"), Some(Color::Named(NamedColor::BrightBlack)));
        assert!(!m.screen_text().contains("Process exited with exit code"));
    });
    assert!(events.borrow().contains(&TerminalEvent::Exited(7)));
    assert!(events.borrow().iter().all(|e| !matches!(e, TerminalEvent::CloseRequested { .. })));
}

#[gpui_kit::test]
fn a_viewer_leaves_attaching_and_an_attach_failure_to_its_viewer(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (_daemon, conn) = refusing_daemon(tonic::Status::unavailable("Connection refused"));
    let viewer = {
        let conn = conn.clone();
        cx.new(|cx| TerminalModel::new(conn, TerminalOptions { role: TerminalRole::Viewer, ..options("p1".into()) }, cx))
    };
    let tab = cx.new(|cx| TerminalModel::new(conn, options("p1".into()), cx));
    assert_eq!(viewer.read_with(cx, |m, _| (m.overlay().clone(), m.banner())), (Overlay::Loading, Overlay::Hidden));
    assert_eq!(tab.read_with(cx, |m, _| m.banner()), Overlay::Loading);

    let failed = Overlay::AttachFailed("14 UNAVAILABLE: Connection refused".to_owned());
    wait_until(cx, &viewer, |m| *m.overlay() == failed);
    assert_eq!(viewer.read_with(cx, |m, _| m.banner()), Overlay::Hidden);
    wait_until(cx, &tab, |m| *m.overlay() == failed);
    assert_eq!(tab.read_with(cx, |m, _| m.banner()), failed);
}

#[gpui_kit::test]
fn a_viewer_still_says_when_its_stream_ends(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let conn = h.conn.clone();
    let options = TerminalOptions { role: TerminalRole::Viewer, ..options("no-such-pty".into()) };
    let model = cx.new(|cx| TerminalModel::new(conn, options, cx));
    wait_until(cx, &model, |m| *m.overlay() == Overlay::Disconnected);
    assert_eq!(model.read_with(cx, |m, _| m.banner()), Overlay::Disconnected);
}

#[gpui_kit::test]
fn refresh_rebuilds_the_screen_from_the_replay(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let pty = spawn_shell(&h);
    let model = open(cx, &h, options(pty));
    model.update(cx, |m, cx| m.set_focused(true, cx));
    type_line(cx, &model, "echo before-$((2*3))");
    wait_until(cx, &model, |m| m.screen_text().contains("before-6"));

    model.update(cx, |m, cx| m.refresh(cx));
    assert_eq!(model.read_with(cx, |m, _| m.overlay().clone()), Overlay::Loading);
    // "Loading terminal..." cannot be clicked away, as Electron's has no click handler.
    model.update(cx, |m, cx| m.dismiss_overlay(cx));
    assert_eq!(model.read_with(cx, |m, _| m.overlay().clone()), Overlay::Loading);
    wait_until(cx, &model, |m| m.screen_text().contains("before-6"));
    // Still focused after the refresh: input keeps working.
    type_line(cx, &model, "echo after-$((3*3))");
    wait_until(cx, &model, |m| m.screen_text().contains("after-9"));
}

#[gpui_kit::test]
fn an_unknown_pty_shows_disconnected(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let conn = h.conn.clone();
    let model = cx.new(|cx| TerminalModel::new(conn, options("no-such-pty".into()), cx));
    wait_until(cx, &model, |m| *m.overlay() == Overlay::Disconnected);
}

#[gpui_kit::test]
fn typing_or_pasting_after_the_stream_ended_shows_why(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let conn = h.conn.clone();
    let model = cx.new(|cx| TerminalModel::new(conn, options("no-such-pty".into()), cx));
    wait_until(cx, &model, |m| *m.overlay() == Overlay::Disconnected);
    let ended = Overlay::Error("pty stream ended".to_owned());

    // Unfocused, the keystroke is not written, so nothing fails.
    model.update(cx, |m, cx| m.write_input(b"x", cx));
    assert_eq!(model.read_with(cx, |m, _| m.overlay().clone()), Overlay::Disconnected);

    model.update(cx, |m, cx| {
        m.set_focused(true, cx);
        m.write_input(b"x", cx);
    });
    assert_eq!(model.read_with(cx, |m, _| m.overlay().clone()), ended);

    // A click clears an error banner.
    model.update(cx, |m, cx| m.dismiss_overlay(cx));
    assert_eq!(model.read_with(cx, |m, _| m.overlay().clone()), Overlay::Hidden);
    model.update(cx, |m, cx| m.paste("y", cx));
    wait_until(cx, &model, |m| *m.overlay() == ended);
}

/// Electron's `PtyViewer` only logs a write that fails: typing or pasting into a viewer
/// whose stream ended leaves its "Terminal disconnected" banner.
#[gpui_kit::test]
fn typing_or_pasting_into_a_viewer_after_its_stream_ended_keeps_its_banner(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let conn = h.conn.clone();
    let viewer = TerminalOptions { role: TerminalRole::Viewer, ..options("no-such-pty".into()) };
    let model = cx.new(|cx| TerminalModel::new(conn, viewer, cx));
    wait_until(cx, &model, |m| *m.overlay() == Overlay::Disconnected);
    let (events, _subscription) = record(cx, &model);

    model.update(cx, |m, cx| {
        m.set_focused(true, cx);
        m.write_input(b"x", cx);
        m.paste("y", cx);
    });
    // Time for the paste's failure to come back (a tab shows it: see above).
    for _ in 0..20 {
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(model.read_with(cx, |m, _| m.overlay().clone()), Overlay::Disconnected);
    assert_eq!(model.read_with(cx, |m, _| m.banner()), Overlay::Disconnected);
    // The input was tried all the same.
    assert_eq!(events.borrow().iter().filter(|e| matches!(e, TerminalEvent::Input(_))).count(), 2);
}

#[gpui_kit::test]
fn writes_after_a_stream_error_show_that_error(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let pty = spawn_shell(&h);
    let dir = tempfile::Builder::new()
        .prefix("tt")
        .tempdir_in("/tmp")
        .unwrap();
    let socket = dir.path().join("p.sock");
    let proxy = Proxy::start(&socket, h.daemon.socket_path());
    let conn = futures::executor::block_on(DaemonConnection::open(&h.rt, &socket))
        .unwrap()
        .unwrap();
    let model = cx.new(|cx| TerminalModel::new(conn, options(pty), cx));
    wait_until(cx, &model, |m| *m.overlay() == Overlay::Hidden && !m.screen_text().trim().is_empty());

    // The connection drops mid-stream: the stream's error shows as it is, not as an attach
    // failure.
    drop(proxy);
    wait_until(cx, &model, |m| matches!(m.overlay(), Overlay::Error(_)));
    let failed = model.read_with(cx, |m, _| m.overlay().clone());

    model.update(cx, |m, cx| {
        m.dismiss_overlay(cx);
        m.set_focused(true, cx);
        m.write_input(b"x", cx);
    });
    assert_eq!(model.read_with(cx, |m, _| m.overlay().clone()), failed);

    model.update(cx, |m, cx| m.dismiss_overlay(cx));
    model.update(cx, |m, cx| m.paste("y", cx));
    wait_until(cx, &model, |m| *m.overlay() == failed);
}

#[gpui_kit::test]
fn an_attach_that_fails_says_so(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (_daemon, conn) = refusing_daemon(tonic::Status::unavailable("Connection refused"));
    let model = cx.new(|cx| TerminalModel::new(conn, options("p1".into()), cx));
    let failed = Overlay::AttachFailed("14 UNAVAILABLE: Connection refused".to_owned());
    wait_until(cx, &model, |m| *m.overlay() == failed);
    assert_eq!(
        failed.message().as_deref(),
        Some("Failed to reattach terminal: 14 UNAVAILABLE: Connection refused")
    );
}

#[gpui_kit::test]
fn a_terminal_whose_attach_failed_takes_no_input(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (_daemon, conn) = refusing_daemon(tonic::Status::unavailable("Connection refused"));
    let model = cx.new(|cx| TerminalModel::new(conn, options("p1".into()), cx));
    let failed = Overlay::AttachFailed("14 UNAVAILABLE: Connection refused".to_owned());
    wait_until(cx, &model, |m| *m.overlay() == failed);
    let (events, _subscription) = record(cx, &model);

    // Electron has no tty after an attach that throws: keystrokes, mouse reports and pastes
    // go nowhere, and the attach failure keeps showing.
    model.update(cx, |m, cx| {
        m.set_focused(true, cx);
        m.write_input(b"x", cx);
        m.paste("y", cx);
    });
    cx.run_until_parked();
    assert_eq!(model.read_with(cx, |m, _| m.overlay().clone()), failed);
    assert!(!events.borrow().iter().any(|e| matches!(e, TerminalEvent::Input(_))));

    // A refresh attaches again, and that attachment takes input until it fails in turn.
    model.update(cx, |m, cx| {
        m.refresh(cx);
        m.write_input(b"x", cx);
    });
    assert!(events.borrow().iter().any(|e| matches!(e, TerminalEvent::Input(_))));
}

#[gpui_kit::test]
fn selection_copies_the_selected_text(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let pty = spawn_shell(&h);
    let model = open(cx, &h, options(pty));
    model.update(cx, |m, cx| m.set_focused(true, cx));
    type_line(cx, &model, "clear; echo select-$((5*5))");
    wait_until(cx, &model, |m| m.screen_text().lines().any(|l| l == "select-25"));

    model.update(cx, |m, cx| {
        let line = m.screen_text().lines().position(|l| l == "select-25").unwrap();
        m.start_selection(CellPoint { line, column: 0, right_half: false }, SelectionKind::Semantic, cx);
        assert_eq!(m.selection_text().as_deref(), Some("select-25"));
        m.clear_selection(cx);
        assert_eq!(m.selection_text(), None);
    });
}

/// A focused shell whose cleared screen shows `printf`'s output, and the row of `first`,
/// the first line it prints.
fn printed(cx: &mut TestAppContext, h: &Harness, options: TerminalOptions, printf: &str, first: &str) -> (Entity<TerminalModel>, usize) {
    let model = open(cx, h, options);
    model.update(cx, |m, cx| m.set_focused(true, cx));
    type_line(cx, &model, &format!("clear; printf '{printf}'"));
    let row = |m: &TerminalModel| m.screen_text().lines().position(|l| l == first);
    wait_until(cx, &model, |m| row(m).is_some());
    let row = model.read_with(cx, |m, _| row(m)).unwrap();
    (model, row)
}

fn keys(edit: impl FnOnce(&mut Modifiers)) -> Modifiers {
    let mut modifiers = Modifiers::default();
    edit(&mut modifiers);
    modifiers
}

fn right_half(line: usize, column: usize) -> CellPoint {
    CellPoint { line, column, right_half: true }
}

/// A press at `from`, dragged over the grid to `to` and released there.
fn drag(m: &mut TerminalModel, from: CellPoint, to: CellPoint, modifiers: Modifiers, cx: &mut gpui_kit::Context<TerminalModel>) {
    m.press_selection(from, 1, modifiers, cx);
    m.drag_selection(to, 0., cx);
    m.release_selection(to, modifiers, cx);
}

#[gpui_kit::test]
fn an_alt_drag_selects_a_block_of_columns(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let pty = spawn_shell(&h);
    let (model, row) = printed(cx, &h, options(pty), r"abcdef\nghijkl\n", "abcdef");
    let (events, _subscription) = record(cx, &model);
    model.update(cx, |m, cx| {
        drag(m, cell(row, 1), right_half(row + 1, 3), keys(|k| k.alt = true), cx);
        assert_eq!(m.selection_text().as_deref(), Some("bcd\nhij"));
        drag(m, cell(row, 1), right_half(row + 1, 3), Modifiers::default(), cx);
        assert_eq!(m.selection_text().as_deref(), Some("bcdef\nghij"));
    });
    // Released with more than a character selected, the Alt drag moved no cursor.
    assert!(!events.borrow().iter().any(|e| matches!(e, TerminalEvent::Input(_))));
}

#[gpui_kit::test]
fn shift_click_extends_the_selection_unless_a_program_tracks_the_mouse(cx: &mut TestAppContext) {
    use treeterm_term::alacritty_terminal::term::TermMode;
    cx.executor().allow_parking();
    let h = harness();
    let pty = spawn_shell(&h);
    let (model, row) = printed(cx, &h, options(pty), r"abcdef\nghijkl\n", "abcdef");
    let shift = keys(|k| k.shift = true);
    let click = |m: &mut TerminalModel, at: CellPoint, modifiers: Modifiers, cx: &mut gpui_kit::Context<TerminalModel>| {
        m.press_selection(at, 1, modifiers, cx);
        m.release_selection(at, modifiers, cx);
    };
    model.update(cx, |m, cx| {
        // A plain click selects nothing, but a Shift+click extends from it.
        click(m, cell(row, 1), Modifiers::default(), cx);
        assert_eq!(m.selection_text(), None);
        click(m, right_half(row + 1, 2), shift, cx);
        assert_eq!(m.selection_text().as_deref(), Some("bcdef\nghi"));
        click(m, right_half(row, 3), shift, cx);
        assert_eq!(m.selection_text().as_deref(), Some("bcd"));
        // Dragged on from a Shift+press, it keeps extending.
        m.press_selection(right_half(row, 2), 1, shift, cx);
        m.drag_selection(right_half(row, 4), 0., cx);
        assert_eq!(m.selection_text().as_deref(), Some("bcde"));
        m.release_selection(right_half(row, 4), shift, cx);
        // With nothing to extend, a Shift+click selects nothing.
        m.clear_selection(cx);
        click(m, right_half(row + 1, 4), shift, cx);
        assert_eq!(m.selection_text(), None);
    });

    // A program tracking the mouse gets plain clicks; a Shift+click starts a new selection.
    type_line(cx, &model, r"printf '\033[?1000h'");
    wait_until(cx, &model, |m| m.term().lock().mode().contains(TermMode::MOUSE_REPORT_CLICK));
    model.update(cx, |m, cx| {
        drag(m, cell(row, 1), right_half(row, 3), Modifiers::default(), cx);
        assert_eq!(m.selection_text().as_deref(), Some("bcd"));
        click(m, right_half(row + 1, 2), shift, cx);
        assert_eq!(m.selection_text(), None);
    });
}

#[gpui_kit::test]
fn a_right_click_selects_the_word_unless_inside_the_selection(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let pty = spawn_shell(&h);
    let (model, row) = printed(cx, &h, options(pty), r"one two three\n", "one two three");
    model.update(cx, |m, cx| {
        m.select_word_unless_selected(cell(row, 5), cx);
        assert_eq!(m.selection_text().as_deref(), Some("two"));

        // Inside the selection it stays.
        drag(m, cell(row, 0), right_half(row, 9), Modifiers::default(), cx);
        assert_eq!(m.selection_text().as_deref(), Some("one two th"));
        m.select_word_unless_selected(cell(row, 5), cx);
        assert_eq!(m.selection_text().as_deref(), Some("one two th"));

        // Blank space selects nothing: a dragged selection outside it goes, a word stays.
        m.select_word_unless_selected(cell(row, 20), cx);
        assert_eq!(m.selection_text(), None);
        m.select_word_unless_selected(cell(row, 10), cx);
        assert_eq!(m.selection_text().as_deref(), Some("three"));
        m.select_word_unless_selected(cell(row, 20), cx);
        assert_eq!(m.selection_text().as_deref(), Some("three"));
    });
}

#[gpui_kit::test]
fn a_ghostty_tab_copies_a_drag_or_a_double_click_as_it_ends(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let pty = spawn_shell(&h);
    let ghostty = TerminalOptions { selection_copy: SelectionCopy::OnSelect, ..options(pty.clone()) };
    let (model, row) = printed(cx, &h, ghostty, r"abcdef ghi\n", "abcdef ghi");
    let (events, _subscription) = record(cx, &model);
    let copied = |events: &RefCell<Vec<TerminalEvent>>| -> Vec<String> {
        events
            .borrow()
            .iter()
            .filter_map(|event| match event {
                TerminalEvent::Clipboard(text) => Some(text.clone()),
                _ => None,
            })
            .collect()
    };
    model.update(cx, |m, cx| {
        drag(m, cell(row, 0), right_half(row, 2), Modifiers::default(), cx);
        m.press_selection(cell(row, 8), 2, Modifiers::default(), cx);
        m.release_selection(cell(row, 8), Modifiers::default(), cx);
        // A plain click selects, and copies, nothing.
        m.press_selection(cell(row, 4), 1, Modifiers::default(), cx);
        m.release_selection(cell(row, 4), Modifiers::default(), cx);
    });
    assert_eq!(copied(&events), ["abc", "ghi"]);

    // A Terminal tab copies only on Copy.
    let terminal = open(cx, &h, options(pty));
    let (events, _subscription) = record(cx, &terminal);
    terminal.update(cx, |m, cx| {
        drag(m, cell(row, 0), right_half(row, 2), Modifiers::default(), cx);
        assert_eq!(m.selection_text().as_deref(), Some("abc"));
    });
    assert!(copied(&events).is_empty());
}

#[gpui_kit::test]
fn an_alt_click_moves_the_shells_cursor_when_quick_and_at_the_bottom(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let model = with_scrollback(cx, &h);
    let (events, _subscription) = record(cx, &model);
    model.update(cx, |m, cx| m.write_input(b"echo 12345", cx));
    let at = |m: &TerminalModel| {
        let lines: Vec<String> = m.screen_text().lines().map(str::to_owned).collect();
        let row = lines.iter().position(|l| l.ends_with("echo 12345"))?;
        Some(cell(row, lines[row].find("12345")? + 2))
    };
    wait_until(cx, &model, |m| at(m).is_some());
    let three = model.read_with(cx, |m, _| at(m)).unwrap();
    let alt = keys(|k| k.alt = true);
    let typed = |events: &RefCell<Vec<TerminalEvent>>| events.borrow().iter().filter(|e| matches!(e, TerminalEvent::Input(_))).count();
    let before = typed(&events);

    // Scrolled up, or released too late, it sends nothing.
    model.update(cx, |m, cx| {
        m.scroll_lines(1, cx);
        m.press_selection(three, 1, alt, cx);
        m.release_selection(three, alt, cx);
        m.scroll_to_bottom(cx);
        m.press_selection(three, 1, alt, cx);
    });
    advance(cx, ALT_CLICK_MOVE_CURSOR_TIME);
    model.update(cx, |m, cx| m.release_selection(three, alt, cx));
    assert_eq!(typed(&events), before);

    model.update(cx, |m, cx| {
        m.press_selection(three, 1, alt, cx);
        m.release_selection(three, alt, cx);
    });
    assert_eq!(typed(&events), before + 1);
    type_line(cx, &model, "X");
    wait_until(cx, &model, |m| m.screen_text().lines().any(|l| l == "12X345"));
}

#[gpui_kit::test]
fn a_selection_dragged_past_the_edge_scrolls_the_scrollback(cx: &mut TestAppContext) {
    use treeterm_term::alacritty_terminal::index::Line;
    cx.executor().allow_parking();
    let h = harness();
    let model = with_scrollback(cx, &h);
    let row = model.read_with(cx, |m, _| m.screen_text().lines().position(|l| l == "300")).unwrap();
    model.update(cx, |m, cx| {
        m.press_selection(right_half(row, 2), 1, Modifiers::default(), cx);
        // 10 pixels above the grid: 4 lines a tick.
        m.drag_selection(cell(0, 5), -10., cx);
    });
    assert_eq!(offset(cx, &model), 0, "nothing before the first tick");
    advance(cx, DRAG_SCROLL_INTERVAL);
    assert_eq!(offset(cx, &model), 4);
    advance(cx, DRAG_SCROLL_INTERVAL);
    assert_eq!(offset(cx, &model), 8);
    let (text, top) = model.read_with(cx, |m, _| {
        let top = treeterm_term::view::line_text(&*m.term().lock(), Line(-8));
        (m.selection_text().unwrap(), top)
    });
    assert!(text.starts_with(&format!("{top}\n")) && text.ends_with("\n300"), "{text:?}");

    // Back over the grid it stops; just past the bottom it goes down a line a tick.
    model.update(cx, |m, cx| m.drag_selection(cell(3, 0), 0., cx));
    advance(cx, DRAG_SCROLL_INTERVAL * 3);
    assert_eq!(offset(cx, &model), 8);
    model.update(cx, |m, cx| m.drag_selection(cell(23, 0), 1., cx));
    advance(cx, DRAG_SCROLL_INTERVAL);
    assert_eq!(offset(cx, &model), 7);

    // The release ends it.
    model.update(cx, |m, cx| m.release_selection(cell(23, 0), Modifiers::default(), cx));
    advance(cx, DRAG_SCROLL_INTERVAL * 3);
    assert_eq!(offset(cx, &model), 7);
    assert!(!model.read_with(cx, |m, _| m.is_selecting()));
}

const HYPERLINK: &str = "https://example.com/osc";
const URL: &str = "https://plain.example/y";

/// A focused shell with scrollback whose last output line is `open-42 or <URL>`, `open-42`
/// being an OSC 8 hyperlink to [`HYPERLINK`]. Returns the line's row in the viewport.
fn with_links(cx: &mut TestAppContext, h: &Harness) -> (Entity<TerminalModel>, usize) {
    let pty = spawn_shell(h);
    let model = open(cx, h, options(pty));
    model.update(cx, |m, cx| m.set_focused(true, cx));
    type_line(
        cx,
        &model,
        &format!(r"seq 1 30; printf '\033]8;;{HYPERLINK}\033\\open-%s\033]8;;\033\\ or {URL}\n' $((40+2))"),
    );
    let row = |m: &TerminalModel| m.screen_text().lines().position(|l| l == format!("open-42 or {URL}"));
    wait_until(cx, &model, |m| row(m).is_some());
    let row = model.read_with(cx, |m, _| row(m)).unwrap();
    (model, row)
}

fn cell(line: usize, column: usize) -> treeterm_model::terminal::CellPoint {
    treeterm_model::terminal::CellPoint { line, column, right_half: false }
}

fn pointer(line: usize, column: usize, secondary: bool) -> Pointer {
    Pointer { at: cell(line, column), secondary }
}

fn uri_and_kind(link: Option<Link>) -> Option<(String, LinkKind)> {
    link.map(|link| (link.uri, link.kind))
}

#[gpui_kit::test]
fn finds_hyperlinks_and_urls_where_the_viewport_shows_them(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let (model, row) = with_links(cx, &h);
    let hyperlink = model.read_with(cx, |m, _| m.link_at(cell(row, 3))).expect("the hyperlink");
    assert_eq!((hyperlink.uri.as_str(), hyperlink.kind, hyperlink.columns.clone()), (HYPERLINK, LinkKind::Hyperlink, 0..7));
    let url = model.read_with(cx, |m, _| m.link_at(cell(row, 12))).expect("the URL");
    assert_eq!((url.uri.as_str(), url.kind, url.columns), (URL, LinkKind::Url, 11..34));
    assert_eq!(model.read_with(cx, |m, _| m.link_at(cell(row, 8))), None);

    // Scrolled up a line, the viewport shows it a row lower.
    model.update(cx, |m, cx| m.scroll_lines(1, cx));
    assert_eq!(model.read_with(cx, |m, _| m.link_at(cell(row + 1, 3))), Some(hyperlink));
    assert_eq!(model.read_with(cx, |m, _| m.link_at(cell(row, 3))), None, "`30` is there now");
}

#[gpui_kit::test]
fn the_hovered_link_is_one_a_click_opens(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let (model, row) = with_links(cx, &h);
    let hovered = |cx: &mut TestAppContext| uri_and_kind(model.read_with(cx, |m, _| m.hovered_link()));
    assert_eq!(hovered(cx), None, "no pointer yet");

    model.update(cx, |m, cx| m.hover(Some(pointer(row, 3, false)), cx));
    assert_eq!(hovered(cx), Some((HYPERLINK.to_owned(), LinkKind::Hyperlink)));
    // A URL in the text is a link only while Cmd/Ctrl is held.
    model.update(cx, |m, cx| m.hover(Some(pointer(row, 12, false)), cx));
    assert_eq!(hovered(cx), None);
    model.update(cx, |m, cx| m.hover_secondary(true, cx));
    assert_eq!(hovered(cx), Some((URL.to_owned(), LinkKind::Url)));
    model.update(cx, |m, cx| m.hover_secondary(false, cx));
    assert_eq!(hovered(cx), None);
    model.update(cx, |m, cx| m.hover(Some(pointer(row, 3, true)), cx));
    assert_eq!(hovered(cx), Some((HYPERLINK.to_owned(), LinkKind::Hyperlink)), "Cmd/Ctrl changes nothing for a hyperlink");

    // Off the grid nothing is hovered, and Cmd/Ctrl going down there changes nothing.
    model.update(cx, |m, cx| m.hover(None, cx));
    assert_eq!(hovered(cx), None);
    model.update(cx, |m, cx| m.hover_secondary(true, cx));
    assert_eq!(hovered(cx), None);

    // The hovered link follows the viewport under a pointer that stays.
    model.update(cx, |m, cx| m.hover(Some(pointer(row + 1, 3, false)), cx));
    assert_eq!(hovered(cx), None);
    model.update(cx, |m, cx| m.scroll_lines(1, cx));
    assert_eq!(hovered(cx), Some((HYPERLINK.to_owned(), LinkKind::Hyperlink)));
}

#[gpui_kit::test]
fn a_link_opens_when_released_where_it_was_pressed(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let (model, row) = with_links(cx, &h);
    let click = |cx: &mut TestAppContext, down: Option<Pointer>, up: Option<usize>| {
        model.update(cx, |m, cx| {
            let press = m.press_link(down);
            m.release_link(up.map(|column| cell(row, column)), cx);
            press
        })
    };

    // A press on a hyperlink leaves the terminal to select or report it too; dragged off the
    // link, or off the terminal, it opens nothing.
    assert_eq!(click(cx, Some(pointer(row, 3, false)), Some(8)), LinkPress::Shared);
    assert_eq!(click(cx, Some(pointer(row, 3, false)), None), LinkPress::Shared);
    // A plain press on a URL in the text is no link press; nor is one off the grid.
    assert_eq!(click(cx, Some(pointer(row, 12, false)), Some(12)), LinkPress::Shared);
    assert_eq!(click(cx, None, Some(3)), LinkPress::Shared);
    // A release on a link that was not pressed opens nothing.
    assert_eq!(click(cx, Some(pointer(row, 8, false)), Some(3)), LinkPress::Shared);
    assert_eq!(cx.opened_url(), None);

    // A plain click on a hyperlink opens it, released anywhere on it.
    assert_eq!(click(cx, Some(pointer(row, 0, false)), Some(6)), LinkPress::Shared);
    assert_eq!(cx.opened_url().as_deref(), Some(HYPERLINK));
    // A Cmd/Ctrl+press on a URL is the link's alone; the release opens it.
    assert_eq!(click(cx, Some(pointer(row, 12, true)), Some(30)), LinkPress::Taken);
    assert_eq!(cx.opened_url().as_deref(), Some(URL));
}

/// A focused shell with 300 lines of scrollback, viewport at the bottom.
fn with_scrollback(cx: &mut TestAppContext, h: &Harness) -> Entity<TerminalModel> {
    let pty = spawn_shell(h);
    let model = open(cx, h, options(pty));
    model.update(cx, |m, cx| m.set_focused(true, cx));
    type_line(cx, &model, "seq 1 300; echo scrolled-$((2+3))");
    wait_until(cx, &model, |m| m.screen_text().contains("scrolled-5"));
    model
}

fn offset(cx: &mut TestAppContext, model: &Entity<TerminalModel>) -> usize {
    model.read_with(cx, |m, _| m.term().lock().grid().display_offset())
}

fn position(cx: &mut TestAppContext, model: &Entity<TerminalModel>) -> ScrollPosition {
    model.read_with(cx, |m, _| m.scroll_position())
}

fn pinned(cx: &mut TestAppContext, model: &Entity<TerminalModel>) -> bool {
    model.read_with(cx, |m, _| m.is_pinned())
}

fn click_badge(cx: &mut TestAppContext, model: &Entity<TerminalModel>) {
    model.update(cx, |m, cx| m.click_scroll_badge(cx));
}

fn advance(cx: &mut TestAppContext, by: Duration) {
    cx.executor().advance_clock(by);
    cx.run_until_parked();
}

#[gpui_kit::test]
fn a_pinned_terminal_snaps_back_until_the_wheel_turns_up(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let model = with_scrollback(cx, &h);
    click_badge(cx, &model);
    click_badge(cx, &model);
    assert!(pinned(cx, &model));

    // Shift+PageUp, the scrollbar and any other scroll leave it at the bottom, still pinned.
    model.update(cx, |m, cx| m.scroll_page(true, cx));
    assert_eq!((offset(cx, &model), pinned(cx, &model)), (0, true));
    model.update(cx, |m, cx| m.scroll_to_ratio(0., cx));
    assert_eq!((offset(cx, &model), pinned(cx, &model)), (0, true));
    model.update(cx, |m, cx| m.scroll_lines(5, cx));
    assert_eq!((offset(cx, &model), pinned(cx, &model)), (0, true));

    // A wheel turned up unpins, whether or not it then scrolls the scrollback.
    model.update(cx, |m, cx| m.wheel_up(cx));
    assert!(!pinned(cx, &model));
    model.update(cx, |m, cx| m.scroll_lines(5, cx));
    assert_eq!(position(cx, &model), ScrollPosition::Middle);
}

#[gpui_kit::test]
fn one_line_above_the_bottom_reads_bottom(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let model = with_scrollback(cx, &h);
    model.update(cx, |m, cx| m.scroll_lines(1, cx));
    assert_eq!(position(cx, &model), ScrollPosition::Bottom);
    model.update(cx, |m, cx| m.scroll_lines(1, cx));
    assert_eq!(position(cx, &model), ScrollPosition::Middle);
}

#[gpui_kit::test]
fn a_badge_click_scrolls_once_no_second_click_came(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let model = with_scrollback(cx, &h);

    // From the bottom a single click goes to the top...
    click_badge(cx, &model);
    advance(cx, BADGE_DOUBLE_CLICK - Duration::from_millis(1));
    assert_eq!(position(cx, &model), ScrollPosition::Bottom);
    advance(cx, Duration::from_millis(1));
    assert_eq!(position(cx, &model), ScrollPosition::Top);

    // ...and from anywhere else to the bottom.
    click_badge(cx, &model);
    advance(cx, BADGE_DOUBLE_CLICK);
    assert_eq!((offset(cx, &model), pinned(cx, &model)), (0, false));
}

#[gpui_kit::test]
fn a_badge_double_click_pins_without_scrolling_first(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let model = with_scrollback(cx, &h);

    click_badge(cx, &model);
    assert_eq!(position(cx, &model), ScrollPosition::Bottom);
    click_badge(cx, &model);
    assert!(pinned(cx, &model));
    advance(cx, BADGE_DOUBLE_CLICK * 2);
    assert_eq!((offset(cx, &model), pinned(cx, &model)), (0, true));
}

#[gpui_kit::test]
fn a_double_click_on_pinned_unpins_then_scrolls_to_the_top(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let model = with_scrollback(cx, &h);
    click_badge(cx, &model);
    click_badge(cx, &model);
    assert!(pinned(cx, &model));

    // The first click unpins and stays at the bottom; the second is a new single click.
    click_badge(cx, &model);
    assert_eq!((offset(cx, &model), pinned(cx, &model)), (0, false));
    click_badge(cx, &model);
    assert!(!pinned(cx, &model));
    advance(cx, BADGE_DOUBLE_CLICK);
    assert_eq!(position(cx, &model), ScrollPosition::Top);
}

/// Focus `model` and wait until the daemon took a line from it: it now owns the input.
fn take_input(cx: &mut TestAppContext, model: &Entity<TerminalModel>, line: &str, output: &str) {
    model.update(cx, |m, cx| m.set_focused(true, cx));
    type_line(cx, model, line);
    wait_until(cx, model, |m| m.screen_text().contains(output));
}

/// Two terminals on one shell, as two windows or machines show it, both focused: `b`
/// took the input from `a`.
fn two_terminals_b_owning(cx: &mut TestAppContext, h: &Harness) -> (Entity<TerminalModel>, Entity<TerminalModel>) {
    let pty = spawn_shell(h);
    let a = open(cx, h, options(pty.clone()));
    let b = open(cx, h, options(pty));
    take_input(cx, &a, "echo took-a-$((0+1))", "took-a-1");
    take_input(cx, &b, "echo b-$((1+1))", "b-2");
    (a, b)
}

/// Resize through `model`'s input queue and wait for the daemon to apply it: everything
/// queued before it has been handled by then. `fit` asks for nothing until the daemon has
/// reported the attachment's size (after a refresh, too), so it is asked again until it does.
fn drain_input(cx: &mut TestAppContext, model: &Entity<TerminalModel>) {
    let target = GridSize { cols: 100, rows: 30 };
    for _ in 0..1000 {
        model.update(cx, |m, _| m.fit(target));
        cx.run_until_parked();
        if model.read_with(cx, |m, _| m.grid_size() == target) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("the resize was not applied");
}

#[gpui_kit::test]
fn a_key_press_in_a_terminal_that_kept_focus_claims_the_input_back(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let (a, _b) = two_terminals_b_owning(cx, &h);

    // `a` kept focus while `b` claimed: no focus change fires, so its line is dropped.
    type_line(cx, &a, "echo dropped-$((2+2))");
    a.read_with(cx, |m, _| m.key_pressed());
    type_line(cx, &a, "echo a-$((3+3))");
    wait_until(cx, &a, |m| m.screen_text().contains("a-6"));
    assert!(!a.read_with(cx, |m, _| m.screen_text().contains("dropped-4")));
}

#[gpui_kit::test]
fn a_key_press_in_an_unfocused_terminal_claims_nothing(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let pty = spawn_shell(&h);
    let a = open(cx, &h, options(pty.clone()));
    let b = open(cx, &h, options(pty));
    take_input(cx, &a, "echo took-a-$((0+1))", "took-a-1");

    b.read_with(cx, |m, _| m.key_pressed());
    drain_input(cx, &b);
    type_line(cx, &a, "echo a-$((1+1))");
    wait_until(cx, &a, |m| m.screen_text().contains("a-2"));
}

#[gpui_kit::test]
fn a_pointer_press_claims_the_input_even_unfocused(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let (a, b) = two_terminals_b_owning(cx, &h);

    // A right-click into `a` while its window is in the background.
    a.update(cx, |m, cx| m.set_focused(false, cx));
    a.read_with(cx, |m, _| m.claim_input());
    drain_input(cx, &a);
    type_line(cx, &b, "echo dropped-$((2+2))");
    b.read_with(cx, |m, _| m.key_pressed());
    type_line(cx, &b, "echo b-$((4+4))");
    wait_until(cx, &b, |m| m.screen_text().contains("b-8"));
    assert!(!b.read_with(cx, |m, _| m.screen_text().contains("dropped-4")));
}

#[gpui_kit::test]
fn paste_claims_the_input_before_writing(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let (a, _b) = two_terminals_b_owning(cx, &h);

    a.update(cx, |m, cx| m.paste("echo pasted-$((5+5))", cx));
    type_line(cx, &a, "");
    wait_until(cx, &a, |m| m.screen_text().contains("pasted-10"));
}

#[gpui_kit::test]
fn a_paste_is_input_as_written_with_its_line_endings_as_enter(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let pty = spawn_shell(&h);
    let model = open(cx, &h, options(pty));
    let (events, _subscription) = record(cx, &model);

    // Unfocused, as from the context menu.
    model.update(cx, |m, cx| m.paste("echo one\necho two", cx));
    let inputs: Vec<String> = events
        .borrow()
        .iter()
        .filter_map(|event| match event {
            TerminalEvent::Input(data) => Some(data.clone()),
            _ => None,
        })
        .collect();
    let [input] = inputs.as_slice() else {
        panic!("one input expected: {inputs:?}");
    };
    // Bracketed if the shell's line editor asked for it.
    assert!(input.contains("echo one\recho two"), "{input:?}");
    assert!(!input.contains('\n'), "{input:?}");
}

const IDLE: Duration = Duration::from_millis(300);

#[gpui_kit::test]
fn a_resize_is_a_change_of_screen_only_where_resizes_are_snapshotted(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let pty = spawn_cat(&h);
    let analyzed = open(cx, &h, TerminalOptions { idle_timeout: Some(IDLE), ..options(pty.clone()) });
    analyzed.update(cx, |m, cx| m.set_idle_snapshots(IdleSnapshots::AfterOutputAndResize, cx));
    let plain = open(cx, &h, TerminalOptions { idle_timeout: Some(IDLE), ..options(pty) });
    // The daemon types the startup command once the shell is ready; `cat` runs once it echoes
    // a line back, and nothing else is printed after that.
    wait_until(cx, &analyzed, |m| m.screen_text().contains("exec cat"));
    analyzed.update(cx, |m, cx| m.set_focused(true, cx));
    type_line(cx, &analyzed, "cat-ready");
    for model in [&analyzed, &plain] {
        wait_until(cx, model, |m| m.screen_text().contains("cat-ready\ncat-ready"));
    }
    // The first burst goes quiet: a resting state is known.
    advance(cx, IDLE);
    let (analyzed_events, _analyzed) = record(cx, &analyzed);
    let (plain_events, _plain) = record(cx, &plain);

    let shorter = GridSize { cols: 80, rows: 20 };
    analyzed.update(cx, |m, _| m.fit(shorter));
    for model in [&analyzed, &plain] {
        wait_until(cx, model, |m| m.grid_size() == shorter);
    }
    advance(cx, IDLE);
    let screen = analyzed.read_with(cx, |m, _| m.viewport());
    let edge = IdleEdge { snapshot: screen.clone(), idle_timeout: IDLE };
    assert_eq!(*analyzed_events.borrow(), vec![TerminalEvent::Activity(screen), TerminalEvent::Idle(edge)]);
    assert_eq!(*plain_events.borrow(), Vec::new());
}

/// A shell that prints nothing, as a new AI harness's shell before its prompt: its first
/// startup file replaces it with `sleep`.
fn spawn_silent(h: &Harness) -> String {
    let root = h.daemon.root();
    // zsh reads `$ZDOTDIR/.zshenv` first; bash and sh read a profile from `$HOME`.
    for file in [".zshenv", ".bash_profile", ".profile"] {
        std::fs::write(root.join(file), "exec sleep 60\n").unwrap();
    }
    let dir = root.to_string_lossy().into_owned();
    let env = HashMap::from([("ZDOTDIR".to_owned(), dir.clone()), ("HOME".to_owned(), dir.clone())]);
    futures::executor::block_on(h.conn.create_pty(CreatePty {
        cwd: dir,
        env,
        size: Some((80, 24)),
        ..CreatePty::default()
    }))
    .unwrap()
    .unwrap()
}

#[gpui_kit::test]
fn a_blank_screen_resized_is_no_change_where_resizes_are_snapshotted(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let pty = spawn_silent(&h);
    let conn = h.conn.clone();
    let options = TerminalOptions { idle_timeout: Some(IDLE), ..options(pty) };
    // As the workspace store does: the analyzer asks for resize snapshots on a terminal it was
    // just given, before anything arrived.
    let model = cx.new(|cx| {
        let mut model = TerminalModel::new(conn, options, cx);
        model.set_idle_snapshots(IdleSnapshots::AfterOutputAndResize, cx);
        model
    });
    let (events, _subscription) = record(cx, &model);
    // The attach replay reports the PTY's size; only then does a fit resize it.
    wait_until(cx, &model, |m| *m.overlay() == Overlay::Hidden);

    // The view's first fit, while the harness has printed nothing.
    let taller = GridSize { cols: 80, rows: 45 };
    model.update(cx, |m, _| m.fit(taller));
    wait_until(cx, &model, |m| m.grid_size() == taller);
    advance(cx, IDLE * 2);
    assert!(model.read_with(cx, |m, _| m.viewport().trim().is_empty()), "the shell printed something");
    assert_eq!(*events.borrow(), Vec::new());

    // A refreshed stream starts on a placeholder grid of another size, replayed blank.
    model.update(cx, |m, cx| m.refresh(cx));
    wait_until(cx, &model, |m| *m.overlay() == Overlay::Hidden && m.grid_size() == taller);
    let shorter = GridSize { cols: 80, rows: 30 };
    model.update(cx, |m, _| m.fit(shorter));
    wait_until(cx, &model, |m| m.grid_size() == shorter);
    advance(cx, IDLE * 2);
    assert!(model.read_with(cx, |m, _| m.viewport().trim().is_empty()), "the shell printed something");
    assert_eq!(*events.borrow(), Vec::new());
}

#[gpui_kit::test]
fn refresh_claims_the_input_for_the_new_attachment_when_focused(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let (a, _b) = two_terminals_b_owning(cx, &h);

    a.update(cx, |m, cx| m.refresh(cx));
    wait_until(cx, &a, |m| *m.overlay() == Overlay::Hidden && m.screen_text().contains("b-2"));
    type_line(cx, &a, "echo a-$((3+3))");
    wait_until(cx, &a, |m| m.screen_text().contains("a-6"));
}

#[gpui_kit::test]
fn refresh_of_an_unfocused_terminal_claims_nothing(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let pty = spawn_shell(&h);
    let a = open(cx, &h, options(pty.clone()));
    let b = open(cx, &h, options(pty));
    take_input(cx, &b, "echo took-b-$((0+1))", "took-b-1");

    a.update(cx, |m, cx| m.refresh(cx));
    wait_until(cx, &a, |m| *m.overlay() == Overlay::Hidden);
    drain_input(cx, &a);
    type_line(cx, &b, "echo b-$((1+1))");
    wait_until(cx, &b, |m| m.screen_text().contains("b-2"));
}
