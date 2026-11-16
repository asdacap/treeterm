//! "Open Workspace": `OpenWorkspaceDialog.tsx`. A path is typed, picked from the recent
//! directories, or "Browse..." picks one: the native folder picker on this machine,
//! `RemoteDirectoryBrowser.tsx` on a remote host (the native picker only sees this machine).
//! The workspace opens with the "Default Application" chosen below the folder.

use std::rc::Rc;

use gpui_kit::component::WindowExt as _;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::menu::DropdownMenu as _;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::*;
use gpui_kit::{
    App, Context, Div, Entity, FocusHandle, Focusable as _, FontWeight, Hsla, KeyDownEvent, SharedString, Subscription,
    Window, div, px,
};
use treeterm_core::wire::workspace_file::WorktreeSettings;
use treeterm_model::session_store::SessionStore;
use treeterm_model::settings_store::SettingsStore;
use treeterm_model::tool_tabs::workspace_settings::{DefaultAppChoice, default_app_label};

use crate::open_panel::{self, Choose};
use crate::remote_dir_browser::{BrowserFocus, Inline, RemoteDirBrowserEvent};
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
    recent: Vec<RecentDir>,
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

/// A "Recent Directories" row: a `<button>` that takes the focus on a click as from Tab.
struct RecentDir {
    path: String,
    focus: FocusHandle,
}

/// What Enter did in the dialog.
enum Entered {
    /// It opened the folder, or the focused field took it: the browser's path field goes to
    /// the typed path, and the folder field has nothing more to do.
    Handled,
    /// It goes on to the focused button, which presses itself.
    PassedOn,
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
        // Enter goes to the whole dialog: `enter`.
        let subscription = cx.subscribe(&folder, |_, _, event: &InputEvent, cx| match event {
            InputEvent::Change => cx.notify(),
            InputEvent::PressEnter { .. } | InputEvent::Focus | InputEvent::Blur => {}
        });
        let recent = recent.into_iter().map(|path| RecentDir { path, focus: cx.focus_handle() }).collect();
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

    /// The "Browse..." button's text: "Opening..." while the native picker is up, "Hide"
    /// while the remote browser is shown.
    fn browse_label(&self) -> &'static str {
        match &self.picker {
            Picker::Native { picking: true, .. } => "Opening...",
            Picker::Remote { browser: Inline::Shown { .. }, .. } => "Hide",
            Picker::Native { picking: false, .. } | Picker::Remote { browser: Inline::Hidden, .. } => "Browse...",
        }
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

