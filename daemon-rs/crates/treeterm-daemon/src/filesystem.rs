use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tokio::fs;
use tokio::sync::Mutex;
use treeterm_proto::treeterm::*;

pub const MAX_FILE_SIZE: u64 = 64 * 1024; // 64KB

/// Serializes all writes so the compare-and-swap check and the rename are atomic
/// with respect to other writers going through this daemon.
fn write_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// Resolve a request's `file_path` against its `workspace_path` scope. Used by the
/// file ops below and by the gRPC layer's file-watch intercepts, so both always
/// refer to the same target path.
pub fn resolve_target(workspace_path: &Path, file_path: &str) -> PathBuf {
    workspace_path.join(file_path)
}

/// Security: Ensure path is within workspace (resolves symlinks to prevent escape)
pub async fn is_path_within_workspace(workspace_path: &Path, target_path: &Path) -> bool {
    // Try resolving symlinks first
    if let (Ok(ws), Ok(tgt)) = (fs::canonicalize(workspace_path).await, fs::canonicalize(target_path).await) {
        return tgt.starts_with(&ws);
    }
    // Fall back to logical resolution if target doesn't exist yet. Re-anchor the
    // target onto the canonicalized workspace (the raw prefix may go through a
    // symlink, e.g. /var → /private/var on macOS) and resolve `..`/`.` lexically —
    // `starts_with` compares raw components, so an unresolved `..` would escape.
    let ws = workspace_path.canonicalize().unwrap_or_else(|_| workspace_path.to_path_buf());
    let rel = target_path.strip_prefix(workspace_path).unwrap_or(target_path);
    let tgt = if rel.is_absolute() {
        rel.to_path_buf()
    } else {
        ws.join(rel)
    };
    let mut normalized = PathBuf::new();
    for component in tgt.components() {
        match component {
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            std::path::Component::CurDir => {}
            other => normalized.push(other),
        }
    }
    normalized.starts_with(&ws)
}

/// Detect language from file extension for syntax highlighting
pub fn detect_language(file_path: &Path) -> &'static str {
    let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
    match ext.to_lowercase().as_str() {
        "ts" | "tsx" => "typescript",
        "js" | "jsx" => "javascript",
        "json" => "json",
        "md" => "markdown",
        "css" => "css",
        "scss" => "scss",
        "less" => "less",
        "html" | "htm" | "vue" | "svelte" => "html",
        "xml" => "xml",
        "py" => "python",
        "rs" => "rust",
        "go" => "go",
        "java" => "java",
        "c" | "h" => "c",
        "cpp" | "hpp" => "cpp",
        "yaml" | "yml" => "yaml",
        "toml" => "toml",
        "sh" | "bash" | "zsh" => "bash",
        "sql" => "sql",
        "graphql" | "gql" => "graphql",
        "rb" => "ruby",
        "php" => "php",
        "swift" => "swift",
        "kt" | "kts" => "kotlin",
        "scala" => "scala",
        "r" => "r",
        "lua" => "lua",
        "dockerfile" => "dockerfile",
        _ => "plaintext",
    }
}

fn file_entry_from(name: String, full_path: &Path, relative_path: String, is_dir: bool, meta: Option<&std::fs::Metadata>) -> FileEntry {
    FileEntry {
        name,
        path: full_path.to_string_lossy().into_owned(),
        relative_path,
        is_directory: is_dir,
        size: meta.map(|m| m.len() as i64),
        modified_time: meta.and_then(|m| {
            m.modified().ok().and_then(|t| {
                t.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs_f64() * 1000.0)
            })
        }),
    }
}

fn sort_entries(entries: &mut [FileEntry]) {
    entries.sort_by(|a, b| {
        b.is_directory.cmp(&a.is_directory).then_with(|| a.name.cmp(&b.name))
    });
}

