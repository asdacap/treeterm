//! Debug-only control socket for end-to-end tests and screenshots.
//!
//! Enabled with the `automation` cargo feature and `TREETERM_AUTOMATION_SOCKET=<path>`.
//! One command per line, one JSON reply per line:
//!
//! - `state` — what the window shows (workspaces, selection, terminal screens, dialogs)
//! - `select <workspace-id>` / `tab <workspace-id> <tab-id>`
//! - `newtab <workspace-id>` / `closetab <workspace-id> <tab-id>`
//! - `open <directory>` — open a directory as a workspace (no folder picker)
//! - `workspacedialog` — the current session's "+" (the Open Workspace dialog)
//! - `fork <workspace-id>` — a quick fork of the workspace (its tree row's Fork button)
//! - `type <text>` — type text as keystrokes; `key <keystroke>` — e.g. `enter`, `ctrl-c`
//! - `click`/`rclick`/`dblclick <x> <y> [modifiers]` and `move <x> <y> [modifiers]` — the
//!   pointer in window points, with keys held (e.g. `cmd`, `ctrl-shift`); `modifiers
//!   [modifiers]` — the keys held change (none: released)
//! - `scroll <x> <y> <dy>` — a wheel scroll of `dy` points over (x, y), in window points
//! - `screenshot <path.png>` — render the last frame offscreen (no screen-capture permission)

use std::path::PathBuf;

use futures::channel::{mpsc, oneshot};
use futures::{SinkExt, StreamExt};
use gpui_kit::{AnyWindowHandle, App, AsyncApp, Entity, Keystroke};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use treeterm_core::rt::Rt;

use crate::app_view::AppView;

type Request = (String, oneshot::Sender<Value>);

pub fn start(path: PathBuf, window: AnyWindowHandle, view: Entity<AppView>, rt: &Rt, cx: &mut App) {
    let (tx, mut rx) = mpsc::channel::<Request>(16);
    let server = rt.run(serve(path, tx));
    cx.spawn(async move |cx: &mut AsyncApp| {
        let _server = server;
        while let Some((line, reply)) = rx.next().await {
            let result = handle(&line, window, &view, cx);
            reply.send(result).ok();
        }
    })
    .detach();
}

async fn serve(path: PathBuf, tx: mpsc::Sender<Request>) {
    if path.exists() && std::fs::remove_file(&path).is_err() {
        tracing::error!(?path, "automation socket path is taken");
        return;
    }
    let listener = match UnixListener::bind(&path) {
        Ok(listener) => listener,
        Err(error) => {
            tracing::error!(%error, ?path, "cannot bind automation socket");
            return;
        }
    };
    tracing::info!(?path, "automation socket listening");
    loop {
        let Ok((stream, _)) = listener.accept().await else {
            return;
        };
        let mut tx = tx.clone();
        tokio::spawn(async move {
            let (read, mut write) = stream.into_split();
            let mut lines = BufReader::new(read).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let (reply_tx, reply_rx) = oneshot::channel();
                if tx.send((line, reply_tx)).await.is_err() {
                    return;
                }
                let reply = reply_rx.await.unwrap_or_else(|_| json!({ "error": "app gone" }));
                let mut out = reply.to_string();
                out.push('\n');
                if write.write_all(out.as_bytes()).await.is_err() {
                    return;
                }
            }
        });
    }
}

/// Keys held, `-`-separated (`cmd`, `ctrl`, `alt`, `shift`); none when `None`.
fn modifiers(keys: Option<&str>) -> Result<gpui_kit::Modifiers, String> {
    let mut modifiers = gpui_kit::Modifiers::default();
    for key in keys.into_iter().flat_map(|keys| keys.split('-')) {
        match key {
            "cmd" => modifiers.platform = true,
            "ctrl" => modifiers.control = true,
            "alt" => modifiers.alt = true,
            "shift" => modifiers.shift = true,
            _ => return Err(format!("unknown modifier {key}: cmd, ctrl, alt or shift, joined by -")),
        }
    }
    Ok(modifiers)
}

/// `<x> <y> [modifiers]`: a point in window points and the keys held ([`modifiers`]).
fn pointer(args: &str) -> Result<(gpui_kit::Point<gpui_kit::Pixels>, gpui_kit::Modifiers), String> {
    let usage = || "usage: <x> <y> [modifiers]".to_owned();
    let mut args = args.split_whitespace();
    let mut coord = || args.next().ok_or_else(usage)?.parse::<f32>().map_err(|_| usage());
    let (x, y) = (coord()?, coord()?);
    Ok((gpui_kit::point(gpui_kit::px(x), gpui_kit::px(y)), modifiers(args.next())?))
}

