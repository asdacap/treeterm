//! Review's diff file by file (`StackedDiffList`'s `FileLoadState`): each listed file's part
//! of the patch loads, fails and is refreshed on its own.
//!
//! Git's patches are split where a file starts (`diff --git`), each section keyed by the path
//! GPUI Kit's `Diff` shows it under (its new path; the old one for a deleted file), read the
//! way the Kit's parser reads it, so the model's keys are the view's.

use indexmap::IndexMap;

/// One listed file's diff (`FileLoadState`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileDiff {
    /// Being read: its section shows "Loading diff..." and its refresh button spins.
    Loading,
    /// Its section of the patch (empty: git printed none for it).
    Loaded(String),
    /// Why it could not be read, shown in its own section.
    Failed(String),
}

impl FileDiff {
    /// A failed read: git's message, or `'Failed to load diff'` when it gave none.
    pub fn failed(message: &str) -> Self {
        match message.trim() {
            "" => FileDiff::Failed("Failed to load diff".to_owned()),
            message => FileDiff::Failed(message.to_owned()),
        }
    }

    /// `path`'s section of a command's patch, or the command's failure.
    pub fn of(path: &str, patch: &Result<String, String>) -> Self {
        match patch {
            Ok(patch) => FileDiff::Loaded(split_patch(patch).swap_remove(path).unwrap_or_default()),
            Err(error) => FileDiff::failed(error),
        }
    }
}

/// Each listed file's [`FileDiff`], in list order; a path listed twice has its first listing's.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileDiffs(IndexMap<String, FileDiff>);

impl FileDiffs {
    /// `paths`, each loading.
    pub fn loading<'a>(paths: impl IntoIterator<Item = &'a str>) -> Self {
        let mut diffs = Self::default();
        for path in paths {
            diffs.add(path, FileDiff::Loading);
        }
        diffs
    }

    /// `paths` with their sections of one command's patch, or its failure each.
    pub fn read<'a>(
        paths: impl IntoIterator<Item = &'a str>,
        patch: &Result<String, String>,
    ) -> Self {
        let mut diffs = Self::default();
        diffs.add_read(paths, patch);
        diffs
    }

    /// Adds `paths` not listed yet with their sections of one command's patch, or its failure.
    pub fn add_read<'a>(
        &mut self,
        paths: impl IntoIterator<Item = &'a str>,
        patch: &Result<String, String>,
    ) {
        let mut sections = match patch {
            Ok(patch) => Ok(split_patch(patch)),
            Err(error) => Err(FileDiff::failed(error)),
        };
        for path in paths {
            let diff = match &mut sections {
                Ok(sections) => FileDiff::Loaded(sections.swap_remove(path).unwrap_or_default()),
                Err(failed) => failed.clone(),
            };
            self.add(path, diff);
        }
    }

    /// Adds `path` unless it is listed already.
    pub fn add(&mut self, path: &str, diff: FileDiff) {
        if !self.0.contains_key(path) {
            self.0.insert(path.to_owned(), diff);
        }
    }

    /// Replaces a listed path's diff (one no longer listed is left out).
    pub fn set(&mut self, path: &str, diff: FileDiff) {
        if let Some(entry) = self.0.get_mut(path) {
            *entry = diff;
        }
    }

    pub fn get(&self, path: &str) -> Option<&FileDiff> {
        self.0.get(path)
    }

    /// `path` is being read.
    pub fn is_loading(&self, path: &str) -> bool {
        matches!(self.0.get(path), Some(FileDiff::Loading))
    }

    /// Every file has been read (loaded or failed).
    pub fn is_complete(&self) -> bool {
        !self
            .0
            .values()
            .any(|diff| matches!(diff, FileDiff::Loading))
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &FileDiff)> {
        self.0.iter().map(|(path, diff)| (path.as_str(), diff))
    }

    /// The diff as the view parses it, in list order: each loaded file's section, and a bare
    /// `diff --git` header for a file loading or failed, so that it has a section (whose
    /// body the view draws: "Loading diff...", or the error). Bare headers have no lines.
    pub fn patch(&self) -> String {
        let mut patch = String::new();
        for (path, diff) in &self.0 {
            match diff {
                FileDiff::Loaded(section) => patch.push_str(section),
                FileDiff::Loading | FileDiff::Failed(_) => patch.push_str(&bare_header(path)),
            }
        }
        patch
    }
}

/// A `diff --git` header naming `path` alone, its paths quoted as git quotes them.
fn bare_header(path: &str) -> String {
    format!("diff --git {} {}\n", quoted("a/", path), quoted("b/", path))
}

