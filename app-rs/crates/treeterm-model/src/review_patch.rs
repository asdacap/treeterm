//! Review's diff file by file (`StackedDiffList`'s `FileLoadState`): each listed file's part
//! of the patch is read when it is first wanted (its section nears the viewport, or find
//! searches it), and fails and is refreshed on its own.
//!
//! Git's patches are split where a file starts (`diff --git`), each section keyed by the path
//! GPUI Kit's `Diff` shows it under (its new path; the old one for a deleted file), read the
//! way the Kit's parser reads it, so the model's keys are the view's.
//!
//! Each file shows on its own ([`shown_diff`]): its rows, or under its header alone what
//! stands in for them. An image shows its sides, a file too large to draw at once
//! (`FileDiffSection`'s size guard) a notice until "Load anyway", and the view parses each
//! other section by itself, so that a binary file shows a notice and a section that does not
//! parse its error while the others show their rows.

use indexmap::IndexMap;
use treeterm_core::files::is_image_path;
use treeterm_core::locale::ElectronLocale;

/// One listed file's diff (`FileLoadState`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileDiff {
    /// Not asked for yet: its section shows "Scroll to load diff" until it nears the viewport.
    NotLoaded,
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
    /// `paths`, none asked for yet.
    pub fn not_loaded<'a>(paths: impl IntoIterator<Item = &'a str>) -> Self {
        let mut diffs = Self::default();
        for path in paths {
            diffs
                .0
                .entry(path.to_owned())
                .or_insert(FileDiff::NotLoaded);
        }
        diffs
    }

    /// Sets listed `paths` to their sections of one command's patch, or to its failure.
    pub fn set_read<'a>(
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
            self.set(path, diff);
        }
    }

    /// A reload's diffs (`self`: the files it listed, those it read again set) as it finishes,
    /// `shown` being what the list shows meanwhile. A file it left not asked for but asked for
    /// in `shown` (being read, read or failed) was asked for since it listed its files, on its
    /// own (near the viewport, searched): that read is the later one, so it is kept, read or
    /// still being read. The files it read again keep what it read. The files kept, in list
    /// order.
    pub fn keep_asked_since(&mut self, shown: &FileDiffs) -> Vec<String> {
        let mut kept = Vec::new();
        for (path, diff) in &mut self.0 {
            match (&*diff, shown.0.get(path)) {
                (
                    FileDiff::NotLoaded,
                    Some(asked @ (FileDiff::Loading | FileDiff::Loaded(_) | FileDiff::Failed(_))),
                ) => {
                    *diff = asked.clone();
                    kept.push(path.clone());
                }
                (FileDiff::NotLoaded, Some(FileDiff::NotLoaded) | None)
                | (FileDiff::Loading | FileDiff::Loaded(_) | FileDiff::Failed(_), _) => {}
            }
        }
        kept
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

    /// Every file has been read (loaded or failed): none is still to ask for or being read.
    pub fn is_complete(&self) -> bool {
        !self
            .0
            .values()
            .any(|diff| matches!(diff, FileDiff::NotLoaded | FileDiff::Loading))
    }

    /// The files whose diff has been asked for (being read, read or failed), in list order.
    pub fn asked(&self) -> impl Iterator<Item = &str> {
        self.iter()
            .filter(|(_, diff)| !matches!(diff, FileDiff::NotLoaded))
            .map(|(path, _)| path)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &FileDiff)> {
        self.0.iter().map(|(path, diff)| (path.as_str(), diff))
    }

    /// The diff as the view parses it, in list order: each loaded file's section, and a bare
    /// `diff --git` header for any other file, so that it has a section (whose body the view
    /// draws: "Scroll to load diff", "Loading diff...", or the error). Bare headers have no
    /// lines.
    pub fn patch(&self) -> String {
        let mut patch = String::new();
        for (path, diff) in &self.0 {
            match diff {
                FileDiff::Loaded(section) => patch.push_str(section),
                FileDiff::NotLoaded | FileDiff::Loading | FileDiff::Failed(_) => {
                    patch.push_str(&bare_header(path))
                }
            }
        }
        patch
    }
}

/// `MAX_DIFF_LINES`: a file whose sides have more lines than this together is not drawn until
/// "Load anyway" (drawing a whole file's diff at once can exhaust the window's memory).
pub const MAX_DIFF_LINES: usize = 20_000;

