//! TreeTerm native app: one process, one UI, talking gRPC to the TreeTerm daemon.

mod app_view;
#[cfg(feature = "automation")]
mod automation;
mod daemon_binary;
mod terminal_element;
mod terminal_view;
mod theme;
mod tree_pane;

use gpui_kit::{
    AppContext as _, Bounds, TitlebarOptions, WindowBounds, WindowOptions, point, px, size,
};
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
        let (window, view) =
            gpui_kit::open_window(options, cx, |_, cx| cx.new(|cx| AppView::new(store, cx)))
                .expect("open the main window");
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
