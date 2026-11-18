//! `RemoteDirectoryBrowser.tsx`: walk a daemon host's directories one level at a time. Open
//! Workspace browses a remote session with it, and SSH Upload picks its destination.

use gpui_kit::{Context, EventEmitter, Task};
use treeterm_core::fs::{FsClient, HiddenEntries};
use treeterm_proto::treeterm::FileEntry;

use crate::file_browser::fs_outcome;

/// What the current directory holds.
#[derive(Debug, Clone, PartialEq)]
pub enum Listing {
    Loading,
    /// Its subdirectories, in the daemon's order (hidden ones skipped, by name).
    Loaded(Vec<FileEntry>),
    /// Why it could not be listed.
    Failed(String),
}

/// A breadcrumb segment: `label` leads to `target`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Crumb {
    pub label: String,
    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirBrowserEvent {
    /// The browser moved to this directory (`navigateTo`): the path field shows it.
    Navigated(String),
}

/// What committing the typed path did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Commit {
    /// The browser went to the typed path: the listing and breadcrumbs were replaced.
    Moved,
    /// Nothing to commit: the typed path is not absolute, or is the directory shown.
    Stayed,
}

pub struct DirBrowser {
    fs: FsClient,
    current: String,
    listing: Listing,
    /// The read of `current`; replacing it drops an older read, so a slow one never
    /// overwrites a later directory's listing.
    _load: Task<()>,
}

impl EventEmitter<DirBrowserEvent> for DirBrowser {}

/// `handleGoUp`: `/a/b/c` → `/a/b`; `/a` → `/`.
pub fn parent_of(path: &str) -> String {
    match path.trim_end_matches('/').rsplit_once('/') {
        Some(("", _)) | None => "/".to_owned(),
        Some((parent, _)) => parent.to_owned(),
    }
}

/// The typed path the path field commits when it loses focus (`handlePathSubmit` on
/// blur): trimmed and absolute, and not the directory already shown.
fn pending_path(typed: &str, current: &str) -> Option<String> {
    let typed = typed.trim();
    (typed.starts_with('/') && typed != current).then(|| typed.to_owned())
}

impl DirBrowser {
    /// Browse `fs`'s host from `start` (`initialPath`).
    pub fn new(fs: FsClient, start: String, cx: &mut Context<Self>) -> Self {
        let load = load(&fs, &start, cx);
        Self {
            fs,
            current: start,
            listing: Listing::Loading,
            _load: load,
        }
    }

    pub fn current(&self) -> &str {
        &self.current
    }

    pub fn listing(&self) -> &Listing {
        &self.listing
    }

    /// ".." is disabled at the root.
    pub fn at_root(&self) -> bool {
        self.current == "/"
    }

    /// `navigateTo`: show "Loading..." until `path` is listed.
    pub fn navigate(&mut self, path: String, cx: &mut Context<Self>) {
        self._load = load(&self.fs, &path, cx);
        self.listing = Listing::Loading;
        self.current = path.clone();
        cx.emit(DirBrowserEvent::Navigated(path));
        cx.notify();
    }

    /// `handlePathSubmit` on Enter: the typed path, trimmed, when it is absolute. It is read
    /// again even when it is the directory shown.
    pub fn submit(&mut self, typed: &str, cx: &mut Context<Self>) {
        let typed = typed.trim();
        if typed.starts_with('/') {
            self.navigate(typed.to_owned(), cx);
        }
    }

    /// The path field lost focus (`onBlur={handlePathSubmit}`): go to the typed path. A
    /// relative path, or the directory already shown, is left alone (and not read again).
    ///
    /// Pressing one of the browser's buttons takes the focus from the field, so the
    /// operations below commit the typed path first, as Electron's blur does before the
    /// click lands.
    pub fn commit(&mut self, typed: &str, cx: &mut Context<Self>) -> Commit {
        match pending_path(typed, &self.current) {
            Some(path) => {
                self.navigate(path, cx);
                Commit::Moved
            }
            None => Commit::Stayed,
        }
    }

    /// "..", from the typed path when there is one. It is disabled at the root.
    pub fn go_up(&mut self, typed: &str, cx: &mut Context<Self>) {
        self.commit(typed, cx);
        if !self.at_root() {
            self.navigate(parent_of(&self.current), cx);
        }
    }

