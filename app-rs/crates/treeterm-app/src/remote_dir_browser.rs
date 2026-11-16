//! `RemoteDirectoryBrowser.tsx`, shown inline by Open Workspace on a remote session and by
//! SSH Upload's "Browse...". The host decides what Select and Cancel do.

use gpui_kit::component::input::{Escape, Input, InputEvent, InputState};
use gpui_kit::prelude::*;
use gpui_kit::{
    Context, Entity, EventEmitter, FontWeight, SharedString, Subscription, Window, div, px,
    relative,
};
use treeterm_core::fs::FsClient;
use treeterm_model::dir_browser::{DirBrowser, DirBrowserEvent, Listing};

use crate::theme::colors;
use crate::ui;

/// Electron's `<button>`s keep the system font and `line-height: normal` inside the
/// monospace SSH pane.
const BUTTON_FONT: &str = ".SystemUIFont";

pub enum RemoteDirBrowserEvent {
    /// "Select": the directory the browser is in.
    Selected(String),
    /// "Cancel", or Escape in the path field.
    Cancelled,
}

pub struct RemoteDirBrowserView {
    browser: Entity<DirBrowser>,
    /// The editable path; Enter goes there.
    path: Entity<InputState>,
    _subscriptions: [Subscription; 3],
}

impl EventEmitter<RemoteDirBrowserEvent> for RemoteDirBrowserView {}

/// A host's browser: hidden, or shown with the host's subscription to its Select and Cancel.
/// Hiding drops both.
pub enum Inline {
    Hidden,
    Shown {
        view: Entity<RemoteDirBrowserView>,
        _events: Subscription,
    },
}

impl Inline {
    /// Show a browser of `fs`'s host from `start`; `on_event` handles its Select and Cancel.
    pub fn show<V: 'static>(
        fs: FsClient,
        start: String,
        window: &mut Window,
        cx: &mut Context<V>,
        on_event: impl Fn(&mut V, &RemoteDirBrowserEvent, &mut Window, &mut Context<V>) + 'static,
    ) -> Self {
        let view = cx.new(|cx| RemoteDirBrowserView::new(fs, start, window, cx));
        let events = cx.subscribe_in(&view, window, move |host, _, event, window, cx| {
            on_event(host, event, window, cx)
        });
        Inline::Shown {
            view,
            _events: events,
        }
    }

    /// The browser to render, while it is shown.
    pub fn view(&self) -> Option<Entity<RemoteDirBrowserView>> {
        match self {
            Inline::Hidden => None,
            Inline::Shown { view, .. } => Some(view.clone()),
        }
    }
}

impl RemoteDirBrowserView {
    /// Browse `fs`'s host from `start` (`initialPath`).
    pub fn new(fs: FsClient, start: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let path = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("/path/to/directory")
                .default_value(start.clone())
        });
        let browser = cx.new(|cx| DirBrowser::new(fs, start, cx));
        // `handlePathKeyDown`: Enter submits the typed path.
        let enter = cx.subscribe(&path, |view, input, event: &InputEvent, cx| {
            if let InputEvent::PressEnter { .. } = event {
                let typed = input.read(cx).value().to_string();
                view.browser
                    .update(cx, |browser, cx| browser.submit(&typed, cx));
            }
        });
        let navigated = cx.subscribe_in(
            &browser,
            window,
            |view, _, event: &DirBrowserEvent, window, cx| match event {
                DirBrowserEvent::Navigated(path) => {
                    view.path
                        .update(cx, |input, cx| input.set_value(path.clone(), window, cx));
                }
            },
        );
        let changed = cx.observe(&browser, |_, _, cx| cx.notify());
        Self {
            browser,
            path,
            _subscriptions: [enter, navigated, changed],
        }
    }

    fn navigate(&mut self, path: String, cx: &mut Context<Self>) {
        self.browser
            .update(cx, |browser, cx| browser.navigate(path, cx));
    }
}

