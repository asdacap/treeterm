use std::path::PathBuf;

pub fn data_dir() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
        .join(".treeterm")
}

/// Directory holding the per-workspace JSON files (one `<workspace-id>.json` each).
/// Advertised to clients via `Session.workspace_data_dir`.
pub fn workspaces_dir() -> PathBuf {
    data_dir().join("workspaces")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspaces_dir_is_under_data_dir() {
        assert_eq!(workspaces_dir(), data_dir().join("workspaces"));
        assert!(data_dir().ends_with(".treeterm"));
    }
}
