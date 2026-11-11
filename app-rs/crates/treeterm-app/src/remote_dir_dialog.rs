//! "Open Workspace" for a remote session: `OpenWorkspaceDialog.tsx` with its
//! `RemoteDirectoryBrowser.tsx` (the native folder picker only sees this machine). A path is
//! typed, or picked by browsing the remote filesystem.

use std::rc::Rc;

use gpui_kit::component::WindowExt as _;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::prelude::*;
use gpui_kit::{App, Context, Entity, FontWeight, SharedString, Subscription, Task, Window, div, px};
use treeterm_model::session_store::SessionStore;

use crate::theme::colors;
use crate::ui;

/// Called with the chosen directory.
pub type OnPicked = Rc<dyn Fn(String, &mut Window, &mut App)>;

enum Listing {
    Loading,
    Loaded(Vec<(String, String)>),
    Failed(String),
}

/// `RemoteDirectoryBrowser`: shown under the folder field by "Browse...".
struct Browser {
    current: String,
    path: Entity<InputState>,
    listing: Listing,
    _load: Option<Task<()>>,
}

pub struct RemoteDirDialog {
    session: Entity<SessionStore>,
    folder: Entity<InputState>,
    browser: Option<Browser>,
    on_picked: OnPicked,
    _subscriptions: Vec<Subscription>,
}

/// `/a/b/c` → `/a/b`; `/a` → `/`.
fn parent_of(path: &str) -> String {
    match path.trim_end_matches('/').rsplit_once('/') {
        Some(("", _)) | None => "/".to_owned(),
        Some((parent, _)) => parent.to_owned(),
    }
}

impl RemoteDirDialog {
    fn new(session: Entity<SessionStore>, on_picked: OnPicked, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let folder = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Enter remote path (e.g. /home/user/project)")
        });
        let subscription = cx.subscribe_in(&folder, window, |dialog, _, event: &InputEvent, window, cx| match event {
            // `handleKeyDown`: Enter opens.
            InputEvent::PressEnter { .. } => dialog.open_selected(window, cx),
            InputEvent::Change => cx.notify(),
            InputEvent::Focus | InputEvent::Blur => {}
        });
        Self { session, folder, browser: None, on_picked, _subscriptions: vec![subscription] }
    }

    fn selected(&self, cx: &App) -> String {
        self.folder.read(cx).value().to_string()
    }

    fn open_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let path = self.selected(cx);
        if path.is_empty() {
            return;
        }
        window.close_dialog(cx);
        (self.on_picked)(path, window, cx);
    }

    /// "Browse..." / "Hide": the browser starts at the typed path, else the root.
    fn toggle_browser(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.browser.take().is_some() {
            cx.notify();
            return;
        }
        let typed = self.selected(cx);
        let start = if typed.is_empty() { "/".to_owned() } else { typed };
        let path = cx.new(|cx| {
            InputState::new(window, cx).placeholder("/path/to/directory").default_value(start.clone())
        });
        let enter = cx.subscribe_in(&path, window, |dialog, input, event: &InputEvent, window, cx| {
            if let InputEvent::PressEnter { .. } = event {
                let typed = input.read(cx).value().trim().to_owned();
                if typed.starts_with('/') {
                    dialog.navigate(typed, window, cx);
                }
            }
        });
        self._subscriptions.truncate(1);
        self._subscriptions.push(enter);
        self.browser = Some(Browser { current: String::new(), path, listing: Listing::Loading, _load: None });
        self.navigate(start, window, cx);
    }

    fn navigate(&mut self, path: String, window: &mut Window, cx: &mut Context<Self>) {
        let read = self.session.read(cx).connection().fs_client().read_directory("/", &path);
        let Some(browser) = self.browser.as_mut() else {
            return;
        };
        browser.path.update(cx, |input, cx| input.set_value(path.clone(), window, cx));
        browser.current = path;
        browser.listing = Listing::Loading;
        browser._load = Some(cx.spawn(async move |this, cx| {
            let listing = match read.await {
                Ok(Ok(contents)) => Listing::Loaded(
                    contents.entries.into_iter().filter(|e| e.is_directory).map(|e| (e.name, e.path)).collect(),
                ),
                Ok(Err(error)) => Listing::Failed(error.to_string()),
                Err(lost) => Listing::Failed(lost.to_string()),
            };
            match this.update(cx, |dialog, cx| {
                if let Some(browser) = dialog.browser.as_mut() {
                    browser.listing = listing;
                }
                cx.notify();
            }) {
                Ok(()) | Err(_) => {} // Err: the dialog closed
            }
        }));
        cx.notify();
    }

    fn render_browser(&self, browser: &Browser, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let c = colors(cx);
        let (primary, secondary, accent, border, hover, bg) =
            (c.text_primary, c.text_secondary, c.accent, c.border, c.bg_hover, c.bg_secondary);
        let at_root = browser.current == "/";
        let up = parent_of(&browser.current);
        // `.remote-dir-up-btn`: "..", 4px 10px, bold 13px; dimmed at the root.
        let up_button = div()
            .id("dir-up")
            .px(px(10.))
            .py(px(4.))
            .rounded(px(4.))
            .border_1()
            .border_color(border)
            .bg(bg)
            .text_size(px(ui::BODY))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(primary)
            .child("..")
            .when(at_root, |d| d.opacity(0.4))
            .when(!at_root, |d| {
                d.cursor_pointer()
                    .hover(move |d| d.bg(hover))
                    .on_click(cx.listener(move |dialog, _, window, cx| dialog.navigate(up.clone(), window, cx)))
            });
        let path_input = gpui_kit::Styled::h(Input::new(&browser.path), px(24.))
            .flex_1()
            .px(px(8.))
            .py(px(4.))
            .bg(bg)
            .border_color(border)
            .rounded(px(4.))
            .text_size(px(12.))
            .font_family("Menlo");
        // `.remote-dir-breadcrumb`: each segment a link back up; the last one plain.
        let segments: Vec<String> = if at_root {
            vec!["/".to_owned()]
        } else {
            std::iter::once("/".to_owned())
                .chain(browser.current.split('/').filter(|s| !s.is_empty()).map(str::to_owned))
                .collect()
        };
        let mut crumbs = div().flex().flex_wrap().items_center().gap(px(2.)).py(px(2.)).text_size(px(12.));
        let count = segments.len();
        for (i, segment) in segments.iter().enumerate() {
            let target = if i == 0 { "/".to_owned() } else { format!("/{}", segments[1..=i].join("/")) };
            let last = i + 1 == count;
            crumbs = crumbs.child(
                div()
                    .id(SharedString::from(format!("crumb-{i}")))
                    .px(px(2.))
                    .py(px(1.))
                    .rounded(px(2.))
                    .when(last, |d| d.text_color(primary).font_weight(FontWeight::MEDIUM))
                    .when(!last, |d| {
                        d.text_color(accent)
                            .cursor_pointer()
                            .hover(move |d| d.bg(hover))
                            .on_click(cx.listener(move |dialog, _, window, cx| dialog.navigate(target.clone(), window, cx)))
                    })
                    .child(segment.clone()),
            );
            if !last {
                crumbs = crumbs.child(div().text_color(secondary).child("/"));
            }
        }
        let status = |text: String, color| div().p(px(8.)).text_size(px(12.)).text_color(color).child(text);
        let mut list = div().id("dir-list").flex().flex_col().gap(px(2.)).max_h(px(250.)).overflow_y_scroll();
        list = match &browser.listing {
            Listing::Loading => list.child(status("Loading...".to_owned(), secondary)),
            Listing::Failed(message) => list.child(status(message.clone(), c.error)),
            Listing::Loaded(dirs) if dirs.is_empty() => list.child(status("No subdirectories".to_owned(), secondary)),
            Listing::Loaded(dirs) => {
                for (i, (name, path)) in dirs.iter().enumerate() {
                    let path = path.clone();
                    list = list.child(
                        div()
                            .id(SharedString::from(format!("dir-{i}")))
                            .flex()
                            .items_center()
                            .px(px(8.))
                            .py(px(5.))
                            .rounded(px(3.))
                            .border_1()
                            .border_color(gpui_kit::transparent_black())
                            .text_size(px(ui::BODY))
                            .text_color(primary)
                            .cursor_pointer()
                            .hover(move |d| d.bg(hover).border_color(border))
                            .child(name.clone())
                            .on_click(cx.listener(move |dialog, _, window, cx| dialog.navigate(path.clone(), window, cx))),
                    );
                }
                list
            }
        };
        let current = browser.current.clone();
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .p(px(8.))
            .rounded(px(4.))
            .border_1()
            .border_color(border)
            .bg(c.bg_tertiary)
            .child(div().flex().gap(px(4.)).child(up_button).child(path_input))
            .child(crumbs)
            .child(list)
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.))
                    .pt(px(4.))
                    .border_t_1()
                    .border_color(border)
                    .child(ui::dialog_button("dir-cancel", "Cancel", ui::ButtonKind::Secondary, false, cx).on_click(
                        cx.listener(|dialog, _, _, cx| {
                            dialog.browser = None;
                            cx.notify();
                        }),
                    ))
                    .child(ui::dialog_button("dir-select", "Select", ui::ButtonKind::Primary, false, cx).on_click(
                        cx.listener(move |dialog, _, window, cx| {
                            let path = current.clone();
                            dialog.folder.update(cx, |input, cx| input.set_value(path, window, cx));
                            dialog.browser = None;
                            cx.notify();
                        }),
                    )),
            )
            .into_any_element()
    }
}

