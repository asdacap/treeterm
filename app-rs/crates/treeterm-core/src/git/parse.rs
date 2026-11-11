//! Parsing of git's porcelain output, and the error and content classification that goes
//! with it. Pure functions; the git invocations live in the parent module.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use indexmap::IndexMap;
use regex::Regex;

use super::error::{CommandError, CommandFailure, GitError};
use super::types::{FileChangeStatus, GitStatusEntry, PathOrigin, Staging};

/// A finished git (or helper) command, decoded as UTF-8.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExecResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub args: Vec<String>,
}

/// `git status --porcelain` → one entry per changed side of each path.
pub fn parse_status(output: &str) -> Vec<GitStatusEntry> {
    let mut entries = Vec::new();
    for line in output.split('\n').filter(|line| !line.is_empty()) {
        let mut chars = line.chars();
        let (Some(staged_char), Some(unstaged_char)) = (chars.next(), chars.next()) else {
            continue;
        };
        if chars.as_str().is_empty() {
            continue;
        }
        let after_status = chars.as_str().trim();

        // Renames: "R  old.txt -> new.txt"
        let (path, origin) = match (staged_char == 'R' || unstaged_char == 'R')
            .then(|| after_status.split_once(" -> "))
            .flatten()
        {
            Some((original, path)) => (path, PathOrigin::RenamedFrom(original.to_owned())),
            None => (after_status, PathOrigin::Same),
        };
        let entry = |status, staged| GitStatusEntry {
            path: path.to_owned(),
            status,
            staged,
            origin: origin.clone(),
        };

        if staged_char == '?' && unstaged_char == '?' {
            entries.push(entry(FileChangeStatus::Untracked, Staging::Unstaged));
            continue;
        }
        if staged_char != ' ' && staged_char != '?' {
            entries.push(entry(status_from_char(staged_char), Staging::Staged));
        }
        if unstaged_char != ' ' && unstaged_char != '?' {
            entries.push(entry(status_from_char(unstaged_char), Staging::Unstaged));
        }
    }
    entries
}

fn status_from_char(c: char) -> FileChangeStatus {
    match c {
        'A' => FileChangeStatus::Added,
        'D' => FileChangeStatus::Deleted,
        'R' => FileChangeStatus::Renamed,
        _ => FileChangeStatus::Modified,
    }
}

/// One `--numstat` line's counts. Git's `-\t-` binary marker is kept until the actual byte
/// sizes are looked up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ParsedNumstat {
    Text { additions: u64, deletions: u64 },
    Binary,
}

pub(crate) fn parse_numstat_counts(line: &str) -> Result<ParsedNumstat, GitError> {
    let mut columns = line.split('\t');
    let (add, del) = (columns.next().unwrap_or(""), columns.next().unwrap_or(""));
    if add == "-" && del == "-" {
        return Ok(ParsedNumstat::Binary);
    }
    let invalid = || GitError::InvalidNumstat {
        line: line.to_owned(),
    };
    Ok(ParsedNumstat::Text {
        additions: parse_count(add, invalid)?,
        deletions: parse_count(del, invalid)?,
    })
}

/// `/^\d+$/` then `Number(...)`.
fn parse_count(text: &str, invalid: impl Fn() -> GitError) -> Result<u64, GitError> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return Err(invalid());
    }
    text.parse().map_err(|_overflow| invalid())
}

static NUMSTAT_NESTED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(.*)\{[^{}]* => ([^{}]*)\}(.*)$").expect("valid regex"));

/// `--numstat` writes a rename as a compact `old => new` path; this is the new path.
pub(crate) fn numstat_path(path: &str) -> String {
    if let Some(nested) = NUMSTAT_NESTED.captures(path) {
        return format!("{}{}{}", &nested[1], &nested[2], &nested[3]);
    }
    match path.find(" => ") {
        Some(arrow) => path[arrow + 4..].to_owned(),
        None => path.to_owned(),
    }
}

/// `--numstat` output keyed by path, in output order. A path in `known_paths` is taken
/// literally (it may contain ` => `); any other is read as a compact rename.
pub(crate) fn parse_numstat(
    stdout: &str,
    known_paths: &HashSet<&str>,
) -> Result<IndexMap<String, ParsedNumstat>, GitError> {
    let mut stats = IndexMap::new();
    for line in stdout.trim().split('\n').filter(|line| !line.is_empty()) {
        let path = line.split('\t').skip(2).collect::<Vec<_>>().join("\t");
        if path.is_empty() {
            return Err(GitError::NumstatWithoutPath {
                line: line.to_owned(),
            });
        }
        let key = if known_paths.contains(path.as_str()) {
            path
        } else {
            numstat_path(&path)
        };
        stats.insert(key, parse_numstat_counts(line)?);
    }
    Ok(stats)
}

