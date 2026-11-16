//! Application menu. Port of `src/main/menu.ts`. Workspace shortcuts (prefix mode,
//! Cmd/Ctrl+1–9) go through the keybinding state machine; see `AppView::perform`.
//!
//! Every command runs from a global listener. GPUI Kit draws dialogs beside the window's
//! view and gives them focus, so a handler on the view is off the dispatch path (and its
//! menu item greyed out) while a dialog is open; Electron sends every menu click to the
//! focused window, whatever it shows.

use std::rc::Rc;

use gpui_kit::component::input::{self, AnyInputState};
use gpui_kit::component::menu::AppMenuBar;
use gpui_kit::component::{GlobalState, WindowExt as _};
use gpui_kit::prelude::*;
use gpui_kit::{
    Action, AnyElement, AnyWindowHandle, App, Context, Entity, KeyBinding, Menu, MenuItem,
    OsAction, SystemMenuType, WeakEntity, Window, actions, deferred, div, px,
};
use treeterm_model::app_store::AppStore;
use treeterm_model::zoom::ZoomStore;
use treeterm_term::keys::KeyOptions;

use crate::active_processes_view::CloseAnyway;
use crate::app_view::AppView;
use crate::terminal_view;
use crate::theme::colors;

actions!(
    treeterm,
    [
        Quit,
        QuitAndKillDaemon,
        Minimize,
        OpenSettings,
        ActiveProcesses,
        About,
        Hide,
        HideOthers,
        ShowAll,
        CloseWindow,
        ToggleFullScreen,
        ResetInput,
        ResetZoom,
        ZoomIn,
        ZoomOut,
        ZoomWindow,
        BringAllToFront,
        LearnMore,
        PasteAndMatchStyle,
        DeleteSelection
    ]
);

/// The menu shortcuts off macOS (Electron's Linux accelerators), in `key_bindings` order.
/// GPUI runs bindings before a focused terminal sees the key, so the terminal takes back
/// those it encodes (`terminal_view::keep_terminal_keys`), as xterm.js consumed them in
/// Electron.
pub const ACCELERATORS: &[&str] = &[
    "ctrl-,",
    "ctrl-q",
    "ctrl-shift-q",
    "ctrl-m",
    "ctrl-w",
    "ctrl-0",
    "ctrl-=",
    "ctrl-+",
    "ctrl--",
    "f11",
    "ctrl-shift-z",
];

/// GPUI Kit's text inputs' key context.
const INPUT_CONTEXT: &str = "Input";

/// The menu shortcuts (Electron's role accelerators). Zoom In is Electron's Cmd/Ctrl+Plus
/// (Shift+=), and also Cmd/Ctrl+= as in Chromium. Its menu item shows Plus: the macOS menu
/// bar shows an action's first binding, GPUI Kit's menu bar (off macOS) its last.
///
/// Edit's Undo, Redo, Cut, Copy, Paste and Select All keep GPUI Kit's keys, which are
/// Electron's, in the inputs and editors that handle them (a terminal binds its own, see
/// `terminal_view::edit_keys`). Redo off macOS is Electron's Linux Ctrl+Shift+Z, besides the
/// Kit's Ctrl+Y (Windows'). Edit > Delete has no shortcut, as in Electron.
fn key_bindings(mac: bool) -> Vec<KeyBinding> {
    if mac {
        vec![
            KeyBinding::new("cmd-,", OpenSettings, None),
            KeyBinding::new("cmd-h", Hide, None),
            KeyBinding::new("cmd-alt-h", HideOthers, None),
            KeyBinding::new("cmd-q", Quit, None),
            KeyBinding::new("cmd-w", CloseWindow, None),
            KeyBinding::new("cmd-shift-q", QuitAndKillDaemon, None),
            KeyBinding::new("cmd-0", ResetZoom, None),
            KeyBinding::new("cmd-+", ZoomIn, None),
            KeyBinding::new("cmd-=", ZoomIn, None),
            KeyBinding::new("cmd--", ZoomOut, None),
            KeyBinding::new("ctrl-cmd-f", ToggleFullScreen, None),
            KeyBinding::new("cmd-m", Minimize, None),
            KeyBinding::new("cmd-alt-shift-v", PasteAndMatchStyle, None),
        ]
    } else {
        vec![
            KeyBinding::new("ctrl-,", OpenSettings, None),
            KeyBinding::new("ctrl-q", Quit, None),
            KeyBinding::new("ctrl-shift-q", QuitAndKillDaemon, None),
            KeyBinding::new("ctrl-m", Minimize, None),
            KeyBinding::new("ctrl-w", CloseWindow, None),
            KeyBinding::new("ctrl-0", ResetZoom, None),
            KeyBinding::new("ctrl-=", ZoomIn, None),
            KeyBinding::new("ctrl-+", ZoomIn, None),
            KeyBinding::new("ctrl--", ZoomOut, None),
            KeyBinding::new("f11", ToggleFullScreen, None),
            KeyBinding::new("ctrl-shift-z", input::Redo, Some(INPUT_CONTEXT)),
        ]
    }
}

