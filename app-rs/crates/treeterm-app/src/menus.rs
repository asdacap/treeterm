//! Application menu. Port of `src/main/menu.ts`. Workspace shortcuts (prefix mode,
//! Cmd/Ctrl+1–9) go through the keybinding state machine; see `AppView::perform`.

use gpui_kit::{App, Entity, KeyBinding, Menu, MenuItem, OsAction, actions};
use treeterm_model::app_store::AppStore;

use crate::terminal_view;

actions!(treeterm, [Quit, QuitAndKillDaemon, Minimize, OpenSettings]);
pub fn install(store: Entity<AppStore>, cx: &mut App) {
    let mut bindings = Vec::new();
    if cfg!(target_os = "macos") {
        bindings.push(KeyBinding::new("cmd-,", OpenSettings, None));
        bindings.push(KeyBinding::new("cmd-q", Quit, None));
        bindings.push(KeyBinding::new("cmd-shift-q", QuitAndKillDaemon, None));
        bindings.push(KeyBinding::new("cmd-m", Minimize, None));
    } else {
        bindings.push(KeyBinding::new("ctrl-,", OpenSettings, None));
        bindings.push(KeyBinding::new("ctrl-shift-q", QuitAndKillDaemon, None));
    }
    cx.bind_keys(bindings);

    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.on_action(move |_: &QuitAndKillDaemon, cx| {
        let shutdown = store.read(cx).shutdown_daemon(cx);
        cx.spawn(async move |cx| {
            if let Some(shutdown) = shutdown {
                match shutdown.await {
                    Ok(Ok(())) => tracing::info!("daemon shut down"),
                    Ok(Err(error)) => tracing::error!(%error, "daemon shutdown failed"),
                    Err(lost) => tracing::error!(%lost, "daemon shutdown failed"),
                }
            }
            cx.update(|cx| cx.quit());
        })
        .detach();
    });

    let quit_and_kill = MenuItem::action("Exit and Kill Daemon", QuitAndKillDaemon);
    let edit = Menu::new("Edit").items([
        MenuItem::os_action("Copy", terminal_view::Copy, OsAction::Copy),
        MenuItem::os_action("Paste", terminal_view::Paste, OsAction::Paste),
        MenuItem::os_action("Select All", terminal_view::SelectAll, OsAction::SelectAll),
    ]);
    let window = Menu::new("Window").items([MenuItem::action("Minimize", Minimize)]);
    let menus = if cfg!(target_os = "macos") {
        vec![
            Menu::new("TreeTerm").items([
                MenuItem::action("Preferences…", OpenSettings),
                MenuItem::separator(),
                MenuItem::action("Quit TreeTerm", Quit),
            ]),
            Menu::new("File").items([quit_and_kill]),
            edit,
            window,
        ]
    } else {
        vec![
            Menu::new("File").items([
                MenuItem::action("Settings", OpenSettings),
                MenuItem::separator(),
                MenuItem::action("Quit", Quit),
                quit_and_kill,
            ]),
            edit,
            window,
        ]
    };
    cx.set_menus(menus);
}