static BIN_LINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\|\s*Bin\s").expect("valid regex"));
static BIN_SIZES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\|\s*Bin\s+([0-9]+)\s+->\s+([0-9]+)\s+bytes\s*$").expect("valid regex")
});

/// The net byte change from `--stat` output holding exactly one binary line, without
/// interpreting the filename.
pub(crate) fn parse_binary_byte_change(stdout: &str, path: &str) -> Result<i64, GitError> {
    let bin_lines: Vec<&str> = stdout
        .split('\n')
        .filter(|line| BIN_LINE.is_match(line))
        .collect();
    let [line] = bin_lines[..] else {
        return Err(GitError::BinarySizesMissing {
            path: path.to_owned(),
        });
    };
    let invalid = || GitError::BinarySizesInvalid {
        path: path.to_owned(),
    };
    let sizes = BIN_SIZES.captures(line).ok_or_else(invalid)?;
    // JS numbers are exact up to 2^53 - 1; anything bigger was rejected there too.
    const MAX_SAFE_INTEGER: i64 = (1 << 53) - 1;
    let size = |index: usize| match sizes[index].parse::<i64>() {
        Ok(size) if size <= MAX_SAFE_INTEGER => Ok(size),
        _ => Err(invalid()),
    };
    let (old_size, new_size) = (size(1)?, size(2)?);
    Ok(new_size - old_size)
}

/// One path of `--name-status` output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NameStatus {
    pub status: FileChangeStatus,
    pub origin: PathOrigin,
}

/// `--name-status` output keyed by the (new) path.
pub(crate) fn parse_name_statuses(stdout: &str) -> HashMap<String, NameStatus> {
    let mut statuses = HashMap::new();
    for line in stdout.trim().split('\n').filter(|line| !line.is_empty()) {
        let mut fields = line.split('\t');
        let change = fields.next().unwrap_or("");
        let paths: Vec<&str> = fields.collect();
        let Some(path) = paths.last().copied() else {
            continue;
        };
        if path.is_empty() || change.is_empty() {
            continue;
        }
        let status = if change.starts_with('A') {
            FileChangeStatus::Added
        } else if change.starts_with('D') {
            FileChangeStatus::Deleted
        } else if change.starts_with('R') {
            FileChangeStatus::Renamed
        } else {
            FileChangeStatus::Modified
        };
        let origin = if change.starts_with('R') && paths.len() > 1 {
            PathOrigin::RenamedFrom(paths[0].to_owned())
        } else {
            PathOrigin::Same
        };
        statuses.insert(path.to_owned(), NameStatus { status, origin });
    }
    statuses
}

static BLAME_HEADER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([0-9a-f]{40}) [0-9]+ ([0-9]+)").expect("valid regex"));

/// Map each final line of `git blame --porcelain` output to its commit hash: index `i`
/// holds the commit that last changed line `i + 1`.
pub fn parse_blame_porcelain(stdout: &str) -> Vec<String> {
    let mut line_commits: Vec<String> = Vec::new();
    for line in stdout.split('\n') {
        let Some(header) = BLAME_HEADER.captures(line) else {
            continue;
        };
        // Line numbers start at 1; anything else is not a line of the file.
        let index = match header[2].parse::<usize>() {
            Ok(final_line) if final_line > 0 => final_line - 1,
            _ => continue,
        };
        if line_commits.len() <= index {
            line_commits.resize(index + 1, String::new());
        }
        line_commits[index] = header[1].to_owned();
    }
    line_commits
}

/// Classify a failed command by its stderr.
pub(crate) fn interpret_error(result: &ExecResult) -> CommandError {
    let stderr = result.stderr.to_lowercase();
    let failure = if stderr.contains("nothing to commit") {
        CommandFailure::NothingToCommit
    } else if stderr.contains("merge conflict") {
        CommandFailure::MergeConflict
    } else if stderr.contains("already exists") {
        CommandFailure::AlreadyExists
    } else if stderr.contains("not a git repository") {
        CommandFailure::NotARepository
    } else if stderr.contains("pathspec") && stderr.contains("did not match") {
        CommandFailure::FileNotFound
    } else if stderr.contains("failed to merge") {
        CommandFailure::MergeFailed
    } else if stderr.contains("could not resolve") {
        CommandFailure::CouldNotResolve
    } else if result.exit_code != 0 && !result.stderr.is_empty() {
        CommandFailure::Stderr(result.stderr.clone())
    } else {
        CommandFailure::ExitCode(result.exit_code)
    };
    CommandError {
        command: format!("git {}", result.args.join(" ")),
        failure,
    }
}

