//! `RemoteDirectoryBrowser.tsx`: walk a daemon host's directories one level at a time. Open
//! Workspace browses a remote session with it, and SSH Upload picks its destination.

use gpui_kit::{Context, EventEmitter, Task};
use treeterm_core::fs::FsClient;
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

    /// "..".
    pub fn go_up(&mut self, cx: &mut Context<Self>) {
        self.navigate(parent_of(&self.current), cx);
    }

    /// `handlePathSubmit`: the typed path, trimmed, when it is absolute.
    pub fn submit(&mut self, typed: &str, cx: &mut Context<Self>) {
        let typed = typed.trim();
        if typed.starts_with('/') {
            self.navigate(typed.to_owned(), cx);
        }
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
    let read = fs.read_directory("/", path);
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
    use super::{Crumb, parent_of};

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