impl Render for RemoteDirDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors(cx);
        let (primary, secondary, border, hover, tertiary) =
            (c.text_primary, c.text_secondary, c.border, c.bg_hover, c.bg_tertiary);
        let showing = self.browser.is_some();
        // `.open-workspace-dialog .dialog-btn.browse`: bg-tertiary with a border.
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
            .cursor_pointer()
            .hover(move |d| d.bg(hover))
            .child(if showing { "Hide" } else { "Browse..." })
            .on_click(cx.listener(|dialog, _, window, cx| dialog.toggle_browser(window, cx)));
        let browser = self.browser.as_ref().map(|browser| self.render_browser(browser, cx));
        let empty = self.selected(cx).is_empty();
        let open = ui::dialog_button("open-remote", "Open", ui::ButtonKind::Primary, empty, cx)
            .when(!empty, |b| b.on_click(cx.listener(|dialog, _, window, cx| dialog.open_selected(window, cx))));
        ui::dialog_root(cx)
            .child(ui::dialog_header("Open Workspace", "×", primary, cx))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .p(px(20.))
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

pub fn open(session: Entity<SessionStore>, on_picked: OnPicked, window: &mut Window, cx: &mut App) {
    let view = cx.new(|cx| RemoteDirDialog::new(session, on_picked, window, cx));
    // `autoFocus` on the folder, once the dialog has taken the focus.
    let folder = view.read(cx).folder.clone();
    window.defer(cx, move |window, cx| folder.update(cx, |input, cx| input.focus(window, cx)));
    window.open_dialog(cx, move |dialog, _window, cx| ui::dialog_frame(dialog, 500., cx).child(view.clone()));
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
