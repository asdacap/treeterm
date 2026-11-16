//! TreeTerm native app: one process, one UI, talking gRPC to the TreeTerm daemon.

mod active_processes_view;
mod app_view;
mod assets;
mod auto_open_dialog;
mod avif;
mod connection_picker;
mod open_panel;
mod open_workspace_dialog;
mod remote_dir_browser;
mod session_info_view;
mod child_dialog;
mod cli;
mod comment_ui;
#[cfg(feature = "automation")]
mod automation;
mod crash_view;
mod browser_view;
mod webview_native;
mod daemon_binary;
mod editor_hooks;
mod file_browser_view;
mod json_view;
mod keybinding_overlay;
#[cfg(target_os = "macos")]
mod macos;
mod menus;
mod number_field;
mod review_view;
mod search_panel;
mod tab_layout_view;
mod tab_strip;
mod settings_view;
mod terminal_element;
mod text_editor_view;
mod terminal_view;
mod theme;
mod tool_views;
mod traffic_badge;
mod tree_pane;
mod ui;

use gpui_kit::{
    App, AppContext as _, Bounds, Entity, TitlebarOptions, WindowBounds, WindowOptions, point, px,
    size,
};
use treeterm_core::keybinding::{
    KeyOutcome, KeyPress, KeybindingSettings, KeybindingSettingsError, Modifiers, Os, ParseError,
};
use treeterm_model::keybinding_store::KeybindingStore;
use treeterm_model::settings_store::{SettingsEvent, SettingsStore};
use treeterm_core::crash_report::{self, CrashReport};
use treeterm_core::local_daemon::LocalDaemonSpec;
use treeterm_core::logging::{self, StoredLevel};
use treeterm_core::paths::{self, process_env};
use treeterm_core::ui_state;
use treeterm_core::rt::Rt;
use treeterm_model::app_store::AppStore;
use treeterm_model::zoom::ZoomStore;

use crate::app_view::AppView;

/// Whether the window counts as the focused window, which decides whether its focused
/// terminal may write. Automation drives a window that is usually behind other apps.
pub fn window_counts_as_active(window: &gpui_kit::Window) -> bool {
    window.is_window_active() || cfg!(feature = "automation") && automation_enabled()
}

fn automation_enabled() -> bool {
    std::env::var_os("TREETERM_AUTOMATION_SOCKET").is_some()
}

/// Prefix-mode settings from the loaded settings. A key that can match no key press does
/// nothing, as in Electron, and is logged; the other keys keep working.
fn keybinding_settings(settings: &treeterm_core::settings::Settings) -> KeybindingSettings {
    let keys = KeybindingSettings::from_settings(settings, Os::host());
    for problem in keys.problems() {
        match problem {
            // An empty prefix key is how Electron's settings turn prefix mode off.
            KeybindingSettingsError::PrefixKey(ParseError::Empty) => tracing::info!("no prefix key; prefix mode is off"),
            problem => tracing::error!(%problem, "key binding matches no key press; it does nothing"),
        }
    }
    keys
}

/// A GPUI keystroke as the shortcut machine reads it.
pub(crate) fn key_press(k: &gpui_kit::Keystroke) -> KeyPress {
    let m = k.modifiers;
    KeyPress::from_gpui(
        &k.key,
        k.key_char.as_deref(),
        Modifiers { control: m.control, alt: m.alt, shift: m.shift, platform: m.platform, function: m.function },
    )
}

/// Every keystroke passes the shortcut machine before the focused view (a terminal) sees
/// it, so the prefix key and its follow-up never reach the PTY.
fn intercept_shortcuts(keybindings: Entity<KeybindingStore>, view: Entity<AppView>, cx: &mut App) {
    cx.intercept_keystrokes(move |event, window, cx| {
        let press = key_press(&event.keystroke);
        match keybindings.update(cx, |kb, cx| kb.handle_key(&press, cx)) {
            KeyOutcome::PassThrough => {}
            KeyOutcome::Consumed => cx.stop_propagation(),
            KeyOutcome::Perform(action) => {
                cx.stop_propagation();
                view.update(cx, |view, cx| view.perform(action, window, cx));
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

/// The app's window: 1200×800, centred, at least 800×600, its title bar drawn by the app
/// (Electron's `hiddenInset`).
fn window_options(cx: &App) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(1200.), px(800.)), cx))),
        window_min_size: Some(size(px(800.), px(600.))),
        // Matches the .desktop file, so Linux launchers show the right icon.
        app_id: Some("treeterm".to_owned()),
        titlebar: Some(TitlebarOptions {
            title: Some("TreeTerm".into()),
            appears_transparent: true,
            traffic_light_position: Some(point(px(15.), px(15.))),
        }),
        ..WindowOptions::default()
    }
}