pub async fn read_directory(workspace_path: &Path, dir_path: &str) -> ReadDirectoryResponse {
    let resolved_dir = workspace_path.join(dir_path);

    if !is_path_within_workspace(workspace_path, &resolved_dir).await {
        return ReadDirectoryResponse { success: false, contents: None, error: Some("Access denied: Path outside workspace".into()) };
    }

    let read_result = match fs::read_dir(&resolved_dir).await {
        Ok(r) => r,
        Err(e) => return ReadDirectoryResponse { success: false, contents: None, error: Some(e.to_string()) },
    };

    let mut rd = read_result;
    let mut entries = Vec::new();

    while let Ok(Some(entry)) = rd.next_entry().await {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let full_path = entry.path();
        let relative_path = full_path.strip_prefix(workspace_path).unwrap_or(&full_path).to_string_lossy().into_owned();
        let ft = entry.file_type().await.ok();
        let is_dir = ft.map_or(false, |f| f.is_dir());
        let meta = fs::metadata(&full_path).await.ok();
        let std_meta = meta.as_ref().map(|_| std::fs::metadata(&full_path).ok()).flatten();
        // Use tokio metadata converted: just re-read with std for the Metadata type
        let meta_ref = std_meta.as_ref();
        entries.push(file_entry_from(name, &full_path, relative_path, is_dir, meta_ref));
    }

    sort_entries(&mut entries);

    ReadDirectoryResponse {
        success: true,
        contents: Some(DirectoryContents {
            path: resolved_dir.to_string_lossy().into_owned(),
            entries,
        }),
        error: None,
    }
}

pub async fn read_file_streaming(workspace_path: &Path, file_path: &str) -> Result<(FileReadHeader, Vec<u8>), String> {
    let resolved = resolve_target(workspace_path, file_path);

    if !is_path_within_workspace(workspace_path, &resolved).await {
        return Err("Access denied: Path outside workspace".into());
    }

    let meta = fs::metadata(&resolved).await.map_err(|e| e.to_string())?;
    if meta.len() > MAX_FILE_SIZE {
        return Err("File too large to preview (max 64KB)".into());
    }

    let content = fs::read(&resolved).await.map_err(|e| e.to_string())?;
    let language = detect_language(&resolved);

    let header = FileReadHeader {
        path: resolved.to_string_lossy().into_owned(),
        size: meta.len() as i64,
        language: language.into(),
    };

    Ok((header, content))
}

/// `expected_sha256` is a compare-and-swap guard: `None` writes unconditionally,
/// `Some("")` requires the file to not exist, otherwise the lowercase-hex SHA-256
/// of the current file bytes must match or the write is rejected with `conflict`.
pub async fn write_file_streaming(
    workspace_path: &Path,
    file_path: &str,
    content: &[u8],
    expected_sha256: Option<String>,
) -> WriteFileResponse {
    let resolved = resolve_target(workspace_path, file_path);

    if !is_path_within_workspace(workspace_path, &resolved).await {
        return WriteFileResponse { success: false, error: Some("Access denied: Path outside workspace".into()), conflict: false };
    }

    if let Some(parent) = resolved.parent() {
        if let Err(e) = fs::create_dir_all(parent).await {
            return WriteFileResponse { success: false, error: Some(e.to_string()), conflict: false };
        }
    }

    // Hold the write lock across CAS check + write so concurrent writers serialize.
    let _guard = write_lock().lock().await;

    if let Some(expected) = expected_sha256 {
        let current = match fs::read(&resolved).await {
            Ok(bytes) => Some(bytes),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return WriteFileResponse { success: false, error: Some(e.to_string()), conflict: false },
        };
        let matches = match (&current, expected.as_str()) {
            (None, "") => true,
            (Some(bytes), exp) if !exp.is_empty() => sha256_hex(bytes) == exp,
            _ => false,
        };
        if !matches {
            return WriteFileResponse {
                success: false,
                error: Some("write conflict: file changed since read".into()),
                conflict: true,
            };
        }
    }

    // Atomic write: temp file in the same directory, then rename over the target,
    // so a concurrent reader never observes a torn/partial file.
    let tmp_name = format!(
        ".{}.tmp-{}",
        resolved.file_name().and_then(|n| n.to_str()).unwrap_or("file"),
        uuid::Uuid::new_v4()
    );
    let tmp_path = resolved.with_file_name(tmp_name);
    if let Err(e) = fs::write(&tmp_path, &content).await {
        return WriteFileResponse { success: false, error: Some(e.to_string()), conflict: false };
    }
    match fs::rename(&tmp_path, &resolved).await {
        Ok(_) => WriteFileResponse { success: true, error: None, conflict: false },
        Err(e) => {
            let _ = fs::remove_file(&tmp_path).await;
            WriteFileResponse { success: false, error: Some(e.to_string()), conflict: false }
        }
    }
}