/// `prefix` and `path` in double quotes, with C-style escapes (git's `quote_c_style`).
fn quoted(prefix: &str, path: &str) -> String {
    let mut out = String::from("\"");
    out.push_str(prefix);
    for character in path.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            control if control.is_control() => {
                let mut bytes = [0; 4];
                for byte in control.encode_utf8(&mut bytes).bytes() {
                    out.push_str(&format!("\\{byte:03o}"));
                }
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// A patch's sections (from each `diff --git` line to the next), by the path each shows
/// under; a path's first section is kept. Anything before the first file is dropped.
pub fn split_patch(patch: &str) -> IndexMap<String, String> {
    let mut sections: IndexMap<String, String> = IndexMap::new();
    let mut starts: Vec<usize> = Vec::new();
    let mut at = 0;
    for line in patch.split_inclusive('\n') {
        if line.starts_with("diff --git ") {
            starts.push(at);
        }
        at += line.len();
    }
    for (index, &start) in starts.iter().enumerate() {
        let section = &patch[start..starts.get(index + 1).copied().unwrap_or(patch.len())];
        if let Some(path) = section_path(section) {
            sections.entry(path).or_insert_with(|| section.to_owned());
        }
    }
    sections
}

/// The path a section was renamed from (`rename from`), if it was.
pub fn rename_from(section: &str) -> Option<String> {
    section
        .lines()
        .take_while(|line| !line.starts_with("@@ "))
        .find_map(|line| line.strip_prefix("rename from "))
        .and_then(unquote)
}

/// Prefix pairs git may give a diff's sides (`diff.mnemonicPrefix`, `diff.noprefix`), as
/// GPUI Kit's parser knows them.
const GIT_PREFIXES: [(&str, &str); 7] = [
    ("a/", "b/"),
    ("i/", "w/"),
    ("c/", "w/"),
    ("c/", "i/"),
    ("o/", "w/"),
    ("1/", "2/"),
    ("", ""),
];

/// The path GPUI Kit's `Diff` shows a section under: the path of its new side (a rename's
/// target), or of its old side when the file was deleted.
fn section_path(section: &str) -> Option<String> {
    let mut lines = section.lines();
    let (old, new, (old_prefix, new_prefix)) =
        git_paths(lines.next()?.strip_prefix("diff --git ")?)?;
    let (mut old, mut new) = (Some(old), Some(new));
    for line in lines.take_while(|line| !line.starts_with("@@ ")) {
        if let Some(path) = line.strip_prefix("--- ") {
            old = side_path(path)?.map(|path| strip(path, old_prefix));
        } else if let Some(path) = line.strip_prefix("+++ ") {
            new = side_path(path)?.map(|path| strip(path, new_prefix));
        } else if let Some(path) = line
            .strip_prefix("rename from ")
            .or_else(|| line.strip_prefix("copy from "))
        {
            old = Some(unquote(path)?);
        } else if let Some(path) = line
            .strip_prefix("rename to ")
            .or_else(|| line.strip_prefix("copy to "))
        {
            new = Some(unquote(path)?);
        } else if line.starts_with("new file mode ") {
            old = None;
        } else if line.starts_with("deleted file mode ") {
            new = None;
        }
    }
    new.or(old)
}

/// A `---` or `+++` path (git ends one holding a space with a tab), `None` for `/dev/null`.
fn side_path(value: &str) -> Option<Option<String>> {
    match value.split('\t').next().unwrap_or(value) {
        "/dev/null" => Some(None),
        path => unquote(path).map(Some),
    }
}

fn strip(path: String, prefix: &str) -> String {
    match path.strip_prefix(prefix) {
        Some(stripped) if !prefix.is_empty() => stripped.to_owned(),
        _ => path,
    }
}

/// The two paths of a `diff --git` header, without their prefixes, and the prefixes.
fn git_paths(value: &str) -> Option<(String, String, (&'static str, &'static str))> {
    let detect = |old: &str, new: &str| {
        GIT_PREFIXES
            .into_iter()
            .find(|(o, n)| old.starts_with(o) && new.starts_with(n))
            .unwrap_or(("", ""))
    };
    let (old, new) = if value.starts_with('"') {
        let end = quoted_end(value)?;
        (unquote(&value[..end])?, unquote(value[end..].trim_start())?)
    } else if value.ends_with('"') {
        let start = value
            .match_indices(" \"")
            .map(|(index, _)| index + 1)
            .find(|index| quoted_end(&value[*index..]) == Some(value.len() - index))?;
        (value[..start - 1].to_owned(), unquote(&value[start..])?)
    } else {
        // Git does not quote spaces. Both sides name the same path unless it was renamed,
        // which finds the separating space; a rename's own lines name its paths.
        let same = GIT_PREFIXES.into_iter().find_map(|(o, n)| {
            value.match_indices(' ').find_map(|(index, _)| {
                let (old, new) = (&value[..index], &value[index + 1..]);
                let path = old.strip_prefix(o)?;
                (!path.is_empty() && new.strip_prefix(n) == Some(path)).then_some((old, new))
            })
        });
        let (old, new) = same
            .or_else(|| {
                GIT_PREFIXES
                    .into_iter()
                    .filter(|(o, _)| !o.is_empty())
                    .find_map(|(o, n)| {
                        let index = value.rfind(&format!(" {n}"))?;
                        value
                            .starts_with(o)
                            .then_some((&value[..index], &value[index + 1..]))
                    })
            })
            .or_else(|| value.split_once(' '))?;
        (old.to_owned(), new.to_owned())
    };
    let prefixes = detect(&old, &new);
    Some((strip(old, prefixes.0), strip(new, prefixes.1), prefixes))
}

/// Where the quoted string `value` starts with ends (just past its closing quote).
fn quoted_end(value: &str) -> Option<usize> {
    let mut escaped = false;
    for (index, character) in value.char_indices().skip(1) {
        if escaped {
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character == '"' {
            return Some(index + 1);
        }
    }
    None
}

/// A path as git prints it: as it is, or in double quotes with C-style escapes (octal for
/// each byte of a character it quotes). `None`: not a valid quoted path.
fn unquote(value: &str) -> Option<String> {
    if !value.starts_with('"') {
        return Some(value.to_owned());
    }
    if quoted_end(value) != Some(value.len()) {
        return None;
    }
    let bytes = &value.as_bytes()[1..value.len() - 1];
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'\\' {
            out.push(bytes[index]);
            index += 1;
            continue;
        }
        index += 1;
        let escaped = *bytes.get(index)?;
        if escaped.is_ascii_digit() && escaped < b'8' {
            let mut byte: u16 = 0;
            let mut count = 0;
            while count < 3 && bytes.get(index).is_some_and(|b| (b'0'..=b'7').contains(b)) {
                byte = byte * 8 + u16::from(bytes[index] - b'0');
                index += 1;
                count += 1;
            }
            out.push(u8::try_from(byte).ok()?);
            continue;
        }
        out.push(match escaped {
            b'n' => b'\n',
            b't' => b'\t',
            b'r' => b'\r',
            b'a' => 7,
            b'b' => 8,
            b'f' => 12,
            b'v' => 11,
            b'"' | b'\\' => escaped,
            _ => return None,
        });
        index += 1;
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODIFIED: &str = "diff --git a/a.txt b/a.txt\nindex 1..2 100644\n--- a/a.txt\n+++ b/a.txt\n@@ -1 +1 @@\n-one\n+ONE\n";
    const ADDED: &str = "diff --git a/new.txt b/new.txt\nnew file mode 100644\nindex 0..1\n--- /dev/null\n+++ b/new.txt\n@@ -0,0 +1 @@\n+new\n";
    const DELETED: &str = "diff --git a/old.txt b/old.txt\ndeleted file mode 100644\nindex 1..0\n--- a/old.txt\n+++ /dev/null\n@@ -1 +0,0 @@\n-old\n";
    const RENAMED: &str = "diff --git a/from.txt b/sp ace.txt\nsimilarity index 100%\nrename from from.txt\nrename to sp ace.txt\n";
    const BINARY: &str = "diff --git a/logo.png b/logo.png\nindex 1..2 100644\nBinary files a/logo.png and b/logo.png differ\n";
    const QUOTED: &str = "diff --git \"a/caf\\303\\251.txt\" \"b/caf\\303\\251.txt\"\nnew file mode 100644\nindex 0..1\n--- /dev/null\n+++ \"b/caf\\303\\251.txt\"\n@@ -0,0 +1 @@\n+caf\n";

    #[test]
    fn a_patch_splits_into_each_files_section_by_the_path_it_shows_under() {
        let patch = [MODIFIED, ADDED, DELETED, RENAMED, BINARY, QUOTED].concat();
        let sections = split_patch(&patch);
        assert_eq!(
            sections.keys().map(String::as_str).collect::<Vec<_>>(),
            [
                "a.txt",
                "new.txt",
                "old.txt",
                "sp ace.txt",
                "logo.png",
                "caf\u{e9}.txt"
            ]
        );
        assert_eq!(sections["a.txt"], MODIFIED);
        assert_eq!(sections["sp ace.txt"], RENAMED);
        assert_eq!(sections["caf\u{e9}.txt"], QUOTED);
        // A path's first section is kept (staged before unstaged); nothing before a file.
        let twice = format!("preamble\n{MODIFIED}{}", MODIFIED.replace("ONE", "TWO"));
        assert_eq!(
            split_patch(&twice).into_iter().collect::<Vec<_>>(),
            [("a.txt".to_owned(), MODIFIED.to_owned())]
        );
        assert!(split_patch("").is_empty());
        // A path with a space on the `+++` side ends with a tab; mnemonic prefixes are stripped.
        let spaced = "diff --git a/s p b/s p\n--- /dev/null\n+++ b/s p\t\n@@ -0,0 +1 @@\n+x\n";
        assert_eq!(split_patch(spaced).keys().collect::<Vec<_>>(), ["s p"]);
        let mnemonic = "diff --git c/x.rs w/x.rs\n--- c/x.rs\n+++ w/x.rs\n@@ -1 +1 @@\n-a\n+b\n";
        assert_eq!(split_patch(mnemonic).keys().collect::<Vec<_>>(), ["x.rs"]);
    }

    #[test]
    fn a_rename_names_the_path_it_came_from() {
        assert_eq!(rename_from(RENAMED).as_deref(), Some("from.txt"));
        assert_eq!(rename_from(MODIFIED), None);
        assert_eq!(
            rename_from("diff --git a/x b/y\nrename from \"q\\\"\"\nrename to y\n").as_deref(),
            Some("q\"")
        );
    }

    #[test]
    fn bare_headers_name_any_path_exactly() {
        for path in [
            "plain.txt",
            "sp ace/x y.txt",
            "q\"uote\\back",
            "tab\there",
            "caf\u{e9}",
            "bell\u{7}",
        ] {
            let header = bare_header(path);
            assert_eq!(
                split_patch(&header).keys().collect::<Vec<_>>(),
                [path],
                "{header}"
            );
        }
        assert_eq!(unquote("\"bad"), None);
        assert_eq!(unquote("\"\\q\""), None);
        assert_eq!(unquote("\"\\777\""), None);
    }

    #[test]
    fn each_file_is_loading_loaded_or_failed_on_its_own() {
        let mut diffs = FileDiffs::loading(["a.txt", "b.txt"]);
        assert!(diffs.is_loading("a.txt") && !diffs.is_complete());
        assert_eq!(
            diffs.patch(),
            [bare_header("a.txt"), bare_header("b.txt")].concat()
        );

        // A command's sections, a file it printed nothing for, and its failure.
        diffs = FileDiffs::read(["a.txt", "b.txt"], &Ok(MODIFIED.to_owned()));
        assert_eq!(
            diffs.get("a.txt"),
            Some(&FileDiff::Loaded(MODIFIED.to_owned()))
        );
        assert_eq!(diffs.get("b.txt"), Some(&FileDiff::Loaded(String::new())));
        assert!(diffs.is_complete());
        diffs.add_read(["a.txt", "new.txt"], &Err("fatal: bad revision".to_owned()));
        assert_eq!(
            diffs.get("a.txt"),
            Some(&FileDiff::Loaded(MODIFIED.to_owned())),
            "listed already"
        );
        assert_eq!(
            diffs.get("new.txt"),
            Some(&FileDiff::Failed("fatal: bad revision".to_owned()))
        );
        assert_eq!(diffs.patch(), [MODIFIED, &bare_header("new.txt")].concat());
        assert_eq!(
            diffs.iter().map(|(path, _)| path).collect::<Vec<_>>(),
            ["a.txt", "b.txt", "new.txt"]
        );

        // One file read again; a path no longer listed is left out.
        diffs.set(
            "b.txt",
            FileDiff::of("b.txt", &Ok(ADDED.replace("new.txt", "b.txt"))),
        );
        assert!(
            matches!(diffs.get("b.txt"), Some(FileDiff::Loaded(section)) if section.contains("+++ b/b.txt"))
        );
        diffs.set("gone.txt", FileDiff::Loading);
        assert_eq!(diffs.get("gone.txt"), None);
        assert_eq!(
            FileDiff::of("a.txt", &Err("  ".to_owned())),
            FileDiff::Failed("Failed to load diff".to_owned())
        );
    }
}
