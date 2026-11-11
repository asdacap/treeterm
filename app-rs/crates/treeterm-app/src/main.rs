//! TreeTerm native app: one process, one UI, talking gRPC to the TreeTerm daemon.

mod app_view;
#[cfg(feature = "automation")]
mod automation;
mod daemon_binary;
mod keybinding_overlay;
mod menus;
mod terminal_element;
mod terminal_view;
mod theme;
mod tree_pane;

use gpui_kit::{
    App, AppContext as _, Bounds, Entity, TitlebarOptions, WindowBounds, WindowOptions, point, px,
    size,
};
use treeterm_core::keybinding::{KeyOutcome, KeyPress, KeybindingSettings, Modifiers, Os};
use treeterm_model::keybinding_store::KeybindingStore;
use treeterm_core::local_daemon::LocalDaemonSpec;
use treeterm_core::paths::{self, process_env};
use treeterm_core::rt::Rt;
use treeterm_model::app_store::AppStore;

use crate::app_view::AppView;

/// Whether the window counts as the focused window, which decides whether its focused
/// terminal may write. Automation drives a window that is usually behind other apps.
pub fn window_counts_as_active(window: &gpui_kit::Window) -> bool {
    window.is_window_active() || cfg!(feature = "automation") && automation_enabled()
}

fn automation_enabled() -> bool {
    std::env::var_os("TREETERM_AUTOMATION_SOCKET").is_some()
}

/// Prefix-mode settings from `settings.json`. A value that does not parse falls back to the
/// defaults, loudly.
fn keybinding_settings() -> KeybindingSettings {
    let raw = paths::user_data_dir()
        .ok()
        .and_then(|dir| std::fs::read(treeterm_core::settings::settings_path(&dir)).ok())
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .unwrap_or(serde_json::Value::Null);
    match KeybindingSettings::from_settings_json(&raw, Os::host()) {
        Ok(settings) => settings,
        Err(error) => {
            tracing::error!(%error, "invalid keybinding settings; using defaults");
            KeybindingSettings::default()
        }
    }
}

/// Every keystroke passes the shortcut machine before the focused view (a terminal) sees
/// it, so the prefix key and its follow-up never reach the PTY.
fn intercept_shortcuts(keybindings: Entity<KeybindingStore>, view: Entity<AppView>, cx: &mut App) {
    cx.intercept_keystrokes(move |event, _window, cx| {
        let k = &event.keystroke;
        let m = k.modifiers;
        let press = KeyPress::from_gpui(
            &k.key,
            k.key_char.as_deref(),
            Modifiers {
                control: m.control,
                alt: m.alt,
                shift: m.shift,
                platform: m.platform,
                function: m.function,
            },
        );
        match keybindings.update(cx, |kb, cx| kb.handle_key(&press, cx)) {
            KeyOutcome::PassThrough => {}
            KeyOutcome::Consumed => cx.stop_propagation(),
            KeyOutcome::Perform(action) => {
                cx.stop_propagation();
                view.update(cx, |view, cx| view.perform(action, cx));
            }
        }
    })
    .detach();
}

fn local_daemon_spec() -> Result<LocalDaemonSpec, paths::PathError> {
    let env: paths::Env<'_> = &process_env;
    Ok(LocalDaemonSpec {
        socket_path: paths::local_socket_path(env, paths::current_uid()),
        binary: daemon_binary::locate(),
        log_path: paths::user_data_dir()?.join("daemon.log"),
        pid_file: paths::data_dir(env)?.join("daemon.pid"),
    })
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let spec = match local_daemon_spec() {
        Ok(spec) => spec,
        Err(error) => {
            eprintln!("treeterm: {error}");
            std::process::exit(1);
        }
    };
    let tokio = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("treeterm-io")
        .build()
        .expect("start the I/O runtime");
    let rt = Rt::new(tokio.handle().clone());
    let settings = match paths::user_data_dir().map_err(|e| e.to_string()).and_then(|dir| {
        treeterm_core::settings::load(&dir).map_err(|e| e.to_string())
    }) {
        Ok(settings) => settings,
        Err(error) => {
            // Same fallback as Electron, but said out loud.
            tracing::error!(%error, "settings unreadable; using defaults");
            treeterm_core::settings::Settings::default()
        }
    };

    #[cfg(feature = "automation")]
    let rt_for_automation = rt.clone();
    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);
        theme::install(cx);
        terminal_view::bind_keys(cx);
        let store = cx.new(|cx| AppStore::new(rt, spec, settings, cx));
        menus::install(store.clone(), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(1200.), px(800.)),
                cx,
            ))),
            window_min_size: Some(size(px(800.), px(600.))),
            titlebar: Some(TitlebarOptions {
                title: Some("TreeTerm".into()),
                appears_transparent: true,
                traffic_light_position: Some(point(px(15.), px(15.))),
            }),
            ..WindowOptions::default()
        };
        let keybindings = cx.new(|_| KeybindingStore::new(keybinding_settings()));
        let (window, view) = gpui_kit::open_window(options, cx, {
            let keybindings = keybindings.clone();
            |_, cx| cx.new(|cx| AppView::new(store, keybindings, cx))
        })
        .expect("open the main window");
        intercept_shortcuts(keybindings, view.clone(), cx);
        #[cfg(feature = "automation")]
        if let Ok(path) = std::env::var("TREETERM_AUTOMATION_SOCKET") {
            automation::start(path.into(), window, view, &rt_for_automation, cx);
        }
        #[cfg(not(feature = "automation"))]
        drop((window, view));
        cx.activate(true);
    });
    drop(tokio);
}