/// The menu shortcuts, then (off macOS) the ones a focused terminal keeps for its PTY,
/// encoded with `terminal`. The order matters: see `terminal_view::keep_terminal_keys`.
fn bind_keys(mac: bool, terminal: KeyOptions, cx: &mut App) {
    cx.bind_keys(key_bindings(mac));
    if !mac {
        terminal_view::keep_terminal_keys(ACCELERATORS, terminal, cx);
    }
}

/// Edit: Electron's edit roles, which act on whatever has focus. Its items run GPUI Kit's edit
/// actions, which its inputs and editors, the diff, selectable text (Copy) and a terminal
/// (Copy, Paste, Select All) handle; on macOS GPUI greys out an item nothing focused handles,
/// and AppKit sends Cut, Copy, Paste and Select All to a focused web view itself.
fn edit_menu(mac: bool) -> Menu {
    let mut items = vec![
        MenuItem::os_action("Undo", input::Undo, OsAction::Undo),
        MenuItem::os_action("Redo", input::Redo, OsAction::Redo),
        MenuItem::separator(),
        MenuItem::os_action("Cut", input::Cut, OsAction::Cut),
        MenuItem::os_action("Copy", input::Copy, OsAction::Copy),
        MenuItem::os_action("Paste", input::Paste, OsAction::Paste),
    ];
    if mac {
        items.push(MenuItem::action("Paste and Match Style", PasteAndMatchStyle));
        items.push(MenuItem::action("Delete", DeleteSelection));
    } else {
        items.push(MenuItem::action("Delete", DeleteSelection));
        items.push(MenuItem::separator());
    }
    items.push(MenuItem::os_action("Select All", input::SelectAll, OsAction::SelectAll));
    Menu::new("Edit").items(items)
}

/// The menu bar: Electron's template, with its role labels. The product is "TreeTerm"
/// where Electron shows `app.name`.
fn menus(mac: bool) -> Vec<Menu> {
    let edit = edit_menu(mac);
    let view = Menu::new("View").items([
        MenuItem::action("Actual Size", ResetZoom),
        MenuItem::action("Zoom In", ZoomIn),
        MenuItem::action("Zoom Out", ZoomOut),
        MenuItem::separator(),
        MenuItem::action("Toggle Full Screen", ToggleFullScreen),
        MenuItem::separator(),
        MenuItem::action("Reset Input", ResetInput),
    ]);
    let help = Menu::new("Help").items([MenuItem::action("Learn More", LearnMore)]);
    let quit_and_kill = MenuItem::action("Exit and Kill Daemon", QuitAndKillDaemon);
    if mac {
        vec![
            Menu::new("TreeTerm").items([
                MenuItem::action("About TreeTerm", About),
                MenuItem::separator(),
                MenuItem::action("Preferences...", OpenSettings),
                MenuItem::separator(),
                MenuItem::os_submenu("Services", SystemMenuType::Services),
                MenuItem::separator(),
                MenuItem::action("Hide TreeTerm", Hide),
                MenuItem::action("Hide Others", HideOthers),
                MenuItem::action("Show All", ShowAll),
                MenuItem::separator(),
                MenuItem::action("Quit TreeTerm", Quit),
            ]),
            Menu::new("File").items([
                MenuItem::action("Close Window", CloseWindow),
                MenuItem::separator(),
                quit_and_kill,
            ]),
            edit,
            view,
            // AppKit lists the open windows below (GPUI makes the menu named "Window" the
            // windows menu), as Electron's `window` role.
            Menu::new("Window").items([
                MenuItem::action("Minimize", Minimize),
                MenuItem::action("Zoom", ZoomWindow),
                MenuItem::separator(),
                MenuItem::action("Active Processes", ActiveProcesses),
                MenuItem::separator(),
                MenuItem::action("Bring All to Front", BringAllToFront),
            ]),
            help,
        ]
    } else {
        vec![
            Menu::new("File").items([
                MenuItem::action("Settings", OpenSettings),
                MenuItem::separator(),
                MenuItem::action("Quit", Quit),
                MenuItem::separator(),
                quit_and_kill,
            ]),
            edit,
            view,
            Menu::new("Window").items([
                MenuItem::action("Minimize", Minimize),
                MenuItem::separator(),
                MenuItem::action("Active Processes", ActiveProcesses),
                MenuItem::action("Close", CloseWindow),
            ]),
            help,
        ]
    }
}

/// The shortcuts, the menu bar and the commands that need no window, with `zoom` as the app's
/// zoom. Runs before the window opens: off macOS the window draws the menu bar from what this
/// sets.
pub fn install(zoom: ZoomStore, cx: &mut App) {
    let mac = cfg!(target_os = "macos");
    bind_keys(mac, KeyOptions::for_host(), cx);
    install_zoom(zoom, cx);
    cx.on_action(|_: &Hide, cx| cx.hide());
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
    cx.on_action(|_: &LearnMore, cx| cx.open_url("https://github.com"));
    #[cfg(target_os = "macos")]
    {
        cx.on_action(|_: &About, _| crate::macos::about_panel());
        cx.on_action(|_: &BringAllToFront, _| crate::macos::bring_all_to_front());
    }

    set_menus(mac, cx);
    #[cfg(target_os = "macos")]
    crate::macos::register_help_menu();
}

