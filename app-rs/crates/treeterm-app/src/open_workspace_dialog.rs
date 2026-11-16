//! "Open Workspace": `OpenWorkspaceDialog.tsx`. A path is typed, picked from the recent
//! directories, or "Browse..." picks one: the native folder picker on this machine,
//! `RemoteDirectoryBrowser.tsx` on a remote host (the native picker only sees this machine).

use std::rc::Rc;

use gpui_kit::component::WindowExt as _;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::prelude::*;
use gpui_kit::{App, Context, Entity, FontWeight, SharedString, Subscription, Window, div, px};
use treeterm_model::session_store::SessionStore;

use crate::remote_dir_browser::{Inline, RemoteDirBrowserEvent};
use crate::theme::colors;
use crate::ui;

/// Called with the chosen directory.
pub type OnPicked = Rc<dyn Fn(String, &mut Window, &mut App)>;

/// Where the directory lives.
pub enum Location {
    Local,
    Remote(Entity<SessionStore>),
}

/// What "Browse..." does.
enum Picker {
    /// The native folder picker; `picking` while it is up ("Opening...").
    Native { picking: bool },
    /// The remote filesystem, browsed under the folder field while `browser` is shown.
    Remote { session: Entity<SessionStore>, browser: Inline },
}

pub struct OpenWorkspaceDialog {
    folder: Entity<InputState>,
    /// "Recent Directories", most recent first; the section is hidden when empty.
    recent: Vec<String>,
    picker: Picker,
    on_picked: OnPicked,
    _subscriptions: Vec<Subscription>,
}

/// `getFolderName`: the last path segment, else the path itself.
fn folder_name(path: &str) -> &str {
    match path.rsplit(['/', '\\']).next() {
        Some("") | None => path,
        Some(name) => name,
    }
}

impl OpenWorkspaceDialog {
    fn new(
        location: Location,
        recent: Vec<String>,
        on_picked: OnPicked,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (picker, placeholder) = match location {
            Location::Local => (Picker::Native { picking: false }, "Enter or browse for a folder..."),
            Location::Remote(session) => {
                (Picker::Remote { session, browser: Inline::Hidden }, "Enter remote path (e.g. /home/user/project)")
            }
        };
        let folder = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        let subscription = cx.subscribe_in(&folder, window, |dialog, _, event: &InputEvent, window, cx| match event {
            // `handleKeyDown`: Enter opens.
            InputEvent::PressEnter { .. } => dialog.open_selected(window, cx),
            InputEvent::Change => cx.notify(),
            InputEvent::Focus | InputEvent::Blur => {}
        });
        Self { folder, recent, picker, on_picked, _subscriptions: vec![subscription] }
    }

    fn selected(&self, cx: &App) -> String {
        self.folder.read(cx).value().to_string()
    }

    /// `isSelecting`: Open waits for the native picker.
    fn picking(&self) -> bool {
        matches!(self.picker, Picker::Native { picking: true })
    }