/// The last `.`-separated segment of a path (the whole path when it has no `.`).
fn extension_of(file_path: &str) -> String {
    file_path
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_lowercase()
}

/// Syntax-highlighting language for a file extension.
pub(crate) fn detect_language(file_path: &str) -> &'static str {
    match extension_of(file_path).as_str() {
        "ts" | "tsx" => "typescript",
        "js" | "jsx" => "javascript",
        "py" => "python",
        "rs" => "rust",
        "go" => "go",
        "java" => "java",
        "cpp" | "hpp" => "cpp",
        "c" | "h" => "c",
        "md" => "markdown",
        "json" => "json",
        "yaml" | "yml" => "yaml",
        "toml" => "toml",
        "proto" => "protobuf",
        "sh" | "bash" | "zsh" => "shell",
        "html" => "html",
        "css" => "css",
        "scss" => "scss",
        "less" => "less",
        "sql" => "sql",
        "rb" => "ruby",
        "php" => "php",
        "swift" => "swift",
        "kt" => "kotlin",
        "scala" => "scala",
        "r" => "r",
        "m" => "objective-c",
        "mm" => "objective-cpp",
        "vue" => "vue",
        "svelte" => "svelte",
        _ => "plaintext",
    }
}

/// Images are recognised by extension (`shared/fileContentKind.ts`).
pub(crate) fn is_image_path(file_path: &str) -> bool {
    matches!(
        extension_of(file_path).as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico" | "avif" | "svg"
    )
}

/// Git's own heuristic: a NUL near the start marks the content binary.
const BINARY_SNIFF_LENGTH: usize = 8000;

pub(crate) fn is_binary_content(content: &str) -> bool {
    content.chars().take(BINARY_SNIFF_LENGTH).any(|c| c == '\0')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numstat_paths_resolve_compact_renames() {
        assert_eq!(numstat_path("dir/{old => new}.png"), "dir/new.png");
        assert_eq!(numstat_path("{a => b}/file.ts"), "b/file.ts");
        assert_eq!(numstat_path("old.ts => new.ts"), "new.ts");
        assert_eq!(numstat_path("plain.ts"), "plain.ts");
    }

    #[test]
    fn numstat_counts_reject_non_numbers() {
        assert_eq!(
            parse_numstat_counts("3\t1\tf").unwrap(),
            ParsedNumstat::Text {
                additions: 3,
                deletions: 1
            }
        );
        assert_eq!(
            parse_numstat_counts("-\t-\tf").unwrap(),
            ParsedNumstat::Binary
        );
        for line in ["x\t1\tf", "1\t\tf", "1", "-\t1\tf", "+1\t1\tf"] {
            assert!(
                matches!(
                    parse_numstat_counts(line),
                    Err(GitError::InvalidNumstat { .. })
                ),
                "{line:?} accepted"
            );
        }
    }

    #[test]
    fn numstat_without_a_path_fails() {
        assert!(matches!(
            parse_numstat("1\t2\n", &HashSet::new()),
            Err(GitError::NumstatWithoutPath { .. })
        ));
    }

    #[test]
    fn blame_ignores_line_zero_and_fills_gaps() {
        let h1 = "a".repeat(40);
        let h2 = "b".repeat(40);
        let porcelain = format!("{h1} 1 0\n{h2} 1 3 1\n");
        assert_eq!(
            parse_blame_porcelain(&porcelain),
            vec![String::new(), String::new(), h2]
        );
    }

    #[test]
    fn binary_sizes_beyond_exact_js_numbers_are_invalid() {
        let stdout = " f | Bin 9007199254740992 -> 1 bytes\n";
        assert!(matches!(
            parse_binary_byte_change(stdout, "f"),
            Err(GitError::BinarySizesInvalid { .. })
        ));
    }

    #[test]
    fn name_status_skips_malformed_lines() {
        let statuses = parse_name_statuses("M\n\tonly-path\nR100\told.ts\tnew.ts\n");
        assert_eq!(statuses.len(), 1);
        assert_eq!(
            statuses["new.ts"],
            NameStatus {
                status: FileChangeStatus::Renamed,
                origin: PathOrigin::RenamedFrom("old.ts".to_owned())
            }
        );
    }

    #[test]
    fn content_kinds() {
        assert!(is_image_path("img/Logo.PNG"));
        assert!(!is_image_path("mesh.obj"));
        assert!(is_binary_content("ELF\0"));
        assert!(!is_binary_content("text"));
        let late_nul = format!("{}\0", "x".repeat(BINARY_SNIFF_LENGTH));
        assert!(!is_binary_content(&late_nul));
        assert_eq!(detect_language("Makefile"), "plaintext");
        assert_eq!(detect_language("a.b.RS"), "rust");
    }
}