/// View > Actual Size, Zoom In and Zoom Out: one level for every window, which terminals
/// draw their text at, saved at every change (see `treeterm_model::zoom`).
fn install_zoom(zoom: ZoomStore, cx: &mut App) {
    cx.set_global(zoom);
    cx.on_action(|_: &ResetZoom, cx| cx.update_global::<ZoomStore, _>(|zoom, _| zoom.reset()));
    cx.on_action(|_: &ZoomIn, cx| cx.update_global::<ZoomStore, _>(|zoom, _| zoom.zoom_in()));
    cx.on_action(|_: &ZoomOut, cx| cx.update_global::<ZoomStore, _>(|zoom, _| zoom.zoom_out()));
}

/// Edit > Paste and Match Style and Delete, which GPUI Kit has no action for: they send the
/// Kit's Paste, or Backspace, to whatever has focus in `window`. All the app's text is plain,
/// so matching the style pastes as Paste does (into a terminal too). Delete removes the
/// selected text, as Chromium's does: with nothing selected it leaves the text as it is.
/// Global listeners keep both enabled, as Electron's are.
fn route_edit(window: AnyWindowHandle, cx: &mut App) {
    cx.on_action(move |_: &PasteAndMatchStyle, cx| {
        in_window(window, cx, |window, cx| window.dispatch_action(Box::new(input::Paste), cx));
    });
    cx.on_action(move |_: &DeleteSelection, cx| {
        in_window(window, cx, |window, cx| {
            if has_selected_text(window, cx) {
                window.dispatch_action(Box::new(input::Backspace), cx);
            }
        });
    });
}

/// Run `act` on `window` once GPUI, which holds the window while it dispatches an action to
/// it, lets go. Once the window has closed nothing has focus to act on.
fn in_window(window: AnyWindowHandle, cx: &mut App, act: impl FnOnce(&mut Window, &mut App) + 'static) {
    cx.defer(move |cx| match window.update(cx, |_, window, cx| act(window, cx)) {
        Ok(()) | Err(_) => {}
    });
}

/// Whether the focused input, text area or editor has text selected.
fn has_selected_text(window: &mut Window, cx: &mut App) -> bool {
    match window.focused_input(cx) {
        Some(AnyInputState::Input(state)) => !state.read(cx).selected_range().is_empty(),
        Some(AnyInputState::Textarea(state)) => !state.read(cx).selected_range().is_empty(),
        Some(AnyInputState::Editor(state)) => !state.read(cx).selected_range().is_empty(),
        Some(AnyInputState::Otp(_)) | None => false,
    }
}

/// The menus, for the system menu bar and for GPUI Kit's `AppMenuBar`: off macOS GPUI only
/// stores them, and the window draws them (`in_window_bar`).
fn set_menus(mac: bool, cx: &mut App) {
    cx.set_menus(menus(mac));
    GlobalState::global_mut(cx).set_app_menus(menus(mac).into_iter().map(Menu::owned).collect());
}

/// The menu bar drawn in the window off macOS, as a row above its content (Electron's Linux
/// window shows its menu bar). GTK draws that bar outside the page, so it stays usable while
/// a dialog is open: this row and its menus paint above GPUI Kit's dialogs, as the Kit's
/// own popups do, and take the pointer from them.
pub fn in_window_bar(bar: &Entity<AppMenuBar>, cx: &App) -> AnyElement {
    let c = colors(cx);
    deferred(
        div()
            .flex_shrink_0()
            .h(px(28.))
            .px_1()
            .bg(c.bg_secondary)
            .border_b_1()
            .border_color(c.border)
            .occlude()
            .child(bar.clone()),
    )
    .with_priority(gpui_kit::base::POPUP_PRIORITY)
    .into_any_element()
}

/// The commands that act on the window, sent to its view (Electron's
/// `BrowserWindow.getFocusedWindow()`). Once the window has closed only Quit, and Exit and
/// Kill Daemon on `store`'s local daemon, do anything (with no window, nothing asks first).
pub fn route_to_window(window: AnyWindowHandle, view: WeakEntity<AppView>, store: Entity<AppStore>, cx: &mut App) {
    route::<Quit, _>(window, view.clone(), AppView::quit, |cx| cx.quit(), cx);
    let kill = CloseAnyway::KillDaemonAndQuit(store);
    route::<QuitAndKillDaemon, _>(window, view.clone(), AppView::quit_and_kill_daemon, move |cx| kill.run(cx), cx);
    route::<CloseWindow, _>(window, view.clone(), AppView::close_window, |_| {}, cx);
    route::<OpenSettings, _>(window, view.clone(), AppView::open_settings, |_| {}, cx);
    route::<ActiveProcesses, _>(window, view.clone(), AppView::open_active_processes, |_| {}, cx);
    route::<ResetInput, _>(window, view.clone(), AppView::reset_input, |_| {}, cx);
    route::<Minimize, _>(window, view.clone(), |_, window, _| window.minimize_window(), |_| {}, cx);
    route::<ZoomWindow, _>(window, view.clone(), |_, window, _| window.zoom_window(), |_| {}, cx);
    route::<ToggleFullScreen, _>(window, view, |_, window, _| window.toggle_fullscreen(), |_| {}, cx);
    route_edit(window, cx);
}

