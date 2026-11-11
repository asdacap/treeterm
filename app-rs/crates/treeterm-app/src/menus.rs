//! Application menu and global shortcuts. Port of `src/main/menu.ts` plus the direct
//! Cmd/Ctrl+1–9 tab switching of `store/keybinding.ts`.

use gpui_kit::{App, Entity, KeyBinding, Menu, MenuItem, OsAction, actions};
use treeterm_model::app_store::AppStore;

use crate::terminal_view;

actions!(treeterm, [Quit, QuitAndKillDaemon, Minimize]);
actions!(
    workspace,
    [
        SwitchToTab1,
        SwitchToTab2,
        SwitchToTab3,
        SwitchToTab4,
        SwitchToTab5,
        SwitchToTab6,
        SwitchToTab7,
        SwitchToTab8,
        SwitchToTab9,
        NewTab
    ]
);

/// The tab index (0-based) a switch action selects.
pub fn tab_index(action: &dyn gpui_kit::Action) -> Option<usize> {
    let any = action.as_any();
    [
        any.is::<SwitchToTab1>(),
        any.is::<SwitchToTab2>(),
        any.is::<SwitchToTab3>(),
        any.is::<SwitchToTab4>(),
        any.is::<SwitchToTab5>(),
        any.is::<SwitchToTab6>(),
        any.is::<SwitchToTab7>(),
        any.is::<SwitchToTab8>(),
        any.is::<SwitchToTab9>(),
    ]
    .iter()
    .position(|hit| *hit)
}

pub fn install(store: Entity<AppStore>, cx: &mut App) {
    // Electron switches on Cmd *or* Ctrl + digit (`store/keybinding.ts`).
    let keys: [&'static str; 9] = if cfg!(target_os = "macos") {
        ["cmd-1", "cmd-2", "cmd-3", "cmd-4", "cmd-5", "cmd-6", "cmd-7", "cmd-8", "cmd-9"]
    } else {
        ["ctrl-1", "ctrl-2", "ctrl-3", "ctrl-4", "ctrl-5", "ctrl-6", "ctrl-7", "ctrl-8", "ctrl-9"]
    };
    let mut bindings = vec![
        KeyBinding::new(keys[0], SwitchToTab1, None),
        KeyBinding::new(keys[1], SwitchToTab2, None),
        KeyBinding::new(keys[2], SwitchToTab3, None),
        KeyBinding::new(keys[3], SwitchToTab4, None),
        KeyBinding::new(keys[4], SwitchToTab5, None),
        KeyBinding::new(keys[5], SwitchToTab6, None),
        KeyBinding::new(keys[6], SwitchToTab7, None),
        KeyBinding::new(keys[7], SwitchToTab8, None),
        KeyBinding::new(keys[8], SwitchToTab9, None),
    ];
    if cfg!(target_os = "macos") {
        bindings.push(KeyBinding::new("cmd-q", Quit, None));
        bindings.push(KeyBinding::new("cmd-shift-q", QuitAndKillDaemon, None));
        bindings.push(KeyBinding::new("cmd-m", Minimize, None));
    } else {
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
            Menu::new("TreeTerm").items([MenuItem::action("Quit TreeTerm", Quit)]),
            Menu::new("File").items([quit_and_kill]),
            edit,
            window,
        ]
    } else {
        vec![
            Menu::new("File").items([MenuItem::action("Quit", Quit), quit_and_kill]),
            edit,
            window,
        ]
    };
    cx.set_menus(menus);
}