    /// `handleKeyDown`: Enter anywhere in the dialog (the Kit's `Confirm`) opens the folder.
    /// With no folder chosen, or while the picker is up, a focused button presses itself
    /// instead, as Electron's `<button>`s do. In the remote browser, Enter acts on the browser
    /// only; Electron also opened the folder field's path from there, which looks accidental.
    fn enter(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entered {
        match self.browser_focus(window, cx) {
            BrowserFocus::PathField => Entered::Handled,
            BrowserFocus::Inside => Entered::PassedOn,
            BrowserFocus::Outside if self.has_folder(cx) => {
                self.open_selected(window, cx);
                Entered::Handled
            }
            BrowserFocus::Outside if self.folder.read(cx).focus_handle(cx).is_focused(window) => Entered::Handled,
            BrowserFocus::Outside => Entered::PassedOn,
        }
    }

    /// Enter on a focused button of the dialog's own (×, a recent row, Browse..., Cancel or
    /// Open) or on the Default Application select: `ui::bind_keys` unbinds Enter there
    /// (`KeyControl`, `KeySelect`), so the Kit's `Confirm`, and [`Self::enter`], never see
    /// it. Electron's `handleKeyDown` still opens the chosen folder before the button's own
    /// activation, so the dialog takes the key on its way down to the button; with no folder
    /// the button presses itself, and the select, as on macOS, ignores it. Anywhere else
    /// `Confirm` has already run, and either opened or found nothing to open.
    fn enter_on_button(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = &event.keystroke;
        if key.key != "enter" || key.modifiers.modified() {
            return;
        }
        if matches!(self.browser_focus(window, cx), BrowserFocus::Outside) && self.has_folder(cx) {
            cx.stop_propagation();
            self.open_selected(window, cx);
        }
    }

    /// Where the focus is relative to the remote browser, if shown.
    fn browser_focus(&self, window: &Window, cx: &App) -> BrowserFocus {
        match &self.picker {
            Picker::Remote { browser: Inline::Shown { view, .. }, .. } => view.read(cx).keyboard_focus(window, cx),
            Picker::Remote { browser: Inline::Hidden, .. } | Picker::Native { .. } => BrowserFocus::Outside,
        }
    }

    /// Whether Enter opens (`selectedPath && !isSelecting`).
    fn has_folder(&self, cx: &App) -> bool {
        !self.selected(cx).is_empty() && !self.picking()
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
        let picked = open_panel::choose_path(Choose::Directory, window, cx);
        cx.spawn_in(window, async move |dialog, cx| {
            let path = picked.await.map(|path| path.to_string_lossy().into_owned());
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

    /// `.recent-directories-list`: each row a `<button>`, the folder name over its path, with
    /// the full path as its tooltip; a click fills the folder field (`handleSelectRecent`).
    /// `.recent-directory-item:focus`: the focused row has an accent border and no focus ring.
    fn render_recent(&self, window: &mut Window, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let c = colors(cx);
        let (primary, secondary, border, hover, tertiary, accent) =
            (c.text_primary, c.text_secondary, c.border, c.bg_hover, c.bg_tertiary, c.accent);
        let mut list = div().id("recent-directories").flex().flex_col().gap(px(4.)).max_h(px(200.)).overflow_y_scroll();
        for (i, RecentDir { path: dir, focus }) in self.recent.iter().enumerate() {
            let (path, tooltip) = (dir.clone(), SharedString::from(dir.clone()));
            let edge = if focus.is_focused(window) { accent } else { border };
            list = list.child(
                ui::ringless_key_button(SharedString::from(format!("recent-{i}")), focus, 4.)
                    .debug_selector(|| format!("recent-{i}"))
                    .flex_col()
                    .items_stretch()
                    .justify_start()
                    .flex_shrink_0()
                    .px(px(12.))
                    .py(px(8.))
                    .rounded(px(4.))
                    .border_1()
                    .border_color(edge)
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
                    .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
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
    /// It is a `<select>`: Tab reaches it, Space, ArrowUp or ArrowDown open its menu, and its
    /// border is accent while focused.
    fn render_default_app(&self, window: &mut Window, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let secondary = colors(cx).text_secondary;
        let selected = self.settings.default_application_id.clone();
        let current = default_app_label(&self.choices, &selected);
        let (choices, dialog) = (self.choices.clone(), cx.entity().downgrade());
        let focus = ui::keyed_focus("open-default-app", window, cx);
        let boxed = ui::key_select_box("open-default-app", current, &focus, cx).w_full();
        let select = ui::Trigger::new(boxed).dropdown_menu(
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors(cx);
        let (primary, secondary, border, hover, tertiary) =
            (c.text_primary, c.text_secondary, c.border, c.bg_hover, c.bg_tertiary);
        let picking = self.picking();
        let label = self.browse_label();
        // `.open-workspace-dialog .dialog-btn.browse`: bg-tertiary with a border; disabled
        // while the native picker is up.
        let browse = ui::key_button("browse", 4., window, cx)
            .disabled(picking)
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
        let recent = (!self.recent.is_empty()).then(|| self.render_recent(window, cx));
        let default_app = self.render_default_app(window, cx);
        let disabled = self.selected(cx).is_empty() || picking;
        let open = ui::dialog_key_button("open-submit", "Open", ui::ButtonKind::Primary, disabled, window, cx)
            .on_click(cx.listener(|dialog, _, window, cx| dialog.open_selected(window, cx)));
        let cancel = ui::dialog_key_button("open-cancel", "Cancel", ui::ButtonKind::Secondary, false, window, cx)
            .on_click(|_, window, cx| window.close_dialog(cx));
        ui::dialog_root(cx)
            .capture_key_down(cx.listener(Self::enter_on_button))
            .child(ui::dialog_key_header("Open Workspace", "×", primary, window, cx))
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
            .child(ui::dialog_actions(cx).child(cancel).child(open))
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
    show(view, window, cx);
}

/// Shows `view` as the dialog.
fn show(view: Entity<OpenWorkspaceDialog>, window: &mut Window, cx: &mut App) {
    // `autoFocus` on the folder, once the dialog has taken the focus.
    let folder = view.read(cx).folder.clone();
    window.defer(cx, move |window, cx| folder.update(cx, |input, cx| input.focus(window, cx)));
    window.open_dialog(cx, move |dialog, _window, cx| {
        let host = view.clone();
        // Enter in the dialog: a later `on_ok` replaces `dialog_frame`'s. The dialog never
        // closes on it by itself.
        ui::dialog_frame(dialog, 500., cx).child(view.clone()).on_ok(move |_, window, cx| {
            match host.update(cx, |dialog, cx| dialog.enter(window, cx)) {
                Entered::Handled => {}
                Entered::PassedOn => cx.propagate(),
            }
            false
        })
    });
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::path::PathBuf;
    use std::rc::Rc;

    use gpui_kit::base::test_support;
    use gpui_kit::{
        AppContext as _, Entity, KeyDownEvent, KeyUpEvent, Keystroke, Modifiers, Role, TestAppContext, VisualTestContext,
        WindowOptions,
    };
    use treeterm_core::settings::Settings;
    use treeterm_model::settings_store::SettingsStore;
    use treeterm_model::tool_tabs::workspace_settings::DefaultAppChoice;

    use super::{Location, OnOpen, OpenWorkspaceDialog, folder_name, show};

    struct Blank;

    impl gpui_kit::Render for Blank {
        fn render(&mut self, _: &mut gpui_kit::Window, _: &mut gpui_kit::Context<Self>) -> impl gpui_kit::IntoElement {
            gpui_kit::div()
        }
    }

    /// What [`open_dialog`] shows.
    struct Shown<'a> {
        cx: &'a mut VisualTestContext,
        dialog: Entity<OpenWorkspaceDialog>,
        /// The paths it opened.
        opened: Rc<RefCell<Vec<String>>>,
        /// `settings.json`, in memory: where a folder picked by "Browse..." is recorded.
        settings: Entity<SettingsStore>,
    }

    /// A window showing Open Workspace on this machine with `recent` offered.
    fn open_dialog<'a>(recent: &[&str], cx: &'a mut TestAppContext) -> Shown<'a> {
        let window = cx.update(|cx| {
            gpui_kit::init(cx);
            crate::theme::install(cx);
            // The app's keymap: Enter and Space reach a focused button (`KeyControl`).
            crate::ui::bind_keys(cx);
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| cx.new(|_| Blank)).expect("open a window").0
        });
        let cx = VisualTestContext::from_window(window, cx).into_mut();
        let opened = Rc::new(RefCell::new(Vec::new()));
        let sink = opened.clone();
        let on_open: OnOpen = Rc::new(move |path, _, _, _| sink.borrow_mut().push(path));
        let recent = recent.iter().map(|dir| (*dir).to_owned()).collect();
        let choices = vec![DefaultAppChoice { id: String::new(), label: "Use Global Default".to_owned() }];
        let (dialog, settings) = cx.update(|window, cx| {
            let settings = cx.new(|_| SettingsStore::in_memory(Settings::default()));
            let location = Location::Local(settings.clone());
            let dialog = cx.new(|cx| OpenWorkspaceDialog::new(location, recent, choices, on_open, window, cx));
            show(dialog.clone(), window, cx);
            (dialog, settings)
        });
        draw(cx);
        Shown { cx, dialog, opened, settings }
    }

    /// Which recent rows have the focus, in order.
    fn focused_rows(dialog: &Entity<OpenWorkspaceDialog>, cx: &mut VisualTestContext) -> Vec<bool> {
        cx.update(|window, cx| dialog.read(cx).recent.iter().map(|row| row.focus.is_focused(window)).collect())
    }

    fn draw(cx: &mut VisualTestContext) {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
    }

    /// Press and release `key`, then show the outcome.
    fn press(key: &str, cx: &mut VisualTestContext) {
        let keystroke = cx.update(|_, cx| Keystroke::parse_with_os(key, cx.platform_info().os).unwrap());
        cx.simulate_event(KeyDownEvent { keystroke: keystroke.clone(), is_held: false, prefer_character_input: false });
        cx.simulate_event(KeyUpEvent { keystroke });
        draw(cx);
    }

    fn is_open(cx: &mut VisualTestContext) -> bool {
        cx.update(gpui_kit::component::WindowExt::has_active_dialog)
    }

    /// Tab from the folder field to "Browse..." and press it with Space (Enter would open the
    /// typed folder: `handleKeyDown`).
    fn press_browse(cx: &mut VisualTestContext) {
        press("tab", cx);
        press("space", cx);
    }

    /// Answer the open panel "Browse..." showed with `folder`, or cancel it. It is
    /// `dialog:selectFolder`'s: one folder, with the default "Open" button (Electron sets no
    /// `buttonLabel`). A test window has no AppKit window to attach a sheet to, so the panel
    /// is GPUI's, which the test platform answers.
    fn answer_panel(folder: Option<&str>, cx: &mut VisualTestContext) {
        assert!(cx.did_prompt_for_paths(), "Browse... shows the open panel");
        cx.simulate_path_prompt_response(|options| {
            assert!(options.directories, "folders can be chosen");
            assert!(!options.files, "files cannot be chosen");
            assert!(!options.multiple, "one folder");
            assert_eq!(options.prompt, None, "the default \"Open\" button");
            folder.map(|folder| vec![PathBuf::from(folder)])
        });
        draw(cx);
    }

    fn browse_label(dialog: &Entity<OpenWorkspaceDialog>, cx: &mut VisualTestContext) -> &'static str {
        cx.update(|_, cx| dialog.read(cx).browse_label())
    }

    fn selected(dialog: &Entity<OpenWorkspaceDialog>, cx: &mut VisualTestContext) -> String {
        cx.update(|_, cx| dialog.read(cx).selected(cx))
    }

    fn recent_directories(settings: &Entity<SettingsStore>, cx: &mut VisualTestContext) -> Vec<String> {
        cx.update(|_, cx| settings.read(cx).settings().recent_directories.clone())
    }

    #[gpui_kit::test]
    fn enter_opens_the_typed_folder_once(cx: &mut TestAppContext) {
        let Shown { cx, opened, .. } = open_dialog(&[], cx);
        cx.simulate_input("/srv/app");
        draw(cx);
        press("enter", cx);
        assert_eq!(*opened.borrow(), vec!["/srv/app".to_owned()]);
        assert!(!is_open(cx));
    }

    /// Recent rows are buttons: Shift-Tab from the folder field focuses the last one, and
    /// Enter or Space presses the focused one while no folder is chosen; once there is one,
    /// Enter opens it (`handleKeyDown`).
    #[gpui_kit::test]
    fn enter_presses_a_focused_button_until_a_folder_is_chosen(cx: &mut TestAppContext) {
        for key in ["enter", "space"] {
            let Shown { cx, dialog, opened, .. } = open_dialog(&["/home/me/src", "/home/me/web"], cx);
            assert_eq!(focused_rows(&dialog, cx), vec![false, false], "{key}");
            // Back from the folder field to the last recent row.
            press("shift-tab", cx);
            assert_eq!(focused_rows(&dialog, cx), vec![false, true], "{key}");
            press(key, cx);
            assert!(opened.borrow().is_empty(), "{key}");
            assert!(is_open(cx), "{key}");
            press("enter", cx);
            assert_eq!(*opened.borrow(), vec!["/home/me/web".to_owned()], "{key}");
            assert!(!is_open(cx), "{key}");
        }
    }

    /// A click on a recent row fills the folder field and focuses the row, as Chromium focuses
    /// a clicked `<button>` (its accent border is `.recent-directory-item:focus`); Enter then
    /// opens the folder.
    #[gpui_kit::test]
    fn a_clicked_recent_row_takes_the_focus(cx: &mut TestAppContext) {
        let Shown { cx, dialog, opened, .. } = open_dialog(&["/home/me/src", "/home/me/web"], cx);
        let row = cx.debug_bounds("recent-0").expect("the first recent row is drawn");
        cx.simulate_click(row.center(), Modifiers::none());
        draw(cx);
        assert_eq!(focused_rows(&dialog, cx), vec![true, false]);
        assert_eq!(cx.update(|_, cx| dialog.read(cx).selected(cx)), "/home/me/src");
        press("enter", cx);
        assert_eq!(*opened.borrow(), vec!["/home/me/src".to_owned()]);
    }

    /// Space, or Enter while there is no folder, on a focused Cancel closes without opening.
    #[gpui_kit::test]
    fn a_focused_cancel_closes_without_opening(cx: &mut TestAppContext) {
        for key in ["space", "enter"] {
            let Shown { cx, opened, .. } = open_dialog(&[], cx);
            // The folder field, Browse..., the Default Application select, Cancel.
            press("tab", cx);
            press("tab", cx);
            press("tab", cx);
            press(key, cx);
            assert!(opened.borrow().is_empty(), "{key}");
            assert!(!is_open(cx), "{key}");
        }
    }

    /// The control holding the focus after a frame: a button by its element id, the folder
    /// field as "text field", and "" when none of those has it (the select is not observed).
    fn focused_control(cx: &mut VisualTestContext) -> String {
        cx.update(|window, cx| {
            window.draw(cx).clear(cx);
            let mut focused = test_support::snapshots(window).into_iter().filter_map(|control| {
                let name = match control.role()? {
                    Role::Button => control.path().last()?.to_string(),
                    Role::TextInput => "text field".to_owned(),
                    _ => return None,
                };
                (control.focused() == Some(true)).then_some(name)
            });
            let control = focused.next().unwrap_or_default();
            assert_eq!(focused.next(), None, "one control has the focus");
            control
        })
    }

    /// Tab moves through the controls in document order, as Electron's native controls do,
    /// and round again: the folder field (focused first), Browse..., the Default Application
    /// select, Cancel, Open (once a folder is chosen), the header's ×, the recent rows.
    #[gpui_kit::test]
    fn tab_moves_through_the_controls_in_order(cx: &mut TestAppContext) {
        let Shown { cx, .. } = open_dialog(&["/home/me/src", "/home/me/web"], cx);
        assert_eq!(focused_control(cx), "text field");
        cx.simulate_input("/srv/app");
        draw(cx);
        let expected = [
            "browse",
            "", // the Default Application select
            "open-cancel",
            "open-submit",
            "dialog-close",
            "recent-0",
            "recent-1",
            "text field",
        ];
        let mut reached = Vec::new();
        for _ in expected {
            press("tab", cx);
            reached.push(focused_control(cx));
        }
        assert_eq!(reached, expected);
        assert!(is_open(cx));
    }

    /// Whether a menu (the select's) is open.
    fn menu_open(cx: &mut VisualTestContext) -> bool {
        cx.update(|_, cx| gpui_kit::base::GlobalState::is_in_deferred_context(cx))
    }

    /// The focused select opens its menu on Space, ArrowDown or ArrowUp, as on macOS. Enter
    /// on it opens the chosen folder (`handleKeyDown`), and with no folder does nothing.
    #[gpui_kit::test]
    fn the_focused_select_opens_its_menu_or_the_folder(cx: &mut TestAppContext) {
        let Shown { cx, opened, .. } = open_dialog(&[], cx);
        press("tab", cx);
        press("tab", cx);
        assert_eq!(focused_control(cx), "", "the select");
        press("enter", cx);
        assert!(!menu_open(cx));
        assert!(is_open(cx));
        for key in ["space", "up", "down"] {
            press(key, cx);
            assert!(menu_open(cx), "{key} opens the menu");
            press("escape", cx);
            assert!(!menu_open(cx), "Escape closes it");
            assert!(is_open(cx), "and not the dialog");
        }
        assert!(opened.borrow().is_empty());

        // A folder chosen: Enter on the select opens it.
        press("shift-tab", cx);
        press("shift-tab", cx);
        assert_eq!(focused_control(cx), "text field");
        cx.simulate_input("/srv/app");
        draw(cx);
        press("tab", cx);
        press("tab", cx);
        assert_eq!(focused_control(cx), "", "the select");
        press("enter", cx);
        assert_eq!(*opened.borrow(), vec!["/srv/app".to_owned()]);
        assert!(!is_open(cx));
    }

    /// The header's × is a `<button>`: Shift+Tab from the folder field reaches it, Space (or
    /// Enter with no folder) closes the dialog, and Enter with a folder chosen opens it
    /// (`handleKeyDown`).
    #[gpui_kit::test]
    fn the_focused_close_button_closes_or_opens_the_folder(cx: &mut TestAppContext) {
        for (key, folder) in [("space", ""), ("enter", ""), ("space", "/srv/app")] {
            let Shown { cx, opened, .. } = open_dialog(&[], cx);
            cx.simulate_input(folder);
            draw(cx);
            press("shift-tab", cx);
            assert_eq!(focused_control(cx), "dialog-close");
            press(key, cx);
            assert!(opened.borrow().is_empty(), "{key} {folder}");
            assert!(!is_open(cx), "{key} {folder}");
        }
        let Shown { cx, opened, .. } = open_dialog(&[], cx);
        cx.simulate_input("/srv/app");
        draw(cx);
        press("shift-tab", cx);
        press("enter", cx);
        assert_eq!(*opened.borrow(), vec!["/srv/app".to_owned()]);
        assert!(!is_open(cx));
    }

    /// "Browse..." shows the open panel and reads "Opening..." until it closes, while Enter
    /// opens nothing (`isSelecting`). The chosen folder replaces the typed path and goes to
    /// the front of `settings.json`'s recent directories (`dialog:selectFolder`).
    #[gpui_kit::test]
    fn a_browsed_folder_replaces_the_typed_path_and_is_recent(cx: &mut TestAppContext) {
        let Shown { cx, dialog, opened, settings } = open_dialog(&[], cx);
        cx.simulate_input("/srv/typed");
        draw(cx);
        press_browse(cx);
        assert_eq!(browse_label(&dialog, cx), "Opening...");
        press("enter", cx);
        assert!(opened.borrow().is_empty());
        assert!(is_open(cx));

        answer_panel(Some("/home/me/chosen"), cx);
        assert_eq!(selected(&dialog, cx), "/home/me/chosen");
        assert_eq!(recent_directories(&settings, cx), vec!["/home/me/chosen".to_owned()]);
        assert_eq!(browse_label(&dialog, cx), "Browse...");
        assert!(opened.borrow().is_empty());
        assert!(is_open(cx));
    }

    /// A cancelled open panel keeps the typed path and records no recent directory.
    #[gpui_kit::test]
    fn a_cancelled_browse_keeps_the_typed_path(cx: &mut TestAppContext) {
        let Shown { cx, dialog, settings, .. } = open_dialog(&[], cx);
        cx.simulate_input("/srv/typed");
        draw(cx);
        press_browse(cx);
        assert_eq!(browse_label(&dialog, cx), "Opening...");

        answer_panel(None, cx);
        assert_eq!(selected(&dialog, cx), "/srv/typed");
        assert_eq!(recent_directories(&settings, cx), Vec::<String>::new());
        assert_eq!(browse_label(&dialog, cx), "Browse...");
    }

    #[test]
    fn folder_names_are_the_last_segment() {
        assert_eq!(folder_name("/home/me/src"), "src");
        assert_eq!(folder_name("C:\\work\\repo"), "repo");
        assert_eq!(folder_name("/home/me/"), "/home/me/");
        assert_eq!(folder_name("repo"), "repo");
    }
}