/// `MAX_DIFF_BYTES`: nor one whose sides are longer than this together, in UTF-16 code units
/// (JavaScript's `length`).
pub const MAX_DIFF_BYTES: usize = 2 * 1024 * 1024;

/// `countLines`: empty text has no lines, any other one more than its newlines.
pub fn count_lines(text: &str) -> usize {
    match text {
        "" => 0,
        text => 1 + text.matches('\n').count(),
    }
}

/// `DiffSize`: both sides of a file's diff, and whether they are past either limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffSize {
    pub lines: usize,
    pub bytes: usize,
    pub oversized: bool,
}

impl DiffSize {
    fn new(lines: usize, bytes: usize) -> Self {
        Self {
            lines,
            bytes,
            oversized: lines > MAX_DIFF_LINES || bytes > MAX_DIFF_BYTES,
        }
    }

    /// `.file-diff-too-large`'s text, its numbers written as `toLocaleString()` writes them
    /// in Electron's locale.
    pub fn notice(&self) -> String {
        let locale = ElectronLocale::current();
        // `Math.round`: halves round up.
        let kilobytes = (self.bytes as f64 / 1024.).round() as u64;
        format!(
            "This diff is large ({} lines, {} KB) and may slow down or crash the app.",
            locale.number(self.lines as u64),
            locale.number(kilobytes)
        )
    }
}

/// `measureDiff` of a file's section: both of its sides, rebuilt from its hunks (the patches
/// carry whole files), counted together.
pub fn measure_diff(section: &str) -> DiffSize {
    let (mut original, mut modified) = (String::new(), String::new());
    // The side(s) the last line went to, whose newline a `\ No newline at end of file` drops.
    let (mut last_original, mut last_modified) = (false, false);
    // Split at `\n` alone: a `\r` before it is the line's own (CRLF files).
    for line in section
        .split('\n')
        .skip_while(|line| !line.starts_with("@@"))
    {
        let (to_original, to_modified) = match line.as_bytes().first() {
            Some(b' ') => (true, true),
            Some(b'-') => (true, false),
            Some(b'+') => (false, true),
            Some(b'\\') => {
                if last_original {
                    original.pop();
                }
                if last_modified {
                    modified.pop();
                }
                (last_original, last_modified) = (false, false);
                continue;
            }
            // A hunk's header.
            Some(_) | None => {
                (last_original, last_modified) = (false, false);
                continue;
            }
        };
        for (to, side) in [(to_original, &mut original), (to_modified, &mut modified)] {
            if to {
                side.push_str(&line[1..]);
                side.push('\n');
            }
        }
        (last_original, last_modified) = (to_original, to_modified);
    }
    DiffSize::new(
        count_lines(&original) + count_lines(&modified),
        original.encode_utf16().count() + modified.encode_utf16().count(),
    )
}

/// What a listed file shows under its header in place of rows (`FileDiffSection`'s body).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Placeholder {
    /// `.file-diff-placeholder`: "Scroll to load diff".
    NotLoaded,
    /// `.file-diff-loading`.
    Loading,
    /// `.file-diff-error`: why its diff could not be read, or shown.
    Failed(String),
    /// An image (`isImagePath`, SVG included): its sides, before and after.
    Image,
    /// "Binary file — diff not shown": git found the section binary (known once parsed).
    Binary,
    /// `.file-diff-too-large`, until "Load anyway".
    TooLarge(DiffSize),
}

/// How a listed file's diff shows, before its section is parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShownDiff<'a> {
    /// Git printed no section for it: it is not in the diff.
    Absent,
    /// Its header ([`bare_header`]) with this under it: no rows to parse, lay out or highlight.
    Placeholder(Placeholder),
    /// Its section, to parse into rows (or a binary file's notice, or why it does not parse).
    Section(&'a str),
}

