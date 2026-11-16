//! "Open Workspace": `OpenWorkspaceDialog.tsx`. A path is typed, picked from the recent
//! directories, or "Browse..." picks one: the native folder picker on this machine,
//! `RemoteDirectoryBrowser.tsx` on a remote host (the native picker only sees this machine).
//! The workspace opens with the "Default Application" chosen below the folder.

use std::rc::Rc;

use gpui_kit::component::WindowExt as _;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::menu::DropdownMenu as _;
use gpui_kit::prelude::*;
use gpui_kit::{App, Context, Div, Entity, FontWeight, Hsla, SharedString, Subscription, Window, div, px};
use treeterm_core::wire::workspace_file::WorktreeSettings;
use treeterm_model::session_store::SessionStore;
use treeterm_model::settings_store::SettingsStore;
use treeterm_model::tool_tabs::workspace_settings::{DefaultAppChoice, default_app_label};

use crate::remote_dir_browser::{Inline, RemoteDirBrowserEvent};
use crate::theme::colors;
use crate::ui;

/// Called with the chosen directory and the new workspace's settings (`onOpen`).
pub type OnOpen = Rc<dyn Fn(String, WorktreeSettings, &mut Window, &mut App)>;

/// Where the directory lives.
pub enum Location {
    /// This machine: a folder the native picker returns is added to `settings.json`'s
    /// recent directories, as Electron's `dialog:selectFolder` does.
    Local(Entity<SettingsStore>),
    Remote(Entity<SessionStore>),
}

/// What "Browse..." does.
enum Picker {
    /// The native folder picker; `picking` while it is up ("Opening...").
    Native { settings: Entity<SettingsStore>, picking: bool },
    /// The remote filesystem, browsed under the folder field while `browser` is shown.
    Remote { session: Entity<SessionStore>, browser: Inline },
}

pub struct OpenWorkspaceDialog {
    folder: Entity<InputState>,
    /// "Recent Directories", most recent first; the section is hidden when empty.
    recent: Vec<String>,
    picker: Picker,
    /// "Default Application": "Use Global Default", then the new-tab menu's applications
    /// (`availableApps`).
    choices: Vec<DefaultAppChoice>,
    /// The workspace's own settings: the chosen application (`selectedAppId`), empty for
    /// the global default.
    settings: WorktreeSettings,
    on_open: OnOpen,
    _subscriptions: Vec<Subscription>,
}

