mod exec_manager;
pub mod filesystem;
mod grpc_server;
pub mod pty_manager;
pub mod session_store;
mod socket_path;

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process;

use tokio::net::UnixListener;
use tokio_stream::wrappers::UnixListenerStream;
use tonic::transport::Server;
use treeterm_proto::treeterm::tree_term_daemon_server::TreeTermDaemonServer;

fn data_dir() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
        .join(".treeterm")
}

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

    let incoming = UnixListenerStream::new(listener);

    // Initialize components (one session per daemon, PtyManager embedded)
    let session_store = session_store::SessionStore::new();
    session_store.start_heartbeat();

    let svc = grpc_server::DaemonService::new(session_store.clone());

    let ss = session_store.clone();
    let shutdown = async move {
        tokio::signal::ctrl_c().await.ok();
        tracing::info!("shutting down");
        ss.shutdown().await;
    };

    Server::builder()
        .add_service(
            TreeTermDaemonServer::new(svc)
                .max_decoding_message_size(8 * 1024 * 1024)
                .max_encoding_message_size(8 * 1024 * 1024),
        )
        .serve_with_incoming_shutdown(incoming, shutdown)
        .await?;

    remove_pid_file(&data);
    let _ = fs::remove_file(&socket_path);
    Ok(())
}
