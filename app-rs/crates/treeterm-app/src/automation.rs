//! Debug-only control socket for end-to-end tests and screenshots.
//!
//! Enabled with the `automation` cargo feature and `TREETERM_AUTOMATION_SOCKET=<path>`.
//! One command per line, one JSON reply per line:
//!
//! - `state` — what the window shows (workspaces, selection, terminal screens)
//! - `select <workspace-id>` / `tab <workspace-id> <tab-id>`
//! - `newtab <workspace-id>` / `closetab <workspace-id> <tab-id>`
//! - `open <directory>` — open a directory as a workspace (no folder picker)
//! - `type <text>` — type text as keystrokes; `key <keystroke>` — e.g. `enter`, `ctrl-c`
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

fn handle(line: &str, window: AnyWindowHandle, view: &Entity<AppView>, cx: &mut AsyncApp) -> Value {
    let (command, rest) = line.split_once(' ').unwrap_or((line, ""));
    let result = window.update(cx, |_, window, cx| -> Result<Value, String> {
        // An occluded window gets no frames from the display link; render one so the reply
        // reflects the current state (views are built and sized while rendering).
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
            "childdialog" => {
                let session = view.read(cx).automation_session().ok_or("not connected")?;
                let on_created: crate::child_dialog::OnCreated = std::rc::Rc::new(|_, _, _| {});
                crate::child_dialog::open(session, rest.to_owned(), "parent".into(), on_created, window, cx);
                Ok(json!({ "ok": true }))
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
