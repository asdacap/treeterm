use std::path::PathBuf;

pub fn socket_path() -> PathBuf {
    if let Ok(p) = std::env::var("TREETERM_SOCKET_PATH") {
        return PathBuf::from(p);
    }
    let uid = unsafe { libc::getuid() };
    PathBuf::from(format!("/tmp/treeterm-{uid}/daemon.sock"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Env var tests must run serially since they mutate global state
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn default_socket_path_contains_uid() {
        let _guard = ENV_LOCK.lock().unwrap();
        let original = std::env::var("TREETERM_SOCKET_PATH").ok();
        unsafe { std::env::remove_var("TREETERM_SOCKET_PATH") };

        let path = socket_path();
        let uid = unsafe { libc::getuid() };
        assert_eq!(path, PathBuf::from(format!("/tmp/treeterm-{uid}/daemon.sock")));

        if let Some(v) = original {
            unsafe { std::env::set_var("TREETERM_SOCKET_PATH", v) };
        }
    }

    #[test]
    fn socket_path_respects_env_override() {
        let _guard = ENV_LOCK.lock().unwrap();
        let original = std::env::var("TREETERM_SOCKET_PATH").ok();
        unsafe { std::env::set_var("TREETERM_SOCKET_PATH", "/custom/path.sock") };

        let path = socket_path();
        assert_eq!(path, PathBuf::from("/custom/path.sock"));

        match original {
            Some(v) => unsafe { std::env::set_var("TREETERM_SOCKET_PATH", v) },
            None => unsafe { std::env::remove_var("TREETERM_SOCKET_PATH") },
        }
    }
}
