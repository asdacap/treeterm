//! Pick a directory on a remote daemon's machine (`RemoteDirectoryBrowser.tsx`): the native
//! folder picker only sees this machine.

use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{App, Context, Entity, SharedString, Subscription, Task, Window, div, px};
use treeterm_model::session_store::SessionStore;

use crate::theme::colors;

/// Called with the chosen directory.
pub type OnPicked = Rc<dyn Fn(String, &mut Window, &mut App)>;

enum Listing {
    Loading,
    Loaded(Vec<(String, String)>),
    Failed(String),
}

pub struct RemoteDirDialog {
    session: Entity<SessionStore>,
    current: String,
    path: Entity<InputState>,
    listing: Listing,
    _load: Option<Task<()>>,
    _enter: Subscription,
}

/// `/a/b/c` → `/a/b`; `/a` → `/`.
fn parent_of(path: &str) -> String {
    match path.trim_end_matches('/').rsplit_once('/') {
        Some(("", _)) | None => "/".to_owned(),
        Some((parent, _)) => parent.to_owned(),
    }
}

impl RemoteDirDialog {
    fn new(session: Entity<SessionStore>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // Start next to the session's first workspace, else at the root.
        let start = {
            let s = session.read(cx);
            s.workspace_ids()
                .filter_map(|id| s.workspace_store(id))
                .map(|w| parent_of(&w.read(cx).workspace().path))
                .next()
                .unwrap_or_else(|| "/".to_owned())
        };
        let path = cx.new(|cx| InputState::new(window, cx).default_value(start.clone()));
        let enter = cx.subscribe_in(&path, window, |dialog, input, event: &InputEvent, _, cx| {
            if let InputEvent::PressEnter { .. } = event {
                let typed = input.read(cx).value().trim().to_owned();
                if typed.starts_with('/') {
                    dialog.navigate(typed, None, cx);
                }
            }
        });
        let mut dialog = Self {
            session,
            current: String::new(),
            path,
            listing: Listing::Loading,
            _load: None,
            _enter: enter,
        };
        dialog.navigate(start, None, cx);
        dialog
    }

    /// Show `path`; `window` updates the path box when the move came from a click.
    fn navigate(&mut self, path: String, window: Option<&mut Window>, cx: &mut Context<Self>) {
        if let Some(window) = window {
            self.path.update(cx, |input, cx| input.set_value(path.clone(), window, cx));
        }
        self.current = path.clone();
        self.listing = Listing::Loading;
        let read = self.session.read(cx).connection().fs_client().read_directory("/", &path);
        self._load = Some(cx.spawn(async move |this, cx| {
            let listing = match read.await {
                Ok(Ok(contents)) => {
                    let mut dirs: Vec<(String, String)> = contents
                        .entries
                        .into_iter()
                        .filter(|e| e.is_directory)
                        .map(|e| (e.name, e.path))
                        .collect();
                    dirs.sort_by_key(|(name, _)| name.to_lowercase());
                    Listing::Loaded(dirs)
                }
                Ok(Err(error)) => Listing::Failed(error.to_string()),
                Err(lost) => Listing::Failed(lost.to_string()),
            };
            match this.update(cx, |dialog, cx| {
                dialog.listing = listing;
                cx.notify();
            }) {
                Ok(()) | Err(_) => {} // Err: dialog closed
            }
        }));
        cx.notify();
    }
}

impl Render for RemoteDirDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors(cx);
        let (secondary, error, hover) = (c.text_secondary, c.error, c.bg_hover);
        let at_root = self.current == "/";
        let up = parent_of(&self.current);
        let list = match &self.listing {
            Listing::Loading => div().p_2().text_sm().text_color(secondary).child("Loading…").into_any_element(),
            Listing::Failed(message) => div().p_2().text_sm().text_color(error).child(message.clone()).into_any_element(),
            Listing::Loaded(dirs) if dirs.is_empty() => {
                div().p_2().text_sm().text_color(secondary).child("No subdirectories").into_any_element()
            }
            Listing::Loaded(dirs) => {
                let mut list = v_flex();
                for (i, (name, path)) in dirs.iter().enumerate() {
                    let path = path.clone();
                    list = list.child(
                        div()
                            .id(SharedString::from(format!("dir-{i}")))
                            .px_2()
                            .py_1()
                            .text_sm()
                            .cursor_pointer()
                            .hover(move |d| d.bg(hover))
                            .child(format!("📁 {name}"))
                            .on_click(cx.listener(move |dialog, _, window, cx| dialog.navigate(path.clone(), Some(window), cx))),
                    );
                }
                list.into_any_element()
            }
        };
        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .gap_2()
                    .child(Button::new("dir-up").small().ghost().label("..").disabled(at_root).on_click(
                        cx.listener(move |dialog, _, window, cx| dialog.navigate(up.clone(), Some(window), cx)),
                    ))
                    .child(div().flex_1().child(Input::new(&self.path).small())),
            )
            .child(div().id("dir-list").h(px(300.)).overflow_y_scroll().border_1().border_color(c.border).child(list))
    }
}

use gpui_kit::component::Disableable as _;

pub fn open(session: Entity<SessionStore>, on_picked: OnPicked, window: &mut Window, cx: &mut App) {
    let view = cx.new(|cx| RemoteDirDialog::new(session, window, cx));
    window.open_dialog(cx, move |dialog, _window, _cx| {
        let (pick, on_picked) = (view.clone(), on_picked.clone());
        dialog
            .title("Open Remote Workspace")
            .w(px(520.))
            .child(view.clone())
            .footer(h_flex().justify_end().child(Button::new("open-remote").primary().label("Open").on_click(
                move |_, window, cx| {
                    let path = pick.read(cx).current.clone();
                    window.close_dialog(cx);
                    on_picked(path, window, cx);
                },
            )))
    });
}

#[cfg(test)]
mod tests {
    use super::parent_of;

    #[test]
    fn parents_stop_at_the_root() {
        assert_eq!(parent_of("/home/me/src"), "/home/me");
        assert_eq!(parent_of("/home"), "/");
        assert_eq!(parent_of("/"), "/");
        assert_eq!(parent_of("/home/me/"), "/home");
    }
}