impl Render for RemoteDirBrowserView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors(cx);
        let (primary, secondary, accent, border, hover, bg) = (
            c.text_primary,
            c.text_secondary,
            c.accent,
            c.border,
            c.bg_hover,
            c.bg_secondary,
        );
        let browser = self.browser.read(cx);
        let (at_root, current, crumbs, listing) = (
            browser.at_root(),
            browser.current().to_owned(),
            browser.crumbs(),
            browser.listing().clone(),
        );
        // `.remote-dir-up-btn`: "..", 4px 10px, bold 13px; dimmed at the root.
        let up_button = div()
            .id("dir-up")
            .px(px(10.))
            .py(px(4.))
            .rounded(px(4.))
            .border_1()
            .border_color(border)
            .bg(bg)
            .font_family(BUTTON_FONT)
            .text_size(px(ui::BODY))
            .line_height(relative(1.2))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(primary)
            .child("..")
            .when(at_root, |d| d.opacity(0.4))
            .when(!at_root, |d| {
                d.cursor_pointer()
                    .hover(move |d| d.bg(hover))
                    .on_click(cx.listener(|view, _, _, cx| {
                        view.browser.update(cx, |browser, cx| browser.go_up(cx))
                    }))
            });
        let path_input = gpui_kit::Styled::h(Input::new(&self.path), px(24.))
            .flex_1()
            .px(px(8.))
            .py(px(4.))
            .bg(bg)
            .border_color(border)
            .rounded(px(4.))
            .text_size(px(12.))
            .font_family(ui::MONO);
        // `.remote-dir-breadcrumb`: each segment a link back up; the last one plain.
        let mut breadcrumb = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(2.))
            .py(px(2.))
            .text_size(px(12.));
        let count = crumbs.len();
        for (i, crumb) in crumbs.into_iter().enumerate() {
            let last = i + 1 == count;
            let target = crumb.target;
            breadcrumb =
                breadcrumb.child(
                    div()
                        .id(SharedString::from(format!("crumb-{i}")))
                        .px(px(2.))
                        .py(px(1.))
                        .rounded(px(2.))
                        .font_family(BUTTON_FONT)
                        .line_height(relative(1.2))
                        .when(last, |d| {
                            d.text_color(primary).font_weight(FontWeight::MEDIUM)
                        })
                        .when(!last, |d| {
                            d.text_color(accent)
                                .cursor_pointer()
                                .hover(move |d| d.bg(hover))
                                .on_click(cx.listener(move |view, _, _, cx| {
                                    view.navigate(target.clone(), cx)
                                }))
                        })
                        .child(crumb.label),
                );
            if !last {
                breadcrumb = breadcrumb.child(div().text_color(secondary).child("/"));
            }
        }
        let status = |text: String, color| {
            div()
                .p(px(8.))
                .text_size(px(12.))
                .text_color(color)
                .child(text)
        };
        let mut list = div()
            .id("dir-list")
            .flex()
            .flex_col()
            .gap(px(2.))
            .max_h(px(250.))
            .overflow_y_scroll();
        list = match listing {
            Listing::Loading => list.child(status("Loading...".to_owned(), secondary)),
            Listing::Failed(message) => list.child(status(message, c.error)),
            Listing::Loaded(dirs) if dirs.is_empty() => {
                list.child(status("No subdirectories".to_owned(), secondary))
            }
            Listing::Loaded(dirs) => {
                for (i, dir) in dirs.into_iter().enumerate() {
                    let path = dir.path;
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
                            .font_family(BUTTON_FONT)
                            .text_size(px(ui::BODY))
                            .line_height(relative(1.2))
                            .text_color(primary)
                            .cursor_pointer()
                            .hover(move |d| d.bg(hover).border_color(border))
                            .child(dir.name)
                            .on_click(
                                cx.listener(move |view, _, _, cx| view.navigate(path.clone(), cx)),
                            ),
                    );
                }
                list
            }
        };
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .p(px(8.))
            .rounded(px(4.))
            .border_1()
            .border_color(border)
            .bg(c.bg_tertiary)
            .child(
                div()
                    .flex()
                    .gap(px(4.))
                    // Escape in the path field: `onCancel`. It goes on up too, as the key
                    // event does in Electron, so Open Workspace still closes.
                    .on_action(cx.listener(|_, _: &Escape, _, cx| {
                        cx.emit(RemoteDirBrowserEvent::Cancelled);
                        cx.propagate();
                    }))
                    .child(up_button)
                    .child(path_input),
            )
            .child(breadcrumb)
            .child(list)
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.))
                    .pt(px(4.))
                    .border_t_1()
                    .border_color(border)
                    .child(
                        ui::dialog_button(
                            "dir-cancel",
                            "Cancel",
                            ui::ButtonKind::Secondary,
                            false,
                            cx,
                        )
                        .font_family(BUTTON_FONT)
                        .on_click(
                            cx.listener(|_, _, _, cx| cx.emit(RemoteDirBrowserEvent::Cancelled)),
                        ),
                    )
                    .child(
                        ui::dialog_button(
                            "dir-select",
                            "Select",
                            ui::ButtonKind::Primary,
                            false,
                            cx,
                        )
                        .font_family(BUTTON_FONT)
                        .on_click(cx.listener(move |_, _, _, cx| {
                            cx.emit(RemoteDirBrowserEvent::Selected(current.clone()));
                        })),
                    ),
            )
    }
}
