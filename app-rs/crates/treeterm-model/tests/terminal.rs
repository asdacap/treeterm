//! TerminalModel against a real in-process daemon and a real shell.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::{AppContext as _, Entity, Subscription, TestAppContext};
use treeterm_core::pty::CreatePty;
use treeterm_core::settings::TerminalSettings;
use treeterm_model::terminal::{
    BADGE_DOUBLE_CLICK, Overlay, TerminalEvent, TerminalModel, TerminalOptions,
};
use treeterm_term::core::GridSize;
use treeterm_term::fit::WidthLimit;
use treeterm_term::palette::DEFAULT_BACKGROUND;
use treeterm_term::view::ScrollPosition;

mod common;
use common::{Harness, harness, wait_until};

/// A login shell with an empty config dir, so the user's dotfiles stay out of the test.
fn spawn_shell(h: &Harness) -> String {
    let mut env = HashMap::new();
    env.insert("ZDOTDIR".to_owned(), h.daemon.root().to_string_lossy().into_owned());
    env.insert("PS1".to_owned(), "$ ".to_owned());
    futures::executor::block_on(h.conn.create_pty(CreatePty {
        cwd: h.daemon.root().to_string_lossy().into_owned(),
        env,
        size: Some((80, 24)),
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
        keep_on_exit: false,
        width_limit: WidthLimit::Capped(160),
        idle_timeout: None,
        traffic: treeterm_core::traffic::ByteCounter::default(),
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

    let meter = cx.new(|cx| treeterm_model::traffic::TrafficMeter::new(counter.clone(), cx));
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
        m.request_size(target);
        assert_eq!(m.grid_size(), GridSize { cols: 80, rows: 24 }, "no local resize");
    });
    wait_until(cx, &model, |m| m.grid_size() == target);
    assert_eq!(model.read_with(cx, |m, _| m.size_mismatch()), None);
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
    let model = open(cx, &h, TerminalOptions { keep_on_exit: true, ..options(pty) });
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
fn selection_copies_the_selected_text(cx: &mut TestAppContext) {
    use treeterm_model::terminal::{CellPoint, SelectionKind};
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