    /// A breadcrumb leading to `target`. When the typed path moved the browser, the
    /// breadcrumbs were rebuilt under the pointer: the click lands only if `target` is
    /// still a link there (an ancestor of the new directory).
    pub fn open_crumb(&mut self, typed: &str, target: String, cx: &mut Context<Self>) {
        let lands = match self.commit(typed, cx) {
            Commit::Stayed => true,
            Commit::Moved => {
                let crumbs = self.crumbs();
                crumbs[..crumbs.len() - 1]
                    .iter()
                    .any(|crumb| crumb.target == target)
            }
        };
        if lands {
            self.navigate(target, cx);
        }
    }

    /// A directory of the listing. When the typed path moved the browser, the listing it
    /// was in is gone and the click does nothing.
    pub fn open_entry(&mut self, typed: &str, path: String, cx: &mut Context<Self>) {
        match self.commit(typed, cx) {
            Commit::Stayed => self.navigate(path, cx),
            Commit::Moved => {}
        }
    }

    /// "Select": the directory to select (`onSelect(currentPath)`), the typed one when
    /// there is one.
    pub fn select(&mut self, typed: &str, cx: &mut Context<Self>) -> String {
        self.commit(typed, cx);
        self.current.clone()
    }

    /// The breadcrumbs; the last one is the current directory.
    pub fn crumbs(&self) -> Vec<Crumb> {
        crumbs_of(&self.current)
    }
}

/// `/`, then each segment of `path` leading to the directory up to it.
fn crumbs_of(path: &str) -> Vec<Crumb> {
    let segments: Vec<&str> = std::iter::once("/")
        .chain(path.split('/').filter(|s| !s.is_empty()))
        .collect();
    segments
        .iter()
        .enumerate()
        .map(|(i, label)| Crumb {
            label: (*label).to_owned(),
            target: match i {
                0 => "/".to_owned(),
                _ => format!("/{}", segments[1..=i].join("/")),
            },
        })
        .collect()
}

/// List `path`'s subdirectories (the scope is the whole host, as Electron's `"/"`).
fn load(fs: &FsClient, path: &str, cx: &mut Context<DirBrowser>) -> Task<()> {
    let read = fs.read_directory("/", path, HiddenEntries::Skip);
    cx.spawn(async move |this, cx| {
        let listing = match fs_outcome(read.await) {
            Ok(contents) => Listing::Loaded(
                contents
                    .entries
                    .into_iter()
                    .filter(|e| e.is_directory)
                    .collect(),
            ),
            Err(error) => Listing::Failed(error.to_string()),
        };
        match this.update(cx, |browser, cx| {
            browser.listing = listing;
            cx.notify();
        }) {
            Ok(()) | Err(_) => {} // Err: the browser closed
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{Crumb, parent_of, pending_path};

    #[test]
    fn a_typed_path_is_pending_when_absolute_and_elsewhere() {
        assert_eq!(pending_path("/srv/app", "/srv/app"), None);
        assert_eq!(
            pending_path("  /srv/app ", "/srv"),
            Some("/srv/app".to_owned())
        );
        assert_eq!(pending_path(" /srv ", "/srv"), None);
        assert_eq!(pending_path("srv/app", "/"), None);
        assert_eq!(pending_path("   ", "/"), None);
        assert_eq!(pending_path("/", "/x"), Some("/".to_owned()));
        assert_eq!(pending_path("/x", "/"), Some("/x".to_owned()));
    }

    #[test]
    fn parents_stop_at_the_root() {
        assert_eq!(parent_of("/home/me/src"), "/home/me");
        assert_eq!(parent_of("/home"), "/");
        assert_eq!(parent_of("/"), "/");
        assert_eq!(parent_of("/home/me/"), "/home");
    }

    #[test]
    fn crumbs_lead_back_up_to_the_root() {
        let crumb = |label: &str, target: &str| Crumb {
            label: label.to_owned(),
            target: target.to_owned(),
        };
        assert_eq!(super::crumbs_of("/"), vec![crumb("/", "/")]);
        assert_eq!(
            super::crumbs_of("/home/me/src"),
            vec![
                crumb("/", "/"),
                crumb("home", "/home"),
                crumb("me", "/home/me"),
                crumb("src", "/home/me/src")
            ]
        );
        // Repeated slashes collapse.
        assert_eq!(
            super::crumbs_of("/srv//app/"),
            vec![
                crumb("/", "/"),
                crumb("srv", "/srv"),
                crumb("app", "/srv/app")
            ]
        );
    }
}