    fn open_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let path = self.selected(cx);
        if path.is_empty() || self.picking() {
            return;
        }
        window.close_dialog(cx);
        (self.on_picked)(path, window, cx);
    }

    fn browse(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match &mut self.picker {
            Picker::Native { picking: true } => {}
            Picker::Native { picking } => {
                *picking = true;
                self.pick_native(window, cx);
            }
            Picker::Remote { browser: Inline::Shown { .. }, .. } => self.hide_browser(cx), // "Hide"
            Picker::Remote { session, browser: Inline::Hidden } => {
                let session = session.clone();
                self.show_browser(session, window, cx);
            }
        }
    }

    /// `handleSelectFolder`: the chosen folder replaces the typed path.
    fn pick_native(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Open Workspace".into()),
        });
        cx.spawn_in(window, async move |dialog, cx| {
            let path = match picked.await {
                Ok(Ok(Some(mut paths))) if !paths.is_empty() => Some(paths.remove(0)),
                Ok(Ok(_)) | Err(_) => None, // cancelled
                Ok(Err(error)) => {
                    tracing::error!(%error, "folder picker failed");
                    None
                }
            };
            match dialog.update_in(cx, |dialog, window, cx| {
                dialog.picker = Picker::Native { picking: false };
                if let Some(path) = path {
                    let path = path.to_string_lossy().into_owned();
                    dialog.folder.update(cx, |input, cx| input.set_value(path, window, cx));
                }
                cx.notify();
            }) {
                Ok(()) | Err(_) => {} // Err: the dialog closed
            }
        })
        .detach();
        cx.notify();
    }

    /// "Browse..." on a remote host: the browser starts at the typed path, else the root.
    /// Select puts its directory into the folder field; both Select and Cancel hide it.
    fn show_browser(&mut self, session: Entity<SessionStore>, window: &mut Window, cx: &mut Context<Self>) {
        let fs = session.read(cx).connection().fs_client();
        let typed = self.selected(cx);
        let start = if typed.is_empty() { "/".to_owned() } else { typed };
        let browser = Inline::show(fs, start, window, cx, |dialog: &mut Self, event, window, cx| {
            if let RemoteDirBrowserEvent::Selected(path) = event {
                dialog.folder.update(cx, |input, cx| input.set_value(path.clone(), window, cx));
            }
            dialog.hide_browser(cx);
        });
        self.picker = Picker::Remote { session, browser };
        cx.notify();
    }

    fn hide_browser(&mut self, cx: &mut Context<Self>) {
        if let Picker::Remote { browser, .. } = &mut self.picker {
            *browser = Inline::Hidden;
        }
        cx.notify();
    }

    /// `.recent-directories-list`: each row the folder name over its path; a click fills the
    /// folder field (`handleSelectRecent`).
    fn render_recent(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let c = colors(cx);
        let (primary, secondary, border, hover, tertiary) =
            (c.text_primary, c.text_secondary, c.border, c.bg_hover, c.bg_tertiary);
        let mut list = div().id("recent-directories").flex().flex_col().gap(px(4.)).max_h(px(200.)).overflow_y_scroll();
        for (i, dir) in self.recent.iter().enumerate() {
            let path = dir.clone();
            list = list.child(
                div()
                    .id(SharedString::from(format!("recent-{i}")))
                    .flex()
                    .flex_col()
                    .flex_shrink_0()
                    .px(px(12.))
                    .py(px(8.))
                    .rounded(px(4.))
                    .border_1()
                    .border_color(border)
                    .bg(tertiary)
                    .cursor_pointer()
                    .hover(move |d| d.bg(hover))
                    .child(
                        div()
                            .text_size(px(ui::BODY))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(primary)
                            .child(folder_name(dir).to_owned()),
                    )
                    .child(div().truncate().text_size(px(11.)).text_color(secondary).child(dir.clone()))
                    .on_click(cx.listener(move |dialog, _, window, cx| {
                        let path = path.clone();
                        dialog.folder.update(cx, |input, cx| input.set_value(path, window, cx));
                    })),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(
                div()
                    .text_size(px(12.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(secondary)
                    .child("Recent Directories"),
            )
            .child(list)
            .into_any_element()
    }
}

impl Render for OpenWorkspaceDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors(cx);
        let (primary, secondary, border, hover, tertiary) =
            (c.text_primary, c.text_secondary, c.border, c.bg_hover, c.bg_tertiary);
        let picking = self.picking();
        let label = match &self.picker {
            Picker::Native { picking: true } => "Opening...",
            Picker::Remote { browser: Inline::Shown { .. }, .. } => "Hide",
            Picker::Native { picking: false } | Picker::Remote { browser: Inline::Hidden, .. } => "Browse...",
        };
        // `.open-workspace-dialog .dialog-btn.browse`: bg-tertiary with a border; disabled
        // while the native picker is up.
        let browse = div()
            .id("browse")
            .flex_shrink_0()
            .px(px(16.))
            .py(px(8.))
            .rounded(px(4.))
            .border_1()
            .border_color(border)
            .bg(tertiary)
            .text_size(px(ui::BODY))
            .line_height(px(15.5))
            .text_color(primary)
            .when(picking, |d| d.opacity(0.5).cursor_not_allowed())
            .when(!picking, |d| d.cursor_pointer().hover(move |d| d.bg(hover)))
            .child(label)
            .on_click(cx.listener(|dialog, _, window, cx| dialog.browse(window, cx)));
        let browser = match &self.picker {
            Picker::Remote { browser, .. } => browser.view(),
            Picker::Native { .. } => None,
        };
        let recent = (!self.recent.is_empty()).then(|| self.render_recent(cx));
        let disabled = self.selected(cx).is_empty() || picking;
        let open = ui::dialog_button("open-submit", "Open", ui::ButtonKind::Primary, disabled, cx)
            .when(!disabled, |b| b.on_click(cx.listener(|dialog, _, window, cx| dialog.open_selected(window, cx))));
        ui::dialog_root(cx)
            .child(ui::dialog_header("Open Workspace", "×", primary, cx))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .p(px(20.))
                    .children(recent)
                    .child(
                        // `.open-workspace-field`: a 12px label 6px above the row.
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(
                                div().text_size(px(12.)).font_weight(FontWeight::MEDIUM).text_color(secondary).child("Folder"),
                            )
                            .child(
                                div()
                                    .flex()
                                    .gap(px(8.))
                                    .child(div().flex_1().min_w_0().child(ui::text_input(&self.folder, cx)))
                                    .child(browse),
                            ),
                    )
                    .children(browser),
            )
            .child(
                ui::dialog_actions(cx)
                    .child(
                        ui::dialog_button("open-cancel", "Cancel", ui::ButtonKind::Secondary, false, cx)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(open),
            )
    }
}

/// `recent`: the "Recent Directories" to offer, most recent first (none hides the section).
pub fn open(location: Location, recent: Vec<String>, on_picked: OnPicked, window: &mut Window, cx: &mut App) {
    let view = cx.new(|cx| OpenWorkspaceDialog::new(location, recent, on_picked, window, cx));
    // `autoFocus` on the folder, once the dialog has taken the focus.
    let folder = view.read(cx).folder.clone();
    window.defer(cx, move |window, cx| folder.update(cx, |input, cx| input.focus(window, cx)));
    window.open_dialog(cx, move |dialog, _window, cx| ui::dialog_frame(dialog, 500., cx).child(view.clone()));
}

#[cfg(test)]
mod tests {
    use super::folder_name;

    #[test]
    fn folder_names_are_the_last_segment() {
        assert_eq!(folder_name("/home/me/src"), "src");
        assert_eq!(folder_name("C:\\work\\repo"), "repo");
        assert_eq!(folder_name("/home/me/"), "/home/me/");
        assert_eq!(folder_name("repo"), "repo");
    }
}