/// A panic on the UI thread ends the app: GPUI cannot carry on, and the panic would unwind
/// into AppKit, which aborts with the system's crash dialog. Once the hook set before has
/// logged the panic (`logging::install_panic_hook`), hand off to a new process that shows
/// Electron's "Application Error" screen, and exit. Panics elsewhere go on as before (a
/// tokio task's panic is that task's `TaskLost`).
fn install_crash_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_hook(info);
        if !crash_report::is_ui_thread(std::thread::current().name()) {
            return;
        }
        let report = CrashReport::from_panic(info, &std::backtrace::Backtrace::force_capture());
        let handed_off = match paths::user_data_dir() {
            Ok(dir) => crash_report::hand_off(&report, &dir).map_err(|error| error.to_string()),
            Err(error) => Err(error.to_string()),
        };
        match handed_off {
            Ok(()) => std::process::exit(101),
            Err(error) => tracing::error!(%error, "cannot show the crash screen"),
        }
    }));
}

/// Start the log, on stdout. Its level is `RUST_LOG` when that is set (tracing's directives),
/// else the `logLevel` kept in `ui-state.json` (Electron's localStorage `treeterm:logLevel`),
/// else info. What could not be read is logged once the log runs, and so are panics from
/// then on.
fn start_logging() {
    let stored = paths::user_data_dir()
        .map_err(|error| error.to_string())
        .and_then(|dir| {
            ui_state::read_log_level(&ui_state::ui_state_path(&dir)).map_err(|error| error.to_string())
        });
    let level = match &stored {
        Ok(name) => StoredLevel::from_stored(name.as_deref()),
        Err(_) => StoredLevel::Absent,
    };
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(level.level().directive()));
    tracing_subscriber::fmt().with_env_filter(filter).init();
    match (stored, level) {
        (Err(error), _) => tracing::error!(%error, "logLevel not read from UI state; it is ignored"),
        (Ok(_), StoredLevel::Unknown(name)) => tracing::warn!(
            log_level = name,
            "logLevel in ui-state.json is not debug, info, warn, error or silent; it is ignored"
        ),
        (Ok(_), StoredLevel::Absent | StoredLevel::Known(_)) => {}
    }
    logging::install_panic_hook();
}

