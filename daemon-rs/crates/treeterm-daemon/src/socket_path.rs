use std::path::PathBuf;

pub fn socket_path() -> PathBuf {
    if let Ok(p) = std::env::var("TREETERM_SOCKET_PATH") {
        return PathBuf::from(p);
    }
    let uid = unsafe { libc::getuid() };
    PathBuf::from(format!("/tmp/treeterm-{uid}/daemon.sock"))
}