fn handle(line: &str, window: AnyWindowHandle, view: &Entity<AppView>, cx: &mut AsyncApp) -> Value {
    let (command, rest) = line.split_once(' ').unwrap_or((line, ""));
    let result = window.update(cx, |_, window, cx| -> Result<Value, String> {
        // An occluded window gets no frames from the display link; render one so the reply
        // reflects the current state (views are built and sized while rendering), running
        // what the last one scheduled for it first, as a frame does.
        window.simulate_next_frame(cx);
        window.draw(cx).clear(cx);
        match command {
            "state" => Ok(view.read(cx).automation_state(window, cx)),
            "select" => {
                view.update(cx, |view, cx| view.automation_select_workspace(rest, cx));
                Ok(json!({ "ok": true }))
            }
            "tab" => {
                let (workspace, tab) = rest.split_once(' ').ok_or("usage: tab <workspace> <tab>")?;
                view.update(cx, |view, cx| view.automation_select_tab(workspace, tab, cx));
                Ok(json!({ "ok": true }))
            }
            "type" => {
                for c in rest.chars() {
                    let key = if c == ' ' { "space".to_owned() } else { c.to_string() };
                    let keystroke = Keystroke {
                        modifiers: Default::default(),
                        key,
                        key_char: Some(c.to_string()),
                    };
                    window.dispatch_keystroke(keystroke, cx);
                }
                Ok(json!({ "ok": true }))
            }
            "newtab" => {
                let (workspace, app) = rest.split_once(' ').unwrap_or((rest, ""));
                let tab = view.update(cx, |view, cx| view.automation_new_tab(workspace, app, cx));
                Ok(json!({ "ok": tab.is_some(), "tab": tab }))
            }
            "closetab" => {
                let (workspace, tab) = rest.split_once(' ').ok_or("usage: closetab <workspace> <tab>")?;
                view.update(cx, |view, cx| view.automation_close_tab(workspace, tab, cx));
                Ok(json!({ "ok": true }))
            }
            "open" => {
                let id = view.update(cx, |view, cx| view.automation_open_workspace(rest.to_owned(), cx));
                Ok(json!({ "ok": id.is_some(), "id": id }))
            }
            "workspacedialog" => {
                let ok = view.update(cx, |view, cx| view.automation_open_workspace_dialog(window, cx));
                Ok(json!({ "ok": ok }))
            }
            "childdialog" => {
                let session = view.read(cx).automation_session().ok_or("not connected")?;
                let on_created: crate::child_dialog::OnCreated = std::rc::Rc::new(|_, _, _| {});
                crate::child_dialog::open(session, rest.to_owned(), "parent".into(), on_created, window, cx);
                Ok(json!({ "ok": true }))
            }
            "fork" => {
                // `fork <workspace-id>`: the tree row's Fork button (past the upstream
                // warning), a child workspace on a new branch, shown once added.
                let ok = view.update(cx, |view, cx| view.automation_fork(rest, cx));
                Ok(json!({ "ok": ok }))
            }
            "fileopen" => {
                let ok = view.update(cx, |view, cx| view.automation_open_file(rest, cx));
                Ok(json!({ "ok": ok }))
            }
            "focus" => {
                // Re-run focus sync: focus the shown terminal again.
                window.activate_window();
                Ok(json!({ "ok": true }))
            }
            "action" => {
                // `action treeterm::ActiveProcesses`
                let action = cx.build_action(rest, None).map_err(|e| e.to_string())?;
                window.dispatch_action(action, cx);
                Ok(json!({ "ok": true }))
            }
            "drag" => {
                // `drag x1 y1 x2 y2`: press, move in steps with the button held, release.
                let coords: Vec<f32> = rest.split_whitespace().filter_map(|v| v.parse().ok()).collect();
                let [x1, y1, x2, y2] = coords[..] else {
                    return Err("usage: drag <x1> <y1> <x2> <y2>".to_owned());
                };
                let modifiers = gpui_kit::Modifiers::default();
                let button = gpui_kit::MouseButton::Left;
                let at = |x: f32, y: f32| gpui_kit::point(gpui_kit::px(x), gpui_kit::px(y));
                window.dispatch_event(
                    gpui_kit::PlatformInput::MouseMove(gpui_kit::MouseMoveEvent { position: at(x1, y1), pressed_button: None, modifiers }),
                    cx,
                );
                window.dispatch_event(
                    gpui_kit::PlatformInput::MouseDown(gpui_kit::MouseDownEvent {
                        button,
                        position: at(x1, y1),
                        modifiers,
                        click_count: 1,
                        first_mouse: false,
                    }),
                    cx,
                );
                for step in 1..=10 {
                    let t = step as f32 / 10.;
                    let position = at(x1 + (x2 - x1) * t, y1 + (y2 - y1) * t);
                    window.dispatch_event(
                        gpui_kit::PlatformInput::MouseMove(gpui_kit::MouseMoveEvent { position, pressed_button: Some(button), modifiers }),
                        cx,
                    );
                    window.refresh();
                }
                window.dispatch_event(
                    gpui_kit::PlatformInput::MouseUp(gpui_kit::MouseUpEvent { button, position: at(x2, y2), modifiers, click_count: 1 }),
                    cx,
                );
                Ok(json!({ "ok": true }))
            }
            "modifiers" => {
                // `modifiers [modifiers]`: the keys held now (none: all released).
                let modifiers = modifiers(Some(rest).filter(|keys| !keys.is_empty()))?;
                window.dispatch_event(
                    gpui_kit::PlatformInput::ModifiersChanged(gpui_kit::ModifiersChangedEvent {
                        modifiers,
                        capslock: gpui_kit::Capslock::default(),
                    }),
                    cx,
                );
                Ok(json!({ "ok": true }))
            }
            "move" => {
                // `move <x> <y> [modifiers]`: the pointer moves there, e.g. with `cmd` held.
                let (position, modifiers) = pointer(rest)?;
                window.dispatch_event(
                    gpui_kit::PlatformInput::MouseMove(gpui_kit::MouseMoveEvent { position, pressed_button: None, modifiers }),
                    cx,
                );
                Ok(json!({ "ok": true }))
            }
            "click" | "rclick" | "dblclick" => {
                // `click <x> <y> [modifiers]` in window points (screenshot pixels / scale
                // factor), e.g. `click 10 20 cmd`; `rclick` uses the right button (context
                // menus).
                let (position, modifiers) = pointer(rest)?;
                window.dispatch_event(
                    gpui_kit::PlatformInput::MouseMove(gpui_kit::MouseMoveEvent { position, pressed_button: None, modifiers }),
                    cx,
                );
                let button = if command == "rclick" { gpui_kit::MouseButton::Right } else { gpui_kit::MouseButton::Left };
                // `dblclick`: a first click, then a second with the platform's count of 2.
                let counts: &[usize] = if command == "dblclick" { &[1, 2] } else { &[1] };
                for &click_count in counts {
                    window.dispatch_event(
                        gpui_kit::PlatformInput::MouseDown(gpui_kit::MouseDownEvent {
                            button,
                            position,
                            modifiers,
                            click_count,
                            first_mouse: false,
                        }),
                        cx,
                    );
                    window.dispatch_event(
                        gpui_kit::PlatformInput::MouseUp(gpui_kit::MouseUpEvent { button, position, modifiers, click_count }),
                        cx,
                    );
                }
                Ok(json!({ "ok": true }))
            }
            "scroll" => {
                // `scroll <x> <y> <dy>`: a wheel scroll of `dy` points over (x, y); a negative
                // `dy` scrolls the content up (reveals what is below).
                let coords: Vec<f32> = rest.split_whitespace().filter_map(|v| v.parse().ok()).collect();
                let [x, y, dy] = coords[..] else {
                    return Err("usage: scroll <x> <y> <dy>".to_owned());
                };
                let position = gpui_kit::point(gpui_kit::px(x), gpui_kit::px(y));
                let modifiers = gpui_kit::Modifiers::default();
                window.dispatch_event(
                    gpui_kit::PlatformInput::MouseMove(gpui_kit::MouseMoveEvent { position, pressed_button: None, modifiers }),
                    cx,
                );
                window.dispatch_event(
                    gpui_kit::PlatformInput::Scroll(gpui_kit::ScrollEvent {
                        position,
                        delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(gpui_kit::px(0.), gpui_kit::px(dy))),
                        modifiers,
                        touch_phase: gpui_kit::TouchPhase::Moved,
                        source: gpui_kit::ScrollSource::Pointer,
                    }),
                    cx,
                );
                Ok(json!({ "ok": true }))
            }
            "key" => {
                let keystroke = Keystroke::parse_with_os(rest, cx.platform_info().os)
                    .map_err(|e| e.to_string())?;
                let handled = window.dispatch_keystroke(keystroke, cx);
                Ok(json!({ "ok": true, "handled": handled }))
            }
            "screenshot" => {
                let image = window.render_to_image().map_err(|e| e.to_string())?;
                image.save(rest).map_err(|e| e.to_string())?;
                Ok(json!({ "ok": true, "width": image.width(), "height": image.height() }))
            }
            other => Err(format!("unknown command {other:?}")),
        }
    });
    match result {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => json!({ "error": error }),
        Err(error) => json!({ "error": error.to_string() }),
    }
}
