use std::path::Path;
use tokio::fs;
use treeterm_proto::treeterm::*;

const MAX_FILE_SIZE: u64 = 1024 * 1024; // 1MB

/// Security: Ensure path is within workspace (resolves symlinks to prevent escape)
pub async fn is_path_within_workspace(workspace_path: &Path, target_path: &Path) -> bool {
    // Try resolving symlinks first
    if let (Ok(ws), Ok(tgt)) = (fs::canonicalize(workspace_path).await, fs::canonicalize(target_path).await) {
        return tgt.starts_with(&ws);
    }
    // Fall back to logical resolution if target doesn't exist yet
    let ws = workspace_path.canonicalize().unwrap_or_else(|_| workspace_path.to_path_buf());
    let tgt = if target_path.is_absolute() {
        target_path.to_path_buf()
    } else {
        workspace_path.join(target_path)
    };
    tgt.starts_with(&ws)
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
    let resolved = workspace_path.join(file_path);

    if !is_path_within_workspace(workspace_path, &resolved).await {
        return Err("Access denied: Path outside workspace".into());
    }

    let meta = fs::metadata(&resolved).await.map_err(|e| e.to_string())?;
    if meta.len() > MAX_FILE_SIZE {
        return Err("File too large to preview (max 1MB)".into());
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

pub async fn write_file_streaming(workspace_path: &Path, file_path: &str, content: Vec<u8>) -> WriteFileResponse {
    let resolved = workspace_path.join(file_path);

    if !is_path_within_workspace(workspace_path, &resolved).await {
        return WriteFileResponse { success: false, error: Some("Access denied: Path outside workspace".into()) };
    }

    if let Some(parent) = resolved.parent() {
        if let Err(e) = fs::create_dir_all(parent).await {
            return WriteFileResponse { success: false, error: Some(e.to_string()) };
        }
    }

    match fs::write(&resolved, &content).await {
        Ok(_) => WriteFileResponse { success: true, error: None },
        Err(e) => WriteFileResponse { success: false, error: Some(e.to_string()) },
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