/// Run `A` on `view` in `window`, or `no_window` once the window is gone. A global listener
/// runs whatever has focus, and its presence keeps the menu item enabled.
fn route<A: Action, V: 'static>(
    window: AnyWindowHandle,
    view: WeakEntity<V>,
    run: impl Fn(&mut V, &mut Window, &mut Context<V>) + 'static,
    no_window: impl Fn(&mut App) + 'static,
    cx: &mut App,
) {
    let (run, no_window) = (Rc::new(run), Rc::new(no_window));
    cx.on_action(move |_: &A, cx| {
        let (view, run, no_window) = (view.clone(), run.clone(), no_window.clone());
        // GPUI holds the window while it dispatches an action to it: act once it lets go.
        cx.defer(move |cx| {
            let ran = window.update(cx, |_, window, cx| view.update(cx, |view, cx| run(view, window, cx)));
            match ran {
                Ok(Ok(())) => {}
                Ok(Err(_)) | Err(_) => no_window(cx),
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use gpui_kit::{
        IntoElement, OperatingSystem, OwnedMenu, OwnedMenuItem, Render,
        TestAppContext, VisualTestContext, WindowOptions, div,
    };
    use treeterm_model::zoom::Zoom;

    use super::*;

    /// A menu as `Label = action` lines, `-` for a separator.
    fn describe(menu: &OwnedMenu) -> (String, Vec<String>) {
        let items = menu
            .items
            .iter()
            .map(|item| match item {
                OwnedMenuItem::Separator => "-".to_owned(),
                OwnedMenuItem::Action { name, action, .. } => format!("{name} = {}", action.name()),
                OwnedMenuItem::SystemMenu(menu) => format!("{} (system)", menu.name),
                OwnedMenuItem::Submenu(menu) => format!("{} (submenu)", menu.name),
            })
            .collect();
        (menu.name.to_string(), items)
    }

    fn described(mac: bool) -> Vec<(String, Vec<String>)> {
        menus(mac).into_iter().map(Menu::owned).map(|menu| describe(&menu)).collect()
    }

    fn menu(name: &str, items: &[&str]) -> (String, Vec<String>) {
        (name.to_owned(), items.iter().map(|item| (*item).to_owned()).collect())
    }

    const MAC_EDIT: [&str; 9] = [
        "Undo = input::Undo",
        "Redo = input::Redo",
        "-",
        "Cut = input::Cut",
        "Copy = input::Copy",
        "Paste = input::Paste",
        "Paste and Match Style = treeterm::PasteAndMatchStyle",
        "Delete = treeterm::DeleteSelection",
        "Select All = input::SelectAll",
    ];
    const LINUX_EDIT: [&str; 9] = [
        "Undo = input::Undo",
        "Redo = input::Redo",
        "-",
        "Cut = input::Cut",
        "Copy = input::Copy",
        "Paste = input::Paste",
        "Delete = treeterm::DeleteSelection",
        "-",
        "Select All = input::SelectAll",
    ];
    const VIEW: [&str; 7] = [
        "Actual Size = treeterm::ResetZoom",
        "Zoom In = treeterm::ZoomIn",
        "Zoom Out = treeterm::ZoomOut",
        "-",
        "Toggle Full Screen = treeterm::ToggleFullScreen",
        "-",
        "Reset Input = treeterm::ResetInput",
    ];
    const HELP: [&str; 1] = ["Learn More = treeterm::LearnMore"];

    #[test]
    fn macos_has_electrons_menus() {
        let expected = vec![
            menu(
                "TreeTerm",
                &[
                    "About TreeTerm = treeterm::About",
                    "-",
                    "Preferences... = treeterm::OpenSettings",
                    "-",
                    "Services (system)",
                    "-",
                    "Hide TreeTerm = treeterm::Hide",
                    "Hide Others = treeterm::HideOthers",
                    "Show All = treeterm::ShowAll",
                    "-",
                    "Quit TreeTerm = treeterm::Quit",
                ],
            ),
            menu(
                "File",
                &["Close Window = treeterm::CloseWindow", "-", "Exit and Kill Daemon = treeterm::QuitAndKillDaemon"],
            ),
            menu("Edit", &MAC_EDIT),
            menu("View", &VIEW),
            menu(
                "Window",
                &[
                    "Minimize = treeterm::Minimize",
                    "Zoom = treeterm::ZoomWindow",
                    "-",
                    "Active Processes = treeterm::ActiveProcesses",
                    "-",
                    "Bring All to Front = treeterm::BringAllToFront",
                ],
            ),
            menu("Help", &HELP),
        ];
        assert_eq!(described(true), expected);
    }

    #[test]
    fn linux_has_electrons_menus() {
        let expected = vec![
            menu(
                "File",
                &[
                    "Settings = treeterm::OpenSettings",
                    "-",
                    "Quit = treeterm::Quit",
                    "-",
                    "Exit and Kill Daemon = treeterm::QuitAndKillDaemon",
                ],
            ),
            menu("Edit", &LINUX_EDIT),
            menu("View", &VIEW),
            menu(
                "Window",
                &[
                    "Minimize = treeterm::Minimize",
                    "-",
                    "Active Processes = treeterm::ActiveProcesses",
                    "Close = treeterm::CloseWindow",
                ],
            ),
            menu("Help", &HELP),
        ];
        assert_eq!(described(false), expected);
    }

    fn shortcuts(mac: bool) -> Vec<(String, String)> {
        let os = if mac { OperatingSystem::Mac } else { OperatingSystem::Linux };
        key_bindings(mac)
            .iter()
            .map(|binding| {
                let keys: Vec<String> = binding.keystrokes().iter().map(|k| k.unparse(os)).collect();
                (keys.join(" "), binding.action().name().to_owned())
            })
            .collect()
    }

    #[test]
    fn shortcuts_are_electrons_accelerators() {
        let pairs = |list: &[(&str, &str)]| -> Vec<(String, String)> {
            list.iter().map(|(k, a)| ((*k).to_owned(), (*a).to_owned())).collect()
        };
        assert_eq!(
            shortcuts(true),
            pairs(&[
                ("cmd-,", "treeterm::OpenSettings"),
                ("cmd-h", "treeterm::Hide"),
                ("alt-cmd-h", "treeterm::HideOthers"),
                ("cmd-q", "treeterm::Quit"),
                ("cmd-w", "treeterm::CloseWindow"),
                ("cmd-shift-q", "treeterm::QuitAndKillDaemon"),
                ("cmd-0", "treeterm::ResetZoom"),
                ("cmd-+", "treeterm::ZoomIn"),
                ("cmd-=", "treeterm::ZoomIn"),
                ("cmd--", "treeterm::ZoomOut"),
                ("ctrl-cmd-f", "treeterm::ToggleFullScreen"),
                ("cmd-m", "treeterm::Minimize"),
                ("alt-cmd-shift-v", "treeterm::PasteAndMatchStyle"),
            ])
        );
        assert_eq!(
            shortcuts(false),
            pairs(&[
                ("ctrl-,", "treeterm::OpenSettings"),
                ("ctrl-q", "treeterm::Quit"),
                ("ctrl-shift-q", "treeterm::QuitAndKillDaemon"),
                ("ctrl-m", "treeterm::Minimize"),
                ("ctrl-w", "treeterm::CloseWindow"),
                ("ctrl-0", "treeterm::ResetZoom"),
                ("ctrl-=", "treeterm::ZoomIn"),
                ("ctrl-+", "treeterm::ZoomIn"),
                ("ctrl--", "treeterm::ZoomOut"),
                ("f11", "treeterm::ToggleFullScreen"),
                ("ctrl-shift-z", "input::Redo"),
            ])
        );
        let keys: Vec<String> = shortcuts(false).into_iter().map(|(keys, _)| keys).collect();
        assert_eq!(keys, ACCELERATORS, "ACCELERATORS lists every shortcut off macOS");
    }

    /// Counts the commands it receives.
    #[derive(Default)]
    struct Probe {
        quits: usize,
    }

    impl Render for Probe {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }

    fn probe_window(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<Probe>) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| cx.new(|_| Probe::default()))
                .expect("open a window")
        })
    }

    #[gpui_kit::test]
    fn a_window_command_reaches_the_view_while_a_dialog_has_focus(cx: &mut TestAppContext) {
        let (window, probe) = probe_window(cx);
        let target = probe.downgrade();
        cx.update(|cx| route::<Quit, _>(window, target, |probe, _, _| probe.quits += 1, |_| panic!("the window is open"), cx));
        let cx = VisualTestContext::from_window(window, cx).into_mut();
        cx.update(|window, cx| window.open_dialog(cx, |dialog, _, _| dialog));
        cx.run_until_parked();
        assert!(cx.update(|window, cx| window.has_active_dialog(cx)));

        cx.simulate_keystrokes("cmd-q");
        cx.run_until_parked();
        assert_eq!(probe.read_with(cx, |probe, _| probe.quits), 1);
        assert!(cx.update(|_, cx| cx.is_action_available(&Quit)), "the menu item stays enabled");
    }

    #[gpui_kit::test]
    fn a_window_command_after_the_window_closed_runs_its_fallback(cx: &mut TestAppContext) {
        let (window, probe) = probe_window(cx);
        let fallbacks = Rc::new(std::cell::Cell::new(0));
        cx.update(|cx| {
            let fallbacks = fallbacks.clone();
            route::<Quit, _>(window, probe.downgrade(), |_, _, _| panic!("the window is closed"), move |_| fallbacks.set(fallbacks.get() + 1), cx);
        });
        window.update(cx, |_, window, _| window.remove_window()).expect("the window is open");
        cx.run_until_parked();

        cx.update(|cx| cx.dispatch_action(&Quit));
        cx.run_until_parked();
        assert_eq!(fallbacks.get(), 1);
    }

    /// A window that shows the in-window menu bar above its content.
    struct WithMenuBar(Entity<AppMenuBar>);

    impl Render for WithMenuBar {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div().size_full().flex().flex_col().child(in_window_bar(&self.0, cx)).child(div().flex_1())
        }
    }

    #[gpui_kit::test]
    fn the_in_window_menu_bar_works_while_a_dialog_is_open(cx: &mut TestAppContext) {
        use gpui_kit::test::TestWindowExt as _;

        let settings = Rc::new(std::cell::Cell::new(0));
        let window = cx.update(|cx| {
            gpui_kit::init(cx);
            crate::theme::install(cx);
            set_menus(false, cx);
            let opened = settings.clone();
            cx.on_action(move |_: &OpenSettings, _| opened.set(opened.get() + 1));
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| cx.new(|cx| WithMenuBar(AppMenuBar::new(cx))))
                .expect("open a window")
                .0
        });
        let cx = VisualTestContext::from_window(window, cx).into_mut();
        // A Kit dialog covers the whole window with its backdrop.
        cx.update(|window, cx| window.open_dialog(cx, |dialog, _, _| dialog));
        cx.run_until_parked();

        // File, then Settings, with the pointer.
        cx.update(|window, cx| window.within("app-menu-bar").within(0usize).click("menu", cx));
        cx.run_until_parked();
        cx.update(|window, cx| window.within("popup-menu").click(0usize, cx));
        cx.run_until_parked();
        assert_eq!(settings.get(), 1, "File > Settings ran");
        assert!(cx.update(|window, cx| window.has_active_dialog(cx)), "the dialog stays open");
    }

    /// A focused terminal: the keys that reach it and the menu commands that ran instead.
    struct Terminal {
        focus: gpui_kit::FocusHandle,
        keys: Vec<String>,
        commands: Vec<&'static str>,
    }

    impl Render for Terminal {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .key_context(terminal_view::CONTEXT)
                .track_focus(&self.focus)
                .on_key_down(cx.listener(|terminal, event: &gpui_kit::KeyDownEvent, _, _| {
                    terminal.keys.push(event.keystroke.unparse(OperatingSystem::Linux));
                }))
                .on_action(cx.listener(|terminal, _: &input::Copy, _, _| terminal.commands.push("Copy")))
                .on_action(cx.listener(|terminal, _: &input::Paste, _, _| terminal.commands.push("Paste")))
                .on_action(cx.listener(|terminal, _: &input::SelectAll, _, _| terminal.commands.push("SelectAll")))
        }
    }

    fn terminal_window(cx: &mut App) -> (AnyWindowHandle, Entity<Terminal>) {
        gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
            cx.new(|cx| Terminal { focus: cx.focus_handle(), keys: Vec::new(), commands: Vec::new() })
        })
        .expect("open a window")
    }

    #[gpui_kit::test]
    fn a_focused_terminal_keeps_the_menu_keys_it_encodes_off_macos(cx: &mut TestAppContext) {
        let linux = KeyOptions {
            alt_sends_escape: true,
            mac_word_motion: false,
        };
        let (window, terminal) = cx.update(|cx| {
            gpui_kit::init(cx);
            // GPUI Kit Root's Copy key off macOS, whatever the host.
            cx.bind_keys([KeyBinding::new("ctrl-c", input::Copy, Some("Root"))]);
            cx.bind_keys(terminal_view::edit_keys(false));
            bind_keys(false, linux, cx);
            install_zoom(ZoomStore::in_memory(), cx);
            terminal_window(cx)
        });
        cx.update(|cx| {
            fn record<A: Action>(name: &'static str, window: AnyWindowHandle, terminal: &Entity<Terminal>, cx: &mut App) {
                route::<A, _>(window, terminal.downgrade(), move |t, _, _| t.commands.push(name), |_| panic!("the window is open"), cx);
            }
            record::<Quit>("Quit", window, &terminal, cx);
            record::<CloseWindow>("CloseWindow", window, &terminal, cx);
            record::<Minimize>("Minimize", window, &terminal, cx);
            record::<ToggleFullScreen>("ToggleFullScreen", window, &terminal, cx);
            record::<QuitAndKillDaemon>("QuitAndKillDaemon", window, &terminal, cx);
            record::<OpenSettings>("OpenSettings", window, &terminal, cx);
        });
        let cx = VisualTestContext::from_window(window, cx).into_mut();
        cx.update(|window, cx| {
            let focus = terminal.read(cx).focus.clone();
            window.focus(&focus, cx);
        });
        cx.run_until_parked();

        // Ctrl+W deletes a word, Ctrl+Q is XON, Ctrl+M is CR, F11 and Ctrl+C go to the PTY.
        for keys in ["ctrl-w", "ctrl-q", "ctrl-m", "f11", "ctrl-c"] {
            cx.simulate_keystrokes(keys);
        }
        cx.run_until_parked();
        let (keys, commands) = terminal.read_with(cx, |t, _| (t.keys.clone(), t.commands.clone()));
        assert_eq!(keys, ["ctrl-w", "ctrl-q", "ctrl-m", "f11", "ctrl-c"]);
        assert!(commands.is_empty(), "the menu took {commands:?} from the terminal");

        // The terminal encodes neither: the menu keeps them.
        cx.simulate_keystrokes("ctrl-shift-q");
        cx.simulate_keystrokes("ctrl-,");
        cx.run_until_parked();
        let commands = terminal.read_with(cx, |t, _| t.commands.clone());
        assert_eq!(commands, ["QuitAndKillDaemon", "OpenSettings"]);

        // Nor the zoom keys (xterm.js leaves Ctrl+- to the app too): the menu zooms.
        assert_zoom_keys(cx, ["ctrl-+", "ctrl-=", "ctrl--", "ctrl-0"]);
        let keys = terminal.read_with(cx, |t, _| t.keys.clone());
        assert_eq!(keys, ["ctrl-w", "ctrl-q", "ctrl-m", "f11", "ctrl-c"], "a zoom key reached the terminal");

        // Copy, Paste and Select All take Shift.
        for keys in ["ctrl-shift-c", "ctrl-shift-v", "ctrl-shift-a"] {
            cx.simulate_keystrokes(keys);
        }
        cx.run_until_parked();
        let (keys, commands) = terminal.read_with(cx, |t, _| (t.keys.clone(), t.commands.clone()));
        assert_eq!(commands, ["QuitAndKillDaemon", "OpenSettings", "Copy", "Paste", "SelectAll"]);
        assert_eq!(keys, ["ctrl-w", "ctrl-q", "ctrl-m", "f11", "ctrl-c"]);
    }

    /// Edit's Copy, Paste and Select All, their keys and Paste and Match Style act on a focused
    /// terminal; the items it has nothing for are greyed out, except Delete, which is a no-op.
    #[gpui_kit::test]
    fn the_edit_menu_acts_on_a_focused_terminal(cx: &mut TestAppContext) {
        let (window, terminal) = cx.update(|cx| {
            gpui_kit::init(cx);
            cx.bind_keys(terminal_view::edit_keys(true));
            bind_keys(true, KeyOptions::for_host(), cx);
            let (window, terminal) = terminal_window(cx);
            route_edit(window, cx);
            (window, terminal)
        });
        let cx = VisualTestContext::from_window(window, cx).into_mut();
        cx.update(|window, cx| {
            // The menu bar asks the active window what its items can do.
            window.activate_window();
            let focus = terminal.read(cx).focus.clone();
            window.focus(&focus, cx);
        });
        cx.run_until_parked();

        for keys in ["cmd-c", "cmd-v", "cmd-a", "cmd-alt-shift-v"] {
            cx.simulate_keystrokes(keys);
        }
        cx.run_until_parked();
        let commands = terminal.read_with(cx, |t, _| t.commands.clone());
        assert_eq!(commands, ["Copy", "Paste", "SelectAll", "Paste"]);

        for action in [
            Box::new(input::Copy) as Box<dyn Action>,
            Box::new(input::Paste),
            Box::new(input::SelectAll),
            Box::new(PasteAndMatchStyle),
            Box::new(DeleteSelection),
        ] {
            cx.update(|window, cx| window.dispatch_action(action, cx));
            cx.run_until_parked();
        }
        let (keys, commands) = terminal.read_with(cx, |t, _| (t.keys.clone(), t.commands.clone()));
        assert_eq!(commands, ["Copy", "Paste", "SelectAll", "Paste", "Copy", "Paste", "SelectAll", "Paste"]);
        assert!(keys.is_empty(), "{keys:?} reached the terminal");

        let available = |cx: &mut VisualTestContext, action: &dyn Action| cx.cx.update(|cx| cx.is_action_available(action));
        assert!(!available(cx, &input::Undo) && !available(cx, &input::Redo) && !available(cx, &input::Cut));
        assert!(available(cx, &input::Copy) && available(cx, &input::Paste) && available(cx, &input::SelectAll));
        assert!(available(cx, &DeleteSelection) && available(cx, &PasteAndMatchStyle));
    }

    /// A focused text input.
    struct Field(Entity<input::InputState>);

    impl Render for Field {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            input::Input::new(&self.0)
        }
    }

    /// Every Edit item acts on a focused input. Delete removes the selection, and with nothing
    /// selected leaves the text as it is (Chromium's Delete).
    #[gpui_kit::test]
    fn the_edit_menu_acts_on_a_focused_input(cx: &mut TestAppContext) {
        let (window, field) = cx.update(|cx| {
            gpui_kit::init(cx);
            let (window, field) = gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                let state = cx.new(|cx| input::InputState::new(window, cx).default_value("hello"));
                cx.new(|_| Field(state))
            })
            .expect("open a window");
            route_edit(window, cx);
            (window, field)
        });
        let cx = VisualTestContext::from_window(window, cx).into_mut();
        let state = field.read_with(cx, |field, _| field.0.clone());
        cx.update(|window, cx| {
            state.update(cx, |state, cx| {
                state.focus(window, cx);
                state.set_selected_range(5..5, cx);
            });
        });
        cx.run_until_parked();
        let menu = |cx: &mut VisualTestContext, action: &dyn Action| {
            cx.update(|window, cx| window.dispatch_action(action.boxed_clone(), cx));
            cx.run_until_parked();
            state.read_with(cx, |state, _| state.value().to_string())
        };

        assert_eq!(menu(cx, &DeleteSelection), "hello", "nothing selected");
        assert_eq!(menu(cx, &input::SelectAll), "hello");
        assert_eq!(menu(cx, &input::Copy), "hello");
        assert_eq!(cx.read_from_clipboard().and_then(|item| item.text()).as_deref(), Some("hello"));
        assert_eq!(menu(cx, &DeleteSelection), "");
        assert_eq!(menu(cx, &input::Undo), "hello");
        assert_eq!(menu(cx, &input::Redo), "");
        assert_eq!(menu(cx, &input::Paste), "hello");
        assert_eq!(menu(cx, &PasteAndMatchStyle), "hellohello");
        menu(cx, &input::SelectAll);
        assert_eq!(menu(cx, &input::Cut), "");
        assert_eq!(cx.read_from_clipboard().and_then(|item| item.text()).as_deref(), Some("hellohello"));
    }

    /// Redo is Electron's Ctrl+Shift+Z off macOS too, as well as GPUI Kit's Ctrl+Y.
    #[gpui_kit::test]
    fn redo_is_ctrl_shift_z_off_macos(cx: &mut TestAppContext) {
        let linux = KeyOptions {
            alt_sends_escape: true,
            mac_word_motion: false,
        };
        let (window, field) = cx.update(|cx| {
            gpui_kit::init(cx);
            bind_keys(false, linux, cx);
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                let state = cx.new(|cx| input::InputState::new(window, cx).default_value("hello"));
                cx.new(|_| Field(state))
            })
            .expect("open a window")
        });
        let cx = VisualTestContext::from_window(window, cx).into_mut();
        let state = field.read_with(cx, |field, _| field.0.clone());
        cx.update(|window, cx| state.update(cx, |state, cx| state.focus(window, cx)));
        cx.run_until_parked();
        let text = |cx: &mut VisualTestContext| state.read_with(cx, |state, _| state.value().to_string());

        cx.update(|window, cx| window.dispatch_action(Box::new(input::SelectAll), cx));
        cx.update(|window, cx| window.dispatch_action(Box::new(input::Cut), cx));
        cx.update(|window, cx| window.dispatch_action(Box::new(input::Undo), cx));
        cx.run_until_parked();
        assert_eq!(text(cx), "hello");
        cx.simulate_keystrokes("ctrl-shift-z");
        cx.run_until_parked();
        assert_eq!(text(cx), "");
    }

    /// Presses Zoom In's two keys, then Zoom Out's and Actual Size's, checking the app's zoom
    /// after each.
    fn assert_zoom_keys(cx: &mut VisualTestContext, [zoom_in, zoom_in_too, zoom_out, actual_size]: [&str; 4]) {
        let zoom = |cx: &mut VisualTestContext| cx.update(|_, cx| cx.global::<ZoomStore>().zoom());
        let mut once = Zoom::default();
        once.zoom_in();
        let mut twice = once;
        twice.zoom_in();
        for (keys, expected) in [(zoom_in, once), (zoom_in_too, twice), (zoom_out, once), (actual_size, Zoom::default())] {
            cx.simulate_keystrokes(keys);
            cx.run_until_parked();
            assert_eq!(zoom(cx), expected, "after {keys}");
        }
    }

    /// The zoom is the app's, as Electron's roles zoom whatever page the window shows: its keys
    /// work while a dialog has focus. Each change is saved for the next start.
    #[gpui_kit::test]
    fn the_zoom_keys_work_while_a_dialog_has_focus(cx: &mut TestAppContext) {
        let user_data = tempfile::tempdir().expect("a user data directory");
        let window = cx.update(|cx| {
            gpui_kit::init(cx);
            bind_keys(true, KeyOptions::for_host(), cx);
            install_zoom(ZoomStore::load(user_data.path()), cx);
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| cx.new(|_| Probe::default()))
                .expect("open a window")
                .0
        });
        let cx = VisualTestContext::from_window(window, cx).into_mut();
        cx.update(|window, cx| window.open_dialog(cx, |dialog, _, _| dialog));
        cx.run_until_parked();

        assert_zoom_keys(cx, ["cmd-+", "cmd-=", "cmd--", "cmd-0"]);
        assert!(cx.update(|window, cx| window.has_active_dialog(cx)), "the dialog stays open");
        assert!(cx.update(|_, cx| cx.is_action_available(&ZoomIn)), "the menu item stays enabled");

        assert_eq!(ZoomStore::load(user_data.path()).zoom(), Zoom::default(), "Actual Size saved");
        cx.simulate_keystrokes("cmd-+");
        cx.run_until_parked();
        let zoomed = cx.update(|_, cx| cx.global::<ZoomStore>().zoom());
        assert_ne!(zoomed, Zoom::default());
        assert_eq!(ZoomStore::load(user_data.path()).zoom(), zoomed, "Zoom In saved");
    }
}