/// How `path`, whose diff is `diff`, shows. `forced`: "Load anyway" was clicked for it.
pub fn shown_diff<'a>(path: &str, diff: &'a FileDiff, forced: bool) -> ShownDiff<'a> {
    let section = match diff {
        FileDiff::NotLoaded => return ShownDiff::Placeholder(Placeholder::NotLoaded),
        FileDiff::Loading => return ShownDiff::Placeholder(Placeholder::Loading),
        FileDiff::Failed(error) => {
            return ShownDiff::Placeholder(Placeholder::Failed(error.clone()));
        }
        FileDiff::Loaded(section) if section.is_empty() => return ShownDiff::Absent,
        FileDiff::Loaded(_) if is_image_path(path) => {
            return ShownDiff::Placeholder(Placeholder::Image);
        }
        FileDiff::Loaded(section) => section,
    };
    match measure_diff(section) {
        size if size.oversized && !forced => ShownDiff::Placeholder(Placeholder::TooLarge(size)),
        _ => ShownDiff::Section(section),
    }
}

/// A `diff --git` header naming `path` alone, its paths quoted as git quotes them.
pub fn bare_header(path: &str) -> String {
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
    fn each_file_is_asked_for_read_or_failed_on_its_own() {
        let mut diffs = FileDiffs::not_loaded(["a.txt", "b.txt", "a.txt"]);
        assert_eq!(
            diffs.iter().collect::<Vec<_>>(),
            [
                ("a.txt", &FileDiff::NotLoaded),
                ("b.txt", &FileDiff::NotLoaded)
            ]
        );
        assert!(!diffs.is_loading("a.txt") && !diffs.is_complete());
        assert_eq!(diffs.asked().count(), 0);
        assert_eq!(
            diffs.patch(),
            [bare_header("a.txt"), bare_header("b.txt")].concat()
        );

        // Asked for: being read.
        diffs.set("b.txt", FileDiff::Loading);
        assert!(diffs.is_loading("b.txt") && !diffs.is_complete());
        assert_eq!(diffs.asked().collect::<Vec<_>>(), ["b.txt"]);
        assert_eq!(
            diffs.patch(),
            [bare_header("a.txt"), bare_header("b.txt")].concat()
        );

        // A command's sections, and a file it printed nothing for.
        diffs.set_read(["a.txt", "b.txt"], &Ok(MODIFIED.to_owned()));
        assert_eq!(
            diffs.get("a.txt"),
            Some(&FileDiff::Loaded(MODIFIED.to_owned()))
        );
        assert_eq!(diffs.get("b.txt"), Some(&FileDiff::Loaded(String::new())));
        assert!(diffs.is_complete());
        assert_eq!(diffs.patch(), MODIFIED);
        // Its failure fails each file it reads; a path not listed is left out.
        diffs.set_read(["b.txt", "new.txt"], &Err("fatal: bad revision".to_owned()));
        assert_eq!(
            diffs.get("b.txt"),
            Some(&FileDiff::Failed("fatal: bad revision".to_owned()))
        );
        assert_eq!(diffs.get("new.txt"), None);
        assert!(diffs.is_complete());
        assert_eq!(diffs.asked().collect::<Vec<_>>(), ["a.txt", "b.txt"]);
        assert_eq!(diffs.patch(), [MODIFIED, &bare_header("b.txt")].concat());

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

    #[test]
    fn a_reload_keeps_the_files_asked_for_on_their_own_since_it_listed_them() {
        let section = |path: &str, text: &str| MODIFIED.replace("a.txt", path).replace("ONE", text);
        // Shown while the reload ran: a.txt read before it (so read again by it); b.txt, c.txt
        // and d.txt asked for on their own since it listed them (read, being read, failed);
        // e.txt not asked for; gone.txt no longer listed.
        let mut shown =
            FileDiffs::not_loaded(["a.txt", "b.txt", "c.txt", "d.txt", "e.txt", "gone.txt"]);
        shown.set("a.txt", FileDiff::Loaded(section("a.txt", "OLD")));
        shown.set("b.txt", FileDiff::Loaded(section("b.txt", "OWN")));
        shown.set("c.txt", FileDiff::Loading);
        shown.set("d.txt", FileDiff::failed("fatal: bad object"));
        shown.set("gone.txt", FileDiff::Loaded(section("gone.txt", "GONE")));

        // The reload lists every file but gone.txt, plus new.txt, and reads a.txt again.
        let mut reloaded =
            FileDiffs::not_loaded(["a.txt", "b.txt", "c.txt", "d.txt", "e.txt", "new.txt"]);
        reloaded.set_read(["a.txt"], &Ok(section("a.txt", "NEW")));
        assert_eq!(
            reloaded.keep_asked_since(&shown),
            ["b.txt", "c.txt", "d.txt"]
        );
        assert_eq!(
            reloaded.iter().collect::<Vec<_>>(),
            [
                ("a.txt", &FileDiff::Loaded(section("a.txt", "NEW"))),
                ("b.txt", &FileDiff::Loaded(section("b.txt", "OWN"))),
                ("c.txt", &FileDiff::Loading),
                ("d.txt", &FileDiff::Failed("fatal: bad object".to_owned())),
                ("e.txt", &FileDiff::NotLoaded),
                ("new.txt", &FileDiff::NotLoaded),
            ]
        );

        // What the reload read (or failed to read) stands over what was shown.
        let mut failed = FileDiffs::not_loaded(["a.txt", "b.txt"]);
        failed.set_read(["a.txt", "b.txt"], &Err("fatal: bad revision".to_owned()));
        assert!(failed.keep_asked_since(&shown).is_empty());
        assert_eq!(
            failed.iter().collect::<Vec<_>>(),
            [
                ("a.txt", &FileDiff::Failed("fatal: bad revision".to_owned())),
                ("b.txt", &FileDiff::Failed("fatal: bad revision".to_owned())),
            ]
        );
    }

    #[test]
    fn empty_text_has_no_lines_and_any_other_one_more_than_its_newlines() {
        assert_eq!(count_lines(""), 0);
        assert_eq!(count_lines("a"), 1);
        assert_eq!(count_lines("a\n"), 2);
        assert_eq!(count_lines("a\nb"), 2);
        assert_eq!(count_lines("\n\n"), 3);
    }

    /// `path`'s section with one hunk of `lines` (each given its newline); the hunk header
    /// is not counted.
    fn section(path: &str, header: &str, lines: &[String]) -> String {
        let mut section =
            format!("diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n{header}\n");
        for line in lines {
            section.push_str(line);
            section.push('\n');
        }
        section
    }

    fn size(lines: usize, bytes: usize) -> DiffSize {
        DiffSize::new(lines, bytes)
    }

    #[test]
    fn a_diff_is_measured_on_both_sides_rebuilt_from_its_hunks() {
        // `one\n` and `ONE\n`: two lines and four characters each.
        assert_eq!(measure_diff(MODIFIED), size(4, 8));
        // An added file has no original side; nor a deleted one a modified side.
        assert_eq!(measure_diff(ADDED), size(2, 4));
        assert_eq!(measure_diff(DELETED), size(2, 4));
        // No hunks: nothing to draw.
        assert_eq!(measure_diff(BINARY), size(0, 0));
        assert_eq!(measure_diff(RENAMED), size(0, 0));
        // Header lines are not counted; context is on both sides, hunk headers on neither.
        let two_hunks = "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1,2 +1,2 @@\n same\n-a\n+bb\n@@ -9 +9 @@\n-c\n";
        assert_eq!(measure_diff(two_hunks), size(4 + 3, 9 + 8));
        // `\ No newline at end of file` drops the newline of the side(s) of the line before it.
        let no_newline = "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n\\ No newline at end of file\n+b\n";
        assert_eq!(measure_diff(no_newline), size(1 + 2, 1 + 2));
        let unchanged_end = "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1,2 +1,2 @@\n-a\n+b\n end\n\\ No newline at end of file\n";
        assert_eq!(measure_diff(unchanged_end), size(2 + 2, 5 + 5));
        // A line's `\r` is its own.
        assert_eq!(
            measure_diff(&section("x", "@@ -0,0 +1 @@", &["+a\r".to_owned()])),
            size(2, 3)
        );
    }

    #[test]
    fn a_diff_past_twenty_thousand_lines_or_two_mebibytes_is_oversized() {
        // 10 000 lines rewritten: 10 001 counted on each side.
        let rewritten: Vec<String> = (0..10_000)
            .map(|n| format!("-old {n}"))
            .chain((0..10_000).map(|n| format!("+new {n}")))
            .collect();
        let measured = measure_diff(&section("x", "@@ -1,10000 +1,10000 @@", &rewritten));
        assert_eq!((measured.lines, measured.oversized), (20_002, true));
        // 20 000 counted lines are within the limit, 20 001 past it.
        let added = |count: usize| measure_diff(&section("x", "@@", &vec!["+".to_owned(); count]));
        assert_eq!(
            (added(19_999).lines, added(19_999).oversized),
            (20_000, false)
        );
        assert_eq!(
            (added(20_000).lines, added(20_000).oversized),
            (20_001, true)
        );
        // One long line: counted in UTF-16 code units, as JavaScript's `length`.
        let long = |text: &str| {
            measure_diff(&section(
                "x",
                "@@",
                &[format!("+{}", text.repeat(1_100_000))],
            ))
        };
        assert_eq!(long("\u{1F600}"), size(2, 2_200_001), "two units each");
        assert!(long("\u{1F600}").oversized);
        assert_eq!(
            long("\u{e9}"),
            size(2, 1_100_001),
            "one unit, though two UTF-8 bytes"
        );
        assert!(!long("\u{e9}").oversized);
    }

    #[test]
    fn the_notice_gives_lines_and_rounded_kilobytes_in_electrons_locale() {
        // `toLocaleString()` in Electron's locale (`25,001` in en-US, `25.001` in German; see
        // `ElectronLocale::number`).
        let notice = |lines: u64, kilobytes: u64| {
            let locale = ElectronLocale::current();
            format!(
                "This diff is large ({} lines, {} KB) and may slow down or crash the app.",
                locale.number(lines),
                locale.number(kilobytes)
            )
        };
        assert_eq!(size(25_001, 1_536_000).notice(), notice(25_001, 1_500));
        // Half a kilobyte rounds up.
        assert_eq!(size(1, 512).notice(), notice(1, 1));
        assert_eq!(size(0, 511).notice(), notice(0, 0));
    }

    #[test]
    fn each_file_shows_its_section_or_what_stands_in_for_it() {
        let loaded = |section: &str| FileDiff::Loaded(section.to_owned());
        assert_eq!(
            shown_diff("a.txt", &loaded(MODIFIED), false),
            ShownDiff::Section(MODIFIED)
        );
        // A binary section is parsed: the view tells it from its section.
        let data = BINARY.replace("logo.png", "data.bin");
        assert_eq!(
            shown_diff("data.bin", &loaded(&data), false),
            ShownDiff::Section(&data)
        );
        // Git printed nothing for it.
        assert_eq!(shown_diff("a.txt", &loaded(""), false), ShownDiff::Absent);
        // Not asked for, loading, or failed to read (an image too).
        assert_eq!(
            shown_diff("logo.png", &FileDiff::NotLoaded, false),
            ShownDiff::Placeholder(Placeholder::NotLoaded)
        );
        assert_eq!(
            shown_diff("a.txt", &FileDiff::Loading, false),
            ShownDiff::Placeholder(Placeholder::Loading)
        );
        assert_eq!(
            shown_diff(
                "a.txt",
                &FileDiff::Failed("fatal: bad object".to_owned()),
                false
            ),
            ShownDiff::Placeholder(Placeholder::Failed("fatal: bad object".to_owned()))
        );
        // Images show as images, an SVG git diffs as text too, oversized or not.
        assert_eq!(
            shown_diff("logo.png", &loaded(BINARY), false),
            ShownDiff::Placeholder(Placeholder::Image)
        );
        let svg = MODIFIED.replace("a.txt", "icons/Logo.SVG");
        assert_eq!(
            shown_diff("icons/Logo.SVG", &loaded(&svg), false),
            ShownDiff::Placeholder(Placeholder::Image)
        );
        let large = |path: &str| {
            section(
                path,
                "@@ -0,0 +1,20000 @@",
                &vec!["+line".to_owned(); 20_000],
            )
        };
        assert_eq!(
            shown_diff("big.svg", &loaded(&large("big.svg")), false),
            ShownDiff::Placeholder(Placeholder::Image)
        );
        // Too large to draw, until "Load anyway".
        let big = loaded(&large("big.txt"));
        assert_eq!(
            shown_diff("big.txt", &big, false),
            ShownDiff::Placeholder(Placeholder::TooLarge(size(20_001, 100_000)))
        );
        assert_eq!(
            shown_diff("big.txt", &big, true),
            ShownDiff::Section(&large("big.txt"))
        );
        // Within the limits, "Load anyway" changes nothing.
        assert_eq!(
            shown_diff("a.txt", &loaded(MODIFIED), true),
            ShownDiff::Section(MODIFIED)
        );
    }
}
