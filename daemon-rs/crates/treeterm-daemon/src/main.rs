use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::process;

use tokio::net::UnixListener;
use treeterm_daemon::paths::{self, data_dir};
use treeterm_daemon::{DaemonConfig, serve, socket_path};

fn check_already_running(data_dir: &std::path::Path) -> bool {
    let pid_path = data_dir.join("daemon.pid");
    if let Ok(contents) = fs::read_to_string(&pid_path) {
        if let Ok(pid) = contents.trim().parse::<i32>() {
            // Check if process is alive AND the socket exists — a live PID without a socket
            // means the PID was recycled by the OS to an unrelated process.
            let alive = unsafe { libc::kill(pid, 0) } == 0;
            if alive && socket_path::socket_path().exists() {
                return true;
            }
            // Stale PID file (process dead or PID recycled), remove it
            let _ = fs::remove_file(&pid_path);
        }
    }
    false
}

fn write_pid_file(data_dir: &std::path::Path) {
    let pid_path = data_dir.join("daemon.pid");
    let mut f = fs::File::create(&pid_path).expect("failed to create pid file");
    write!(f, "{}", process::id()).expect("failed to write pid");
}

fn remove_pid_file(data_dir: &std::path::Path) {
    let _ = fs::remove_file(data_dir.join("daemon.pid"));
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let data = data_dir();
    fs::create_dir_all(&data)?;
    fs::create_dir_all(paths::workspaces_dir())?;

    if check_already_running(&data) {
        tracing::info!("daemon already running, exiting");
        process::exit(0);
    }

    write_pid_file(&data);

    let socket_path = socket_path::socket_path();
    if let Some(parent) = socket_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let _ = fs::remove_file(&socket_path);

    let listener = UnixListener::bind(&socket_path)?;
    fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600))?;
    tracing::info!(?socket_path, "daemon listening");

    let config = DaemonConfig {
        workspaces_dir: paths::workspaces_dir(),
    };
    let shutdown = async {
        tokio::signal::ctrl_c().await.ok();
    };
    serve(config, listener, shutdown).await?;

    remove_pid_file(&data);
    let _ = fs::remove_file(&socket_path);
    Ok(())
}