/// Delete a file. Idempotent: deleting a file that does not exist succeeds.
pub async fn delete_file(workspace_path: &Path, file_path: &str) -> DeleteFileResponse {
    let resolved = resolve_target(workspace_path, file_path);

    if !is_path_within_workspace(workspace_path, &resolved).await {
        return DeleteFileResponse { success: false, error: Some("Access denied: Path outside workspace".into()) };
    }

    // Serialize against writers so a delete cannot race a CAS write's check+rename.
    let _guard = write_lock().lock().await;

    match fs::remove_file(&resolved).await {
        Ok(()) => DeleteFileResponse { success: true, error: None },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => DeleteFileResponse { success: true, error: None },
        Err(e) => DeleteFileResponse { success: false, error: Some(e.to_string()) },
    }
}

pub async fn search_files(workspace_path: &Path, query: &str) -> SearchFilesResponse {
    let normalized = query.to_lowercase();
    let normalized = normalized.trim();
    if normalized.is_empty() {
        return SearchFilesResponse { success: true, entries: vec![], error: None };
    }

    let mut results = Vec::new();
    if let Err(e) = walk_dir_search(workspace_path, workspace_path, normalized, &mut results).await {
        return SearchFilesResponse { success: false, entries: vec![], error: Some(e) };
    }

    sort_entries(&mut results);

    SearchFilesResponse { success: true, entries: results, error: None }
}

