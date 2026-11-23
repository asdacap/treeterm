//! Hidden entries: those whose name starts with `.`. The daemon lists every entry of a
//! directory; which of them show is up to whoever shows the listing.

use treeterm_proto::treeterm::FileEntry;

/// Whether a listing shows the entries whose name starts with `.`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HiddenEntries {
    Skip,
    Include,
}

impl HiddenEntries {
    /// Whether `entry` shows in a listing that makes this choice.
    pub fn shows(self, entry: &FileEntry) -> bool {
        match self {
            HiddenEntries::Include => true,
            HiddenEntries::Skip => !entry.name.starts_with('.'),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(name: &str) -> FileEntry {
        FileEntry { name: name.to_owned(), ..FileEntry::default() }
    }

    #[test]
    fn skipping_hides_only_names_that_start_with_a_dot() {
        assert!(!HiddenEntries::Skip.shows(&named(".env")));
        assert!(!HiddenEntries::Skip.shows(&named(".git")));
        assert!(HiddenEntries::Skip.shows(&named("a.txt")));
        assert!(HiddenEntries::Skip.shows(&named("dir.d")));
    }

    #[test]
    fn including_shows_everything() {
        assert!(HiddenEntries::Include.shows(&named(".env")));
        assert!(HiddenEntries::Include.shows(&named("a.txt")));
    }
}
