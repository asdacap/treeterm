//! Favourite files and directories (port of `utils/favouriteFiles.ts`, with the path helpers
//! of `shared/workspaceFavourites.ts` it needs).
//!
//! Favourites are stored as workspace-relative paths with `/` separators, so they stay
//! portable between local and remote hosts.

use std::cmp::Ordering;
use std::collections::{HashSet, VecDeque};

use indexmap::IndexMap;
use treeterm_proto::treeterm::FileEntry;

use crate::fs::FsError;
use crate::fs_ext::DirectoryApi;

/// A favourite file, expanded from a favourite path (a directory favourite expands to every
/// file under it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FavouriteFile {
    /// Absolute, as the daemon reports it.
    pub path: String,
    /// Workspace-relative, with `/` separators.
    pub relative_path: String,
}

/// `normalizeFavouritePath`: no leading `./` or `/`, no trailing `/`.
pub fn normalize_favourite_path(path: &str) -> String {
    let path = path.strip_prefix("./").unwrap_or(path);
    path.trim_start_matches('/')
        .trim_end_matches('/')
        .to_owned()
}

/// `normalizeFileEntryRelativePath`: a `FileEntry.relative_path` uses the daemon host's
/// separator; on a Windows workspace it is converted to `/`.
pub fn normalize_file_entry_relative_path(path: &str, workspace_path: &str) -> String {
    let bytes = workspace_path.as_bytes();
    let drive_root = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/');
    let is_windows =
        drive_root || workspace_path.starts_with("\\\\") || workspace_path.starts_with("//");
    if is_windows {
        normalize_favourite_path(&path.replace('\\', "/"))
    } else {
        normalize_favourite_path(path)
    }
}

