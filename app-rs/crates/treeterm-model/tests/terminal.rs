//! TerminalModel against a real in-process daemon and a real shell.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gpui_kit::{AppContext as _, Entity, Subscription, TestAppContext};
use treeterm_core::pty::CreatePty;
use treeterm_core::settings::TerminalSettings;
use treeterm_model::terminal::{Overlay, TerminalEvent, TerminalModel, TerminalOptions};
use treeterm_term::core::GridSize;
use treeterm_term::fit::WidthLimit;
use treeterm_term::palette::DEFAULT_BACKGROUND;

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
    model.update(cx, |m, _| m.write_input(format!("{line}\r").as_bytes()));
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