fn main() {
    // Review's AVIF decoder (`avif::ARG`): before logging and its panic hook, so a panic there
    // never opens the crash screen, and without the GUI.
    if std::env::args_os().nth(1).is_some_and(|arg| arg == avif::ARG) {
        std::process::exit(avif::serve());
    }
    start_logging();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let cwd = std::env::current_dir().unwrap_or_default();
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    let command = match cli::parse(&args, &cwd, now_ms) {
        Ok(command) => command,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(1);
        }
    };
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
    let (initial_workspace, initial_ssh) = match command {
        cli::Command::Help => {
            println!("{}", cli::USAGE);
            return;
        }
        cli::Command::ShutdownDaemon => {
            let result = tokio.block_on(treeterm_core::daemon::shutdown_daemon(&rt, &spec.socket_path));
            let error = match result {
                Ok(Ok(())) => {
                    println!("Daemon shut down");
                    return;
                }
                Ok(Err(error)) => error.to_string(),
                Err(lost) => lost.to_string(),
            };
            eprintln!("{error}");
            std::process::exit(1);
        }
        cli::Command::CrashScreen { report } => {
            let report = match CrashReport::read(&report) {
                Ok(report) => report,
                Err(error) => {
                    eprintln!("treeterm: {error}");
                    std::process::exit(1);
                }
            };
            gpui_kit::application().with_assets(assets::AppAssets).run(move |cx| {
                gpui_kit::init(cx);
                theme::install(cx);
                ui::bind_keys(cx);
                let options = window_options(cx);
                let (window, view) = crash_view::open(report, options, cx);
                #[cfg(feature = "automation")]
                if let Ok(path) = std::env::var("TREETERM_AUTOMATION_SOCKET") {
                    automation::start(path.into(), window, automation::Screen::Crash(view), &rt, cx);
                }
                #[cfg(not(feature = "automation"))]
                drop((window, view, rt));
                cx.activate(true);
            });
            return;
        }
        cli::Command::Gui { workspace, ssh } => (workspace, ssh),
    };

    #[cfg(feature = "automation")]
    let rt_for_automation = rt.clone();
    // Before AppKit finishes launching, as Electron does it.
    #[cfg(target_os = "macos")]
    macos::disable_auto_full_screen_item();
    install_crash_hook();
    gpui_kit::application().with_assets(assets::AppAssets).run(move |cx| {
        gpui_kit::init(cx);
        theme::install(cx);
        ui::bind_keys(cx);
        terminal_view::bind_keys(cx);
        text_editor_view::bind_keys(cx);
        file_browser_view::bind_keys(cx);
        review_view::bind_keys(cx);
        let settings = cx.new(|cx| SettingsStore::new(paths::user_data_dir().ok(), cx));
        let store = cx.new(|cx| AppStore::new(rt, spec, settings.clone(), cx));
        if let Some(config) = initial_ssh {
            store.update(cx, |s, cx| s.connect_remote(*config, treeterm_core::connection::OutdatedDaemon::Report, cx));
        }
        // View > Zoom's level from the last run.
        let zoom = match paths::user_data_dir() {
            Ok(dir) => ZoomStore::load(&dir),
            Err(error) => {
                tracing::error!(%error, "the zoom level is not saved");
                ZoomStore::in_memory()
            }
        };
        menus::install(zoom, cx);
        let options = window_options(cx);
        let initial_keys = keybinding_settings(settings.read(cx).settings());
        let keybindings = cx.new(|_| KeybindingStore::new(initial_keys));
        cx.subscribe(&settings, {
            let keybindings = keybindings.clone();
            move |settings, event: &SettingsEvent, cx| match event {
                SettingsEvent::Changed => {
                    let next = keybinding_settings(settings.read(cx).settings());
                    keybindings.update(cx, |kb, cx| kb.set_settings(next, cx));
                }
            }
        })
        .detach();
        let names = cx.new(|_| treeterm_model::session_names::SessionNames::new(paths::user_data_dir().ok()));
        let (window, view) = gpui_kit::open_window(options, cx, {
            let (store, keybindings, settings) = (store.clone(), keybindings.clone(), settings.clone());
            |window, cx| cx.new(|cx| AppView::new(store, keybindings, settings, names, window, cx))
        })
        .expect("open the main window");
        intercept_shortcuts(keybindings, view.clone(), cx);
        menus::route_to_window(window, view.downgrade(), store, cx);
        if let Some(path) = initial_workspace {
            view.update(cx, |v, cx| v.open_on_start(path.to_string_lossy().into_owned(), cx));
        }
        // Closing the window asks about unmerged workspaces; closing the last one quits, as
        // Electron's `window-all-closed`.
        let closing = view.clone();
        match window.update(cx, |_, window, cx| {
            window.on_window_should_close(cx, move |window, cx| closing.update(cx, |v, cx| v.request_close(window, cx)));
        }) {
            Ok(()) => {}
            Err(error) => tracing::error!(%error, "could not watch the window closing"),
        }
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        #[cfg(feature = "automation")]
        if let Ok(path) = std::env::var("TREETERM_AUTOMATION_SOCKET") {
            automation::start(path.into(), window, automation::Screen::App(view), &rt_for_automation, cx);
        }
        #[cfg(not(feature = "automation"))]
        drop((window, view));
        cx.activate(true);
    });
    drop(tokio);
}