/// The label for a favourite in lists: its basename (the full relative path is the tooltip).
/// Favourites are often deep in a tree, and the leading directories crowd out the part that
/// identifies the file.
pub fn favourite_file_name(file: &FavouriteFile) -> &str {
    file.relative_path
        .rsplit('/')
        .next()
        .unwrap_or(&file.relative_path)
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum FavouritesError {
    /// A directory on the way could not be listed.
    #[error("{}", load_message(.path, .source))]
    Load { path: String, source: FsError },
}

fn load_message(path: &str, source: &FsError) -> String {
    match source.to_string() {
        message if message.is_empty() => format!("Failed to load {path}"),
        message => message,
    }
}

/// Expand `favourite_paths` to the files they name, sorted by relative path. A favourite
/// that does not exist here (inherited from a parent whose branch has it) is skipped.
///
/// Directories are read one at a time: a favourite may cover a very large tree, and one
/// request per directory all at once would flood the daemon. Dropping the future stops the
/// walk after the read in flight.
pub async fn resolve_favourite_files(
    workspace_path: &str,
    favourite_paths: &[String],
    fs: &dyn DirectoryApi,
) -> Result<Vec<FavouriteFile>, FavouritesError> {
    let mut files: IndexMap<String, FavouriteFile> = IndexMap::new();
    let mut visited_directories: HashSet<String> = HashSet::new();

    for raw_path in favourite_paths {
        let relative_path = normalize_favourite_path(raw_path);
        if relative_path.is_empty() {
            continue;
        }
        let Some(entry) = resolve_entry(workspace_path, &relative_path, fs).await? else {
            continue;
        };
        // Depth first, children in listing order.
        let mut pending = VecDeque::from([entry]);
        while let Some(entry) = pending.pop_front() {
            if !entry.is_directory {
                let relative_path =
                    normalize_file_entry_relative_path(&entry.relative_path, workspace_path);
                files.insert(
                    relative_path.clone(),
                    FavouriteFile {
                        path: entry.path,
                        relative_path,
                    },
                );
                continue;
            }
            if !visited_directories.insert(entry.path.clone()) {
                continue;
            }
            let contents = fs
                .read_directory(workspace_path, &entry.path)
                .await
                .map_err(|source| FavouritesError::Load {
                    path: entry.relative_path.clone(),
                    source,
                })?;
            for child in contents.entries.into_iter().rev() {
                pending.push_front(child);
            }
        }
    }

    let mut files: Vec<FavouriteFile> = files.into_values().collect();
    files.sort_by(|a, b| locale_compare(&a.relative_path, &b.relative_path));
    Ok(files)
}

/// The entry at `relative_path`, walking down from the workspace root.
async fn resolve_entry(
    workspace_path: &str,
    relative_path: &str,
    fs: &dyn DirectoryApi,
) -> Result<Option<FileEntry>, FavouritesError> {
    let parts: Vec<&str> = relative_path.split('/').collect();
    let mut directory_path = workspace_path.to_owned();
    for (index, name) in parts.iter().enumerate() {
        let contents = fs
            .read_directory(workspace_path, &directory_path)
            .await
            .map_err(|source| FavouritesError::Load {
                path: relative_path.to_owned(),
                source,
            })?;
        let Some(entry) = contents
            .entries
            .into_iter()
            .find(|candidate| candidate.name == *name)
        else {
            return Ok(None);
        };
        if index == parts.len() - 1 {
            return Ok(Some(entry));
        }
        if !entry.is_directory {
            return Ok(None);
        }
        directory_path = entry.path;
    }
    Ok(None)
}

/// An approximation of `String.prototype.localeCompare` (ICU root collation) for paths:
/// letters compare case-insensitively, and on a tie lowercase sorts first.
pub fn locale_compare(a: &str, b: &str) -> Ordering {
    a.to_lowercase()
        .cmp(&b.to_lowercase())
        .then_with(|| b.cmp(a))
}

#[cfg(test)]
mod tests {
    //! Port of `utils/favouriteFiles.test.ts`, case by case.

    use super::*;
    use crate::fakes::{FakeFs, block_on};

    fn entry(relative_path: &str, is_directory: bool) -> FileEntry {
        FileEntry {
            name: relative_path.rsplit('/').next().unwrap().to_owned(),
            path: format!("/repo/{relative_path}"),
            relative_path: relative_path.to_owned(),
            is_directory,
            size: None,
            modified_time: None,
        }
    }

    fn favourite(relative_path: &str) -> FavouriteFile {
        FavouriteFile {
            path: format!("/repo/{relative_path}"),
            relative_path: relative_path.to_owned(),
        }
    }

    fn paths(paths: &[&str]) -> Vec<String> {
        paths.iter().map(|path| (*path).to_owned()).collect()
    }

    mod resolve_favourite_files {
        use super::*;

        #[test]
        fn resolves_files_and_recursively_expands_favourite_directories() {
            let fs = FakeFs::new();
            fs.set_directory("/repo", vec![entry("README.md", false), entry("src", true)]);
            fs.set_directory(
                "/repo/src",
                vec![entry("src/index.ts", false), entry("src/components", true)],
            );
            fs.set_directory(
                "/repo/src/components",
                vec![entry("src/components/App.tsx", false)],
            );

            let files = block_on(resolve_favourite_files(
                "/repo",
                &paths(&["README.md", "src"]),
                fs.as_ref(),
            ))
            .unwrap();
            assert_eq!(
                files,
                vec![
                    favourite("README.md"),
                    favourite("src/components/App.tsx"),
                    favourite("src/index.ts"),
                ]
            );
        }

        #[test]
        fn deduplicates_overlapping_file_and_directory_favourites() {
            let fs = FakeFs::new();
            fs.set_directory("/repo", vec![entry("src", true)]);
            fs.set_directory("/repo/src", vec![entry("src/index.ts", false)]);

            let files = block_on(resolve_favourite_files(
                "/repo",
                &paths(&["src", "src/index.ts"]),
                fs.as_ref(),
            ))
            .unwrap();
            assert_eq!(files, vec![favourite("src/index.ts")]);
        }

        #[test]
        fn ignores_inherited_paths_whose_file_or_parent_directory_does_not_exist_in_the_current_workspace()
         {
            let fs = FakeFs::new();
            fs.set_directory("/repo", vec![entry("present.ts", false)]);
            let files = block_on(resolve_favourite_files(
                "/repo",
                &paths(&["missing.ts", "missing/child.ts", "present.ts"]),
                fs.as_ref(),
            ))
            .unwrap();
            assert_eq!(files, vec![favourite("present.ts")]);
        }

        #[test]
        fn fails_loudly_when_a_favourite_directory_cannot_be_read() {
            let fs = FakeFs::new();
            fs.set_directory("/repo", vec![entry("src", true)]);
            fs.fail_directory("/repo/src", "permission denied");
            let error = block_on(resolve_favourite_files(
                "/repo",
                &paths(&["src"]),
                fs.as_ref(),
            ))
            .unwrap_err();
            assert_eq!(error.to_string(), "permission denied");
        }

        /// TS passed an `isCancelled` callback; here dropping the future is the cancellation.
        #[test]
        fn stops_scheduling_directory_reads_after_cancellation() {
            let fs = FakeFs::new();
            fs.set_directory("/repo", vec![entry("src", true)]);
            fs.set_directory(
                "/repo/src",
                vec![entry("src/first", true), entry("src/second", true)],
            );
            fs.hang_directory("/repo/src/first");

            let favourites = paths(&["src"]);
            let mut walk = Box::pin(resolve_favourite_files("/repo", &favourites, fs.as_ref()));
            assert!(futures::FutureExt::now_or_never(&mut walk).is_none());
            drop(walk);

            assert_eq!(
                fs.directory_reads(),
                vec!["/repo", "/repo/src", "/repo/src/first"]
            );
        }

        #[test]
        fn normalizes_favourite_paths_and_skips_empty_ones() {
            let fs = FakeFs::new();
            fs.set_directory("/repo", vec![entry("a.ts", false)]);
            let files = block_on(resolve_favourite_files(
                "/repo",
                &paths(&["./a.ts/", "/", ""]),
                fs.as_ref(),
            ))
            .unwrap();
            assert_eq!(files, vec![favourite("a.ts")]);
        }
    }

    mod favourite_file_name {
        use super::*;

        #[test]
        fn reduces_a_nested_path_to_its_basename() {
            assert_eq!(
                favourite_file_name(&favourite("src/deep/index.ts")),
                "index.ts"
            );
        }

        #[test]
        fn keeps_a_root_level_path_unchanged() {
            assert_eq!(favourite_file_name(&favourite("README.md")), "README.md");
        }
    }

    #[test]
    fn windows_relative_paths_use_forward_slashes() {
        assert_eq!(
            normalize_file_entry_relative_path("src\\a.ts", "C:\\repo"),
            "src/a.ts"
        );
        assert_eq!(
            normalize_file_entry_relative_path("src\\a.ts", "//server/share"),
            "src/a.ts"
        );
        assert_eq!(
            normalize_file_entry_relative_path("src\\a.ts", "/repo"),
            "src\\a.ts"
        );
    }

    #[test]
    fn locale_compare_ignores_case_then_puts_lowercase_first() {
        let mut names = vec!["b", "A", "a", "C", "_x"];
        names.sort_by(|a, b| locale_compare(a, b));
        assert_eq!(names, vec!["_x", "a", "A", "b", "C"]);
    }
}
