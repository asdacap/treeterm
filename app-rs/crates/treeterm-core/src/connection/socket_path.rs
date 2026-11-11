//! Local socket paths for ssh tunnels (port of Electron's `socketPath.ts`; the local daemon
//! socket rule lives in [`crate::paths::local_socket_path`]).

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// `/tmp/treeterm-<uid>`: where tunnel sockets go. Always `/tmp`, never `$TMPDIR`: macOS's
/// long `TMPDIR` would push socket paths past the ~104-byte limit.
pub fn forward_socket_dir(uid: u32) -> PathBuf {
    PathBuf::from(format!("/tmp/treeterm-{uid}"))
}

/// A fresh local end for a tunnel to `connection_id`'s daemon:
/// `<dir>/r-<sha256(id)[..12]>-<uuid[..8]>.sock`. Unique per call, so two windows (or a
/// tunnel and its replacement) never share a socket.
pub fn remote_forward_socket_path(dir: &Path, connection_id: &str) -> PathBuf {
    let hash = hex::encode(Sha256::digest(connection_id.as_bytes()));
    let unique = uuid::Uuid::new_v4().simple().to_string();
    dir.join(format!("r-{}-{}.sock", &hash[..12], &unique[..8]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::{current_uid, local_socket_path, node_tmpdir, process_env};

    mod get_default_socket_path {
        use super::*;

        #[test]
        fn returns_path_containing_daemon_sock() {
            let env = |key: &str| match key {
                "TMPDIR" => Some("/var/tmp".to_owned()),
                _ => None,
            };
            assert!(local_socket_path(&env, 501).ends_with("daemon.sock"));
        }

        #[test]
        fn returns_path_under_tmpdir_with_treeterm_prefix() {
            let uid = current_uid();
            let env = |key: &str| match key {
                crate::paths::SOCKET_PATH_ENV => None,
                other => process_env(other),
            };
            assert_eq!(
                local_socket_path(&env, uid),
                node_tmpdir(&process_env)
                    .join(format!("treeterm-{uid}"))
                    .join("daemon.sock")
            );
        }
    }

    mod get_remote_forward_socket_path {
        use super::*;

        fn path_for(connection_id: &str) -> String {
            remote_forward_socket_path(&forward_socket_dir(current_uid()), connection_id)
                .to_string_lossy()
                .into_owned()
        }

        #[test]
        fn returns_path_with_r_prefix_hash_and_unique_suffix() {
            let path = path_for("test-connection");
            let pattern = regex::Regex::new(r"/r-[a-f0-9]{12}-[a-f0-9]{8}\.sock$").unwrap();
            assert!(pattern.is_match(&path), "{path}");
        }

        #[test]
        fn returns_path_under_tmp_with_treeterm_prefix() {
            let path = path_for("test-connection");
            assert!(
                path.contains(&format!("/tmp/treeterm-{}/", current_uid())),
                "{path}"
            );
        }

        #[test]
        fn produces_different_paths_for_different_connection_ids() {
            assert_ne!(path_for("connection-1"), path_for("connection-2"));
        }

        #[test]
        fn produces_unique_paths_for_the_same_connection_id_no_collision_between_windows() {
            assert_ne!(path_for("my-connection"), path_for("my-connection"));
        }

        #[test]
        fn hashes_the_connection_id_like_electron() {
            let expected = hex::encode(Sha256::digest(b"test-connection"));
            assert!(path_for("test-connection").contains(&format!("/r-{}-", &expected[..12])));
        }
    }
}