async fn walk_dir_search(workspace_path: &Path, dir: &Path, query: &str, results: &mut Vec<FileEntry>) -> Result<(), String> {
    let mut rd = fs::read_dir(dir).await.map_err(|e| e.to_string())?;

    while let Ok(Some(entry)) = rd.next_entry().await {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || name == "node_modules" {
            continue;
        }

        let full_path = entry.path();
        let relative_path = full_path.strip_prefix(workspace_path).unwrap_or(&full_path).to_string_lossy().into_owned();
        let ft = entry.file_type().await.ok();
        let is_dir = ft.map_or(false, |f| f.is_dir());

        if name.to_lowercase().contains(query) {
            let std_meta = std::fs::metadata(&full_path).ok();
            results.push(file_entry_from(name.clone(), &full_path, relative_path, is_dir, std_meta.as_ref()));
        }

        if is_dir {
            // Ignore errors in subdirectories (permission denied, etc.)
            let _ = Box::pin(walk_dir_search(workspace_path, &full_path, query, results)).await;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;
    use tokio::fs;

    // -- detect_language --

    #[test]
    fn detect_language_known_extensions() {
        assert_eq!(detect_language(Path::new("foo.ts")), "typescript");
        assert_eq!(detect_language(Path::new("foo.tsx")), "typescript");
        assert_eq!(detect_language(Path::new("foo.js")), "javascript");
        assert_eq!(detect_language(Path::new("foo.rs")), "rust");
        assert_eq!(detect_language(Path::new("foo.py")), "python");
        assert_eq!(detect_language(Path::new("foo.go")), "go");
        assert_eq!(detect_language(Path::new("foo.json")), "json");
        assert_eq!(detect_language(Path::new("foo.yaml")), "yaml");
        assert_eq!(detect_language(Path::new("foo.yml")), "yaml");
        assert_eq!(detect_language(Path::new("foo.html")), "html");
        assert_eq!(detect_language(Path::new("foo.css")), "css");
        assert_eq!(detect_language(Path::new("foo.toml")), "toml");
        assert_eq!(detect_language(Path::new("foo.sh")), "bash");
        assert_eq!(detect_language(Path::new("foo.sql")), "sql");
    }

    #[test]
    fn detect_language_unknown_extension() {
        assert_eq!(detect_language(Path::new("foo.xyz")), "plaintext");
        assert_eq!(detect_language(Path::new("noext")), "plaintext");
    }

    // -- sort_entries --

    #[test]
    fn sort_entries_dirs_first_then_alphabetical() {
        let mut entries = vec![
            FileEntry { name: "z_file".into(), is_directory: false, ..Default::default() },
            FileEntry { name: "a_dir".into(), is_directory: true, ..Default::default() },
            FileEntry { name: "a_file".into(), is_directory: false, ..Default::default() },
            FileEntry { name: "b_dir".into(), is_directory: true, ..Default::default() },
        ];
        sort_entries(&mut entries);

        assert_eq!(entries[0].name, "a_dir");
        assert_eq!(entries[1].name, "b_dir");
        assert_eq!(entries[2].name, "a_file");
        assert_eq!(entries[3].name, "z_file");
    }

    // -- is_path_within_workspace --

    #[tokio::test]
    async fn path_within_workspace_valid() {
        let tmp = TempDir::new().unwrap();
        let sub = tmp.path().join("sub");
        fs::create_dir(&sub).await.unwrap();

        assert!(is_path_within_workspace(tmp.path(), &sub).await);
    }

    #[tokio::test]
    async fn path_outside_workspace_rejected() {
        let tmp1 = TempDir::new().unwrap();
        let tmp2 = TempDir::new().unwrap();

        assert!(!is_path_within_workspace(tmp1.path(), tmp2.path()).await);
    }

    #[tokio::test]
    async fn path_traversal_rejected() {
        let tmp = TempDir::new().unwrap();
        let escape = tmp.path().join("../../../etc/passwd");

        assert!(!is_path_within_workspace(tmp.path(), &escape).await);
    }

    #[tokio::test]
    async fn path_traversal_to_nonexistent_target_rejected() {
        let tmp = TempDir::new().unwrap();
        // Target does not exist, so canonicalize fails and the lexical fallback
        // must still resolve the `..` instead of comparing raw components.
        let escape = tmp.path().join("../escape-does-not-exist.json");

        assert!(!is_path_within_workspace(tmp.path(), &escape).await);
    }

    #[tokio::test]
    async fn path_nonexistent_target_under_symlinked_workspace_allowed() {
        let tmp = TempDir::new().unwrap();
        // Raw (non-canonicalized) workspace path: on macOS this goes through the
        // /var -> /private/var symlink while the canonical workspace does not.
        let target = tmp.path().join("new-file.json");

        assert!(is_path_within_workspace(tmp.path(), &target).await);
    }

    #[tokio::test]
    async fn path_nonexistent_target_within_workspace() {
        let tmp = TempDir::new().unwrap();
        // Canonicalize to resolve macOS /var -> /private/var symlink
        let ws = tmp.path().canonicalize().unwrap();
        let target = ws.join("does-not-exist.txt");

        assert!(is_path_within_workspace(&ws, &target).await);
    }

    // -- read_directory --

    #[tokio::test]
    async fn read_directory_lists_entries() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("hello.txt"), "hi").await.unwrap();
        fs::create_dir(tmp.path().join("subdir")).await.unwrap();

        let resp = read_directory(tmp.path(), ".").await;
        assert!(resp.success);
        let contents = resp.contents.unwrap();
        assert_eq!(contents.entries.len(), 2);
        // dirs first
        assert!(contents.entries[0].is_directory);
        assert_eq!(contents.entries[0].name, "subdir");
        assert_eq!(contents.entries[1].name, "hello.txt");
    }

    #[tokio::test]
    async fn read_directory_skips_dotfiles() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(".hidden"), "").await.unwrap();
        fs::write(tmp.path().join("visible"), "").await.unwrap();

        let resp = read_directory(tmp.path(), ".").await;
        assert!(resp.success);
        let entries = &resp.contents.unwrap().entries;
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "visible");
    }

    #[tokio::test]
    async fn read_directory_outside_workspace_denied() {
        let tmp1 = TempDir::new().unwrap();
        let tmp2 = TempDir::new().unwrap();

        let resp = read_directory(tmp1.path(), tmp2.path().to_str().unwrap()).await;
        assert!(!resp.success);
        assert!(resp.error.unwrap().contains("Access denied"));
    }

    #[tokio::test]
    async fn read_directory_nonexistent_errors() {
        let tmp = TempDir::new().unwrap();
        let resp = read_directory(tmp.path(), "nope").await;
        assert!(!resp.success);
        assert!(resp.error.is_some());
    }

    // -- read_file_streaming --

    #[tokio::test]
    async fn read_file_streaming_success() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("test.rs"), "fn main() {}").await.unwrap();

        let result = read_file_streaming(tmp.path(), "test.rs").await;
        assert!(result.is_ok());
        let (header, content) = result.unwrap();
        assert_eq!(header.language, "rust");
        assert_eq!(content, b"fn main() {}");
    }

    #[tokio::test]
    async fn read_file_streaming_too_large() {
        let tmp = TempDir::new().unwrap();
        let big = vec![0u8; (MAX_FILE_SIZE + 1) as usize];
        fs::write(tmp.path().join("big.bin"), &big).await.unwrap();

        let result = read_file_streaming(tmp.path(), "big.bin").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("too large"));
    }

    #[tokio::test]
    async fn read_file_outside_workspace_denied() {
        let tmp1 = TempDir::new().unwrap();
        let tmp2 = TempDir::new().unwrap();
        fs::write(tmp2.path().join("secret.txt"), "secret").await.unwrap();

        let abs = tmp2.path().join("secret.txt");
        let result = read_file_streaming(tmp1.path(), abs.to_str().unwrap()).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Access denied"));
    }

    // -- write_file_streaming --

    #[tokio::test]
    async fn write_file_streaming_creates_file() {
        let tmp = TempDir::new().unwrap();
        let ws = tmp.path().canonicalize().unwrap();
        let resp = write_file_streaming(&ws, "new.txt", b"hello", None).await;
        assert!(resp.success);

        let content = fs::read_to_string(ws.join("new.txt")).await.unwrap();
        assert_eq!(content, "hello");
    }

    #[tokio::test]
    async fn write_file_streaming_creates_parent_dirs() {
        let tmp = TempDir::new().unwrap();
        let ws = tmp.path().canonicalize().unwrap();
        let resp = write_file_streaming(&ws, "a/b/c.txt", b"nested", None).await;
        assert!(resp.success);

        let content = fs::read_to_string(ws.join("a/b/c.txt")).await.unwrap();
        assert_eq!(content, "nested");
    }

    #[tokio::test]
    async fn write_file_outside_workspace_denied() {
        let tmp1 = TempDir::new().unwrap();
        let tmp2 = TempDir::new().unwrap();
        let target = tmp2.path().join("evil.txt");

        let resp = write_file_streaming(tmp1.path(), target.to_str().unwrap(), b"hack", None).await;
        assert!(!resp.success);
        assert!(resp.error.unwrap().contains("Access denied"));
    }

    #[tokio::test]
    async fn write_file_cas_matching_hash_succeeds() {
        let tmp = TempDir::new().unwrap();
        let ws = tmp.path().canonicalize().unwrap();
        fs::write(ws.join("f.txt"), b"old").await.unwrap();

        let expected = sha256_hex(b"old");
        let resp = write_file_streaming(&ws, "f.txt", b"new", Some(expected)).await;
        assert!(resp.success);
        assert!(!resp.conflict);
        assert_eq!(fs::read_to_string(ws.join("f.txt")).await.unwrap(), "new");
    }

    #[tokio::test]
    async fn write_file_cas_mismatched_hash_conflicts() {
        let tmp = TempDir::new().unwrap();
        let ws = tmp.path().canonicalize().unwrap();
        fs::write(ws.join("f.txt"), b"changed by someone else").await.unwrap();

        let stale = sha256_hex(b"old");
        let resp = write_file_streaming(&ws, "f.txt", b"new", Some(stale)).await;
        assert!(!resp.success);
        assert!(resp.conflict);
        // The file is untouched on conflict.
        assert_eq!(fs::read_to_string(ws.join("f.txt")).await.unwrap(), "changed by someone else");
    }

    #[tokio::test]
    async fn write_file_cas_empty_expected_requires_absent() {
        let tmp = TempDir::new().unwrap();
        let ws = tmp.path().canonicalize().unwrap();

        // Absent → create succeeds.
        let resp = write_file_streaming(&ws, "f.txt", b"first", Some(String::new())).await;
        assert!(resp.success);

        // Present → conflict.
        let resp = write_file_streaming(&ws, "f.txt", b"second", Some(String::new())).await;
        assert!(!resp.success);
        assert!(resp.conflict);
        assert_eq!(fs::read_to_string(ws.join("f.txt")).await.unwrap(), "first");
    }

    #[tokio::test]
    async fn write_file_cas_expected_content_but_file_missing_conflicts() {
        let tmp = TempDir::new().unwrap();
        let ws = tmp.path().canonicalize().unwrap();

        let resp = write_file_streaming(&ws, "gone.txt", b"new", Some(sha256_hex(b"old"))).await;
        assert!(!resp.success);
        assert!(resp.conflict);
    }

    #[tokio::test]
    async fn write_file_leaves_no_temp_files() {
        let tmp = TempDir::new().unwrap();
        let ws = tmp.path().canonicalize().unwrap();
        let resp = write_file_streaming(&ws, "f.txt", b"data", None).await;
        assert!(resp.success);

        let mut entries = fs::read_dir(&ws).await.unwrap();
        let mut names = Vec::new();
        while let Some(e) = entries.next_entry().await.unwrap() {
            names.push(e.file_name().to_string_lossy().into_owned());
        }
        assert_eq!(names, vec!["f.txt"]);
    }

    // -- search_files --

    #[tokio::test]
    async fn search_files_finds_matching() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("foo.txt"), "").await.unwrap();
        fs::write(tmp.path().join("bar.txt"), "").await.unwrap();
        fs::write(tmp.path().join("foobar.rs"), "").await.unwrap();

        let resp = search_files(tmp.path(), "foo").await;
        assert!(resp.success);
        assert_eq!(resp.entries.len(), 2);
        let names: Vec<&str> = resp.entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"foo.txt"));
        assert!(names.contains(&"foobar.rs"));
    }

    #[tokio::test]
    async fn search_files_case_insensitive() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("README.md"), "").await.unwrap();

        let resp = search_files(tmp.path(), "readme").await;
        assert!(resp.success);
        assert_eq!(resp.entries.len(), 1);
    }

    #[tokio::test]
    async fn search_files_empty_query_returns_empty() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("file.txt"), "").await.unwrap();

        let resp = search_files(tmp.path(), "  ").await;
        assert!(resp.success);
        assert_eq!(resp.entries.len(), 0);
    }

    #[tokio::test]
    async fn search_files_skips_dotfiles_and_node_modules() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(".hidden_match"), "").await.unwrap();
        let nm = tmp.path().join("node_modules");
        fs::create_dir(&nm).await.unwrap();
        fs::write(nm.join("match.js"), "").await.unwrap();
        fs::write(tmp.path().join("match.txt"), "").await.unwrap();

        let resp = search_files(tmp.path(), "match").await;
        assert!(resp.success);
        assert_eq!(resp.entries.len(), 1);
        assert_eq!(resp.entries[0].name, "match.txt");
    }

    #[tokio::test]
    async fn search_files_recurses_into_subdirs() {
        let tmp = TempDir::new().unwrap();
        let sub = tmp.path().join("deep");
        fs::create_dir(&sub).await.unwrap();
        fs::write(sub.join("target.txt"), "").await.unwrap();

        let resp = search_files(tmp.path(), "target").await;
        assert!(resp.success);
        assert_eq!(resp.entries.len(), 1);
    }

    // -- file_entry_from --

    #[test]
    fn file_entry_from_without_metadata() {
        let entry = file_entry_from(
            "test.txt".into(),
            &PathBuf::from("/a/test.txt"),
            "test.txt".into(),
            false,
            None,
        );
        assert_eq!(entry.name, "test.txt");
        assert_eq!(entry.path, "/a/test.txt");
        assert!(!entry.is_directory);
        assert!(entry.size.is_none());
        assert!(entry.modified_time.is_none());
    }
}
