//! Favourite files and directories (port of `utils/favouriteFiles.ts` and
//! `shared/workspaceFavourites.ts`).
//!
//! Favourites are stored as workspace-relative paths with `/` separators, so they stay
//! portable between local and remote hosts. A workspace's favourites are its own and then
//! every ancestor's: child worktrees inherit them.

use std::collections::{HashSet, VecDeque};

use indexmap::IndexMap;
use treeterm_proto::treeterm::FileEntry;

use crate::fs::{FsError, HiddenEntries};
use crate::fs_ext::DirectoryApi;
use crate::locale;
use crate::wire::workspace_file::WorkspaceFile;

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

/// Favourite path lists joined in order (`getWorkspaceFavouritePaths` over a workspace's own
/// list, then its parent's, its grandparent's, …): each path normalized, empty ones dropped,
/// and each kept where it first appears.
pub fn combine_favourite_paths<'a>(lists: impl IntoIterator<Item = &'a [String]>) -> Vec<String> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut paths = Vec::new();
    for raw_path in lists.into_iter().flatten() {
        let path = normalize_favourite_path(raw_path);
        if !path.is_empty() && seen.insert(path.clone()) {
            paths.push(path);
        }
    }
    paths
}

/// The favourite paths workspace `id` inherits: its parent's, then its grandparent's, and so
/// on, combined as [`combine_favourite_paths`] does (`getWorkspaceFavouritePaths` past the
/// workspace's own list). `lookup` finds a loaded workspace's body; the walk ends at a
/// workspace it cannot find and at one it has already passed (a `parentId` cycle).
pub fn inherited_favourite_paths<'a>(
    id: &str,
    lookup: impl Fn(&str) -> Option<&'a WorkspaceFile>,
) -> Vec<String> {
    let mut seen: HashSet<String> = HashSet::from([id.to_owned()]);
    let mut lists: Vec<&'a [String]> = Vec::new();
    let mut parent = lookup(id).and_then(|file| file.parent_id.as_deref());
    while let Some(parent_id) = parent {
        if !seen.insert(parent_id.to_owned()) {
            break;
        }
        let Some(file) = lookup(parent_id) else {
            break;
        };
        lists.push(&file.favourite_paths);
        parent = file.parent_id.as_deref();
    }
    combine_favourite_paths(lists)
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
/// that does not exist here (inherited from a parent whose branch has it) is skipped. A
/// favourite may name a hidden entry (starred while the tree showed them); a favourite
/// directory expands to its files that are not hidden.
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
                .read_directory(workspace_path, &entry.path, HiddenEntries::Skip)
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
    files.sort_by(|a, b| locale::compare(&a.relative_path, &b.relative_path));
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
            .read_directory(workspace_path, &directory_path, HiddenEntries::Include)
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
        fn finds_hidden_favourites_but_skips_hidden_files_in_favourite_directories() {
            let fs = FakeFs::new();
            fs.set_directory(
                "/repo",
                vec![entry(".env", false), entry(".github", true)],
            );
            fs.set_directory(
                "/repo/.github",
                vec![entry(".github/.keep", false), entry(".github/ci.yml", false)],
            );

            let files = block_on(resolve_favourite_files(
                "/repo",
                &paths(&[".env", ".github"]),
                fs.as_ref(),
            ))
            .unwrap();
            assert_eq!(files, vec![favourite(".env"), favourite(".github/ci.yml")]);
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

        #[test]
        fn sorts_files_as_locale_compare_does() {
            // An accented letter sorts beside its base letter (in every locale Electron
            // ships), not after `z` as its code point would.
            let fs = FakeFs::new();
            fs.set_directory(
                "/repo",
                vec![
                    entry("zed.ts", false),
                    entry("éa.ts", false),
                    entry("fa.ts", false),
                ],
            );
            let files = block_on(resolve_favourite_files(
                "/repo",
                &paths(&["zed.ts", "éa.ts", "fa.ts"]),
                fs.as_ref(),
            ))
            .unwrap();
            assert_eq!(
                files,
                vec![favourite("éa.ts"), favourite("fa.ts"), favourite("zed.ts")]
            );
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

    /// `getWorkspaceFavouritePaths` (`shared/workspaceFavourites.test.ts`), split into
    /// joining the lists and walking up to the ancestors.
    mod workspace_favourite_paths {
        use std::collections::HashMap;

        use super::*;
        use crate::wire::workspace_file::parse_workspace_file;

        fn file(parent: Option<&str>, favourites: &[&str]) -> WorkspaceFile {
            let mut json = serde_json::json!({
                "name": "w", "status": "active", "isGitRepo": true, "isWorktree": false,
                "appStates": {}, "metadata": {}, "createdAt": 1, "lastActivity": 1,
                "parentHash": "", "favouritePaths": favourites
            });
            if let Some(parent) = parent {
                json["parentId"] = parent.into();
            }
            parse_workspace_file(json.to_string().as_bytes()).unwrap()
        }

        fn inherited(id: &str, workspaces: &HashMap<&str, WorkspaceFile>) -> Vec<String> {
            inherited_favourite_paths(id, |id| workspaces.get(id))
        }

        #[test]
        fn combines_lists_in_order_normalized_without_empties_or_repeats() {
            let own = paths(&["./src/", "", "README.md"]);
            let parent = paths(&["/docs", "src", "/", "README.md/"]);
            assert_eq!(
                combine_favourite_paths([own.as_slice(), parent.as_slice()]),
                paths(&["src", "README.md", "docs"])
            );
            assert!(combine_favourite_paths([paths(&["", "./", "//"]).as_slice()]).is_empty());
        }

        #[test]
        fn inherits_the_parents_then_the_grandparents_favourites() {
            let workspaces = HashMap::from([
                ("root", file(None, &["docs", "shared.md"])),
                ("parent", file(Some("root"), &["src", "./shared.md"])),
                ("child", file(Some("parent"), &["mine.md"])),
            ]);
            assert_eq!(
                inherited("child", &workspaces),
                paths(&["src", "shared.md", "docs"])
            );
            assert_eq!(
                inherited("parent", &workspaces),
                paths(&["docs", "shared.md"])
            );
            assert!(inherited("root", &workspaces).is_empty());
        }

        #[test]
        fn a_missing_parent_ends_the_walk() {
            let workspaces = HashMap::from([
                ("root", file(None, &["docs"])),
                ("parent", file(Some("gone"), &["src"])),
                ("child", file(Some("parent"), &[])),
                ("orphan", file(Some(""), &["a"])),
            ]);
            assert_eq!(inherited("child", &workspaces), paths(&["src"]));
            assert!(inherited("orphan", &workspaces).is_empty());
            assert!(inherited("unknown", &workspaces).is_empty());
        }

        #[test]
        fn a_parent_cycle_ends_the_walk() {
            let workspaces = HashMap::from([
                ("a", file(Some("b"), &["a.md"])),
                ("b", file(Some("c"), &["b.md"])),
                ("c", file(Some("a"), &["c.md"])),
                ("self", file(Some("self"), &["self.md"])),
            ]);
            assert_eq!(inherited("a", &workspaces), paths(&["b.md", "c.md"]));
            assert!(inherited("self", &workspaces).is_empty());
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
}