/// `.open-workspace-field label`: 12px, medium, secondary.
fn field_label(text: &'static str, color: Hsla) -> Div {
    div().text_size(px(12.)).font_weight(FontWeight::MEDIUM).text_color(color).child(text)
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
        choices: Vec<DefaultAppChoice>,
        on_open: OnOpen,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (picker, placeholder) = match location {
            Location::Local(settings) => {
                (Picker::Native { settings, picking: false }, "Enter or browse for a folder...")
            }
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
        Self {
            folder,
            recent,
            picker,
            choices,
            settings: WorktreeSettings::default(),
            on_open,
            _subscriptions: vec![subscription],
        }
    }

    fn selected(&self, cx: &App) -> String {
        self.folder.read(cx).value().to_string()
    }

    /// `isSelecting`: Open waits for the native picker.
    fn picking(&self) -> bool {
        matches!(self.picker, Picker::Native { picking: true, .. })
    }

    /// `handleOpen`: the workspace opens with the chosen default application as its own.
    fn open_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let path = self.selected(cx);
        if path.is_empty() || self.picking() {
            return;
        }
        window.close_dialog(cx);
        (self.on_open)(path, self.settings.clone(), window, cx);
    }

    fn browse(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match &mut self.picker {
            Picker::Native { picking: true, .. } => {}
            Picker::Native { settings, picking } => {
                *picking = true;
                let settings = settings.clone();
                self.pick_native(settings, window, cx);
            }
            Picker::Remote { browser: Inline::Shown { .. }, .. } => self.hide_browser(cx), // "Hide"
            Picker::Remote { session, browser: Inline::Hidden } => {
                let session = session.clone();
                self.show_browser(session, window, cx);
            }
        }
    }

    /// `handleSelectFolder`: the chosen folder replaces the typed path. As in
    /// `dialog:selectFolder`, it also goes to the front of `settings.json`'s recent
    /// directories, even when the dialog was closed meanwhile; a cancelled or failed pick
    /// records nothing.
    fn pick_native(&mut self, settings: Entity<SettingsStore>, window: &mut Window, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Open Workspace".into()),
        });
        cx.spawn_in(window, async move |dialog, cx| {
            let path = match picked.await {
                Ok(Ok(Some(mut paths))) if !paths.is_empty() => Some(paths.remove(0).to_string_lossy().into_owned()),
                Ok(Ok(_)) | Err(_) => None, // cancelled
                Ok(Err(error)) => {
                    tracing::error!(%error, "folder picker failed");
                    None
                }
            };
            if let Some(path) = &path {
                settings.update(cx, |s, cx| s.add_recent_directory(path, cx));
            }
            match dialog.update_in(cx, |dialog, window, cx| {
                dialog.picker = Picker::Native { settings, picking: false };
                if let Some(path) = path {
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
            .child(field_label("Recent Directories", secondary))
            .child(list)
            .into_any_element()
    }

    /// "Default Application": a full-width `.settings-select` whose menu has a separator
    /// after "Use Global Default" (Electron's disabled "──────────" option), then the hint.
    fn render_default_app(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let secondary = colors(cx).text_secondary;
        let selected = self.settings.default_application_id.clone();
        let current = default_app_label(&self.choices, &selected);
        let (choices, dialog) = (self.choices.clone(), cx.entity().downgrade());
        let select = ui::Trigger::new(ui::select_box("open-default-app", current, cx).w_full()).dropdown_menu(
            move |mut menu, _, _| {
                for (index, choice) in choices.iter().enumerate() {
                    if index == 1 {
                        menu = menu.separator();
                    }
                    let (dialog, id) = (dialog.clone(), choice.id.clone());
                    menu = menu.item(ui::menu_item(choice.label.clone()).checked(choice.id == selected).on_click(
                        move |_, _, cx| {
                            if let Err(error) = dialog.update(cx, |dialog, cx| {
                                dialog.settings.default_application_id = id.clone();
                                cx.notify();
                            }) {
                                tracing::debug!(%error, "open workspace dialog closed");
                            }
                        },
                    ));
                }
                menu.min_w(px(200.))
            },
        );
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(field_label("Default Application", secondary))
            .child(select)
            .child(
                // `.settings-hint`: 11px, 2px below the select.
                div().mt(px(2.)).text_size(px(ui::SMALL)).line_height(px(13.)).text_color(secondary).child(
                    "The application to open by default in this workspace. Child worktrees will inherit this setting.",
                ),
            )
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
            Picker::Native { picking: true, .. } => "Opening...",
            Picker::Remote { browser: Inline::Shown { .. }, .. } => "Hide",
            Picker::Native { picking: false, .. } | Picker::Remote { browser: Inline::Hidden, .. } => "Browse...",
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
        let default_app = self.render_default_app(cx);
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
                            .child(field_label("Folder", secondary))
                            .child(
                                div()
                                    .flex()
                                    .gap(px(8.))
                                    .child(div().flex_1().min_w_0().child(ui::text_input(&self.folder, cx)))
                                    .child(browse),
                            ),
                    )
                    .children(browser)
                    .child(default_app),
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
/// `choices`: the "Default Application" choices, "Use Global Default" first
/// (`default_app_choices`).
pub fn open(
    location: Location,
    recent: Vec<String>,
    choices: Vec<DefaultAppChoice>,
    on_open: OnOpen,
    window: &mut Window,
    cx: &mut App,
) {
    let view = cx.new(|cx| OpenWorkspaceDialog::new(location, recent, choices, on_open, window, cx));
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
