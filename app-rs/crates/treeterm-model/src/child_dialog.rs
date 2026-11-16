//! "Add Child Workspace" (`CreateChildDialog.tsx`): its tabs, the list each one reads, the
//! picked row and the submit. The view keeps the text fields and renders this.
//!
//! Electron renders each picker tab as a loader component that mounts when the tab is
//! entered: every visit reads its list afresh (retrying a failed read), and leaving the tab
//! cancels the read, so a late result or error never lands on another tab. Here the shown
//! tab's read is one owned task; entering another tab replaces (drops, so cancels) it.
//!
//! Opening an existing or recent worktree is awaited in the dialog (`isProcessing`): its
//! controls are disabled, with a message, until the worktree is open (the dialog closes) or
//! the error shows.

use std::collections::HashMap;

use gpui_kit::{App, Context, Entity, EventEmitter, Subscription, Task};
use treeterm_core::git::WorktreeInfo;
use treeterm_core::github::{HeadRepo, PrListItem};
use treeterm_core::worktree_registry::WorktreeRegistryEntry;

use crate::session_store::{AdoptOptions, ChildOptions, ChildSource, SessionStore};

/// `title` of a disabled branch row.
pub const IN_WORKTREE_TOOLTIP: &str = "Branch is already in a worktree";
/// `title` of a disabled pull request row.
pub const FORK_TOOLTIP: &str = "PR is from a fork \u{2014} open it manually instead";

/// Electron's `TabMode`, in tab order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChildTab {
    Create,
    Existing,
    Recent,
    Branch,
    Remote,
    Pr,
}

impl ChildTab {
    pub const ALL: [ChildTab; 6] =
        [ChildTab::Create, ChildTab::Existing, ChildTab::Recent, ChildTab::Branch, ChildTab::Remote, ChildTab::Pr];

    /// The tab's label; Electron's 400px dialog wraps each onto two lines.
    pub fn label(self) -> [&'static str; 2] {
        match self {
            ChildTab::Create => ["Create", "New"],
            ChildTab::Existing => ["Open", "Existing"],
            ChildTab::Recent => ["Recent", "Workspace"],
            ChildTab::Branch => ["Open", "Branch"],
            ChildTab::Remote => ["Open", "Remote"],
            ChildTab::Pr => ["Open", "PR"],
        }
    }
}

/// A picker tab's list: `Loading` while its read runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Listing<T> {
    Loading,
    Loaded(Vec<T>),
}

/// A local or remote branch and whether it already has a worktree (`BranchInfo`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchItem {
    pub name: String,
    pub in_worktree: bool,
}

/// A worktree of the parent's repository, with what the registry remembers of it.
#[derive(Debug, Clone, PartialEq)]
pub struct WorktreeItem {
    pub info: WorktreeInfo,
    pub entry: Option<WorktreeRegistryEntry>,
}

/// A remembered worktree that still exists.
#[derive(Debug, Clone, PartialEq)]
pub struct RecentItem {
    pub info: WorktreeInfo,
    pub entry: WorktreeRegistryEntry,
}

/// Whether a row can be picked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowState {
    Available,
    /// Greyed, with a badge and a tooltip saying why.
    Unavailable { badge: &'static str, tooltip: &'static str },
}

/// One row of `.create-child-existing-list`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildRow {
    pub key: String,
    pub name: String,
    /// The accent second line (`.worktree-branch`).
    pub branch: Option<String>,
    /// The dimmed second line of a Recent row.
    pub description: Option<String>,
    pub state: RowState,
}

/// What picking a row writes into the Name and Description fields (`None`: left as is).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Fill {
    pub name: Option<String>,
    pub description: Option<String>,
}

/// "Use custom default application" and its select (`useCustomSettings`, `selectedAppId`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppChoice {
    /// Unchecked: the child inherits its default application.
    Inherited,
    /// Checked, with the picked application (`None`: "Select an application...").
    Custom(Option<String>),
}

/// What a submit did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Submitted {
    /// Nothing: a worktree is being opened already, or Enter found nothing to submit.
    Ignored,
    /// Refused: its error shows, and the dialog stays as it was.
    Refused,
    /// The child was added or its creation started (`Created` follows), or the picked
    /// worktree is being opened ([`ChildDialogModel::processing`]).
    Started,
}

/// What the dialog tells its view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChildDialogEvent {
    /// The child was added, or its creation has started: the dialog closes and shows it.
    Created(String),
    /// The parent is no longer loaded: the dialog closes (Electron renders it only while its
    /// parent is).
    ParentGone,
}

/// Opening an existing or recent worktree (`isProcessing`, `processingMessage`).
enum Adopting {
    Idle,
    /// The task waits for the adopt, which runs on in the session if the dialog goes away.
    Waiting { message: &'static str, _wait: Task<()> },
}

/// What Open Existing and Recent Workspace say.
#[derive(Clone, Copy)]
struct AdoptTexts {
    unpicked: &'static str,
    processing: &'static str,
    failed: &'static str,
}

const EXISTING: AdoptTexts = AdoptTexts {
    unpicked: "Please select a worktree",
    processing: "Opening existing worktree...",
    failed: "Failed to open worktree",
};

const RECENT: AdoptTexts = AdoptTexts {
    unpicked: "Please select a recent workspace",
    processing: "Opening recent workspace...",
    failed: "Failed to open recent workspace",
};

/// An item a row is made from.
trait Pickable: Clone {
    /// The row's key.
    fn key(&self) -> String;
    /// Whether the row can be picked (Electron's `onClick` guard).
    fn available(&self) -> bool {
        true
    }
}

impl Pickable for WorktreeItem {
    fn key(&self) -> String {
        self.info.path.clone()
    }
}

impl Pickable for RecentItem {
    fn key(&self) -> String {
        self.info.path.clone()
    }
}

impl Pickable for BranchItem {
    fn key(&self) -> String {
        self.name.clone()
    }
    fn available(&self) -> bool {
        !self.in_worktree
    }
}

impl Pickable for PrListItem {
    fn key(&self) -> String {
        self.number.to_string()
    }
    fn available(&self) -> bool {
        self.head_repo != HeadRepo::Fork
    }
}

/// One picker tab: its list and the picked item. The pick is the item itself, as Electron
/// keeps the selected object, so it outlives a re-read of the list.
struct Picker<T> {
    listing: Listing<T>,
    picked: Option<T>,
}

impl<T: Pickable> Picker<T> {
    fn new() -> Self {
        Self { listing: Listing::Loading, picked: None }
    }

    fn loaded(&self) -> Option<&[T]> {
        match &self.listing {
            Listing::Loading => None,
            Listing::Loaded(list) => Some(list),
        }
    }

    /// Pick the listed item at `key`, if it can be picked.
    fn pick(&mut self, key: &str) -> Option<&T> {
        let item = self.loaded()?.iter().find(|item| item.key() == key && item.available())?.clone();
        self.picked = Some(item);
        self.picked.as_ref()
    }
}

/// What a finished read stores in the model.
type Apply = Box<dyn FnOnce(&mut ChildDialogModel)>;

pub struct ChildDialogModel {
    session: Entity<SessionStore>,
    parent_id: String,
    tab: ChildTab,
    existing: Picker<WorktreeItem>,
    recent: Picker<RecentItem>,
    branches: Picker<BranchItem>,
    remotes: Picker<BranchItem>,
    prs: Picker<PrListItem>,
    detached: bool,
    app: AppChoice,
    error: Option<String>,
    adopting: Adopting,
    /// The shown tab's read. Replacing it drops the read of the tab left, so its rows and
    /// error never land (Electron's `cancelled` guards).
    _load: Option<Task<()>>,
    /// The parent's name, its inherited application and the open worktrees follow the
    /// session, and the dialog goes when the parent does.
    _session: Subscription,
}

impl EventEmitter<ChildDialogEvent> for ChildDialogModel {}

impl ChildDialogModel {
    pub fn new(session: Entity<SessionStore>, parent_id: String, tab: ChildTab, cx: &mut Context<Self>) -> Self {
        let observe = cx.observe(&session, |model, session, cx| {
            if session.read(cx).workspace_store(&model.parent_id).is_none() {
                cx.emit(ChildDialogEvent::ParentGone);
            }
            cx.notify();
        });
        let mut model = Self {
            session,
            parent_id,
            tab,
            existing: Picker::new(),
            recent: Picker::new(),
            branches: Picker::new(),
            remotes: Picker::new(),
            prs: Picker::new(),
            detached: false,
            app: AppChoice::Inherited,
            error: None,
            adopting: Adopting::Idle,
            _load: None,
            _session: observe,
        };
        model.enter(tab, cx);
        model
    }

    pub fn tab(&self) -> ChildTab {
        self.tab
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// While an existing or recent worktree is being opened: what the dialog says
    /// (`processingMessage`). Its controls are disabled meanwhile.
    pub fn processing(&self) -> Option<&'static str> {
        match &self.adopting {
            Adopting::Idle => None,
            Adopting::Waiting { message, .. } => Some(*message),
        }
    }

    pub fn detached(&self) -> bool {
        self.detached
    }

    pub fn set_detached(&mut self, detached: bool, cx: &mut Context<Self>) {
        self.detached = detached;
        cx.notify();
    }

    pub fn app_choice(&self) -> &AppChoice {
        &self.app
    }

    pub fn set_app_choice(&mut self, app: AppChoice, cx: &mut Context<Self>) {
        self.app = app;
        cx.notify();
    }

    /// "Parent:" (`parentWsData.name`), read live.
    pub fn parent_name(&self, cx: &App) -> String {
        self.session
            .read(cx)
            .workspace_store(&self.parent_id)
            .map(|w| w.read(cx).file().name.clone())
            .unwrap_or_default()
    }

    fn git_root(&self, cx: &App) -> Option<String> {
        let session = self.session.read(cx);
        let parent = session.workspace_store(&self.parent_id)?;
        parent.read(cx).file().git_root_path.clone()
    }

    fn open_paths(&self, cx: &App) -> Vec<String> {
        let session = self.session.read(cx);
        session
            .workspace_ids()
            .filter_map(|id| session.workspace_store(id))
            .map(|w| w.read(cx).workspace().path.clone())
            .collect()
    }

    /// A tab click (`setMode` + `setError(null)`, and the pick reset of Recent, Branch, Remote
    /// and PR; Existing keeps its pick). Another tab is entered and reads its list; the tab
    /// already shown is not (Electron does not remount its loader, so the list stays). Returns
    /// whether another tab is now shown, so the view starts a fresh search box.
    pub fn switch(&mut self, tab: ChildTab, cx: &mut Context<Self>) -> bool {
        self.error = None;
        match tab {
            ChildTab::Create | ChildTab::Existing => {}
            ChildTab::Recent => self.recent.picked = None,
            ChildTab::Branch => self.branches.picked = None,
            ChildTab::Remote => self.remotes.picked = None,
            ChildTab::Pr => self.prs.picked = None,
        }
        let entered = tab != self.tab;
        if entered {
            self.enter(tab, cx);
        }
        cx.notify();
        entered
    }

    /// Show `tab` and read its list again (Electron's loader mounting).
    fn enter(&mut self, tab: ChildTab, cx: &mut Context<Self>) {
        self.tab = tab;
        self._load = None;
        match tab {
            ChildTab::Create => return,
            ChildTab::Existing => self.existing.listing = Listing::Loading,
            ChildTab::Recent => self.recent.listing = Listing::Loading,
            ChildTab::Branch => self.branches.listing = Listing::Loading,
            ChildTab::Remote => self.remotes.listing = Listing::Loading,
            ChildTab::Pr => self.prs.listing = Listing::Loading,
        }
        match self.git_root(cx) {
            Some(root) => self._load = Some(self.read(tab, root, cx)),
            None => {
                self.error = Some("Parent workspace is not a git repository".to_owned());
                emptied(tab)(self);
            }
        }
    }

    /// Read `tab`'s list (the same git, registry and GitHub calls as Electron's loaders).
    fn read(&self, tab: ChildTab, root: String, cx: &mut Context<Self>) -> Task<()> {
        let session = self.session.read(cx);
        let git = session.connection().git();
        let registry = session.worktree_registry();
        let github = session.github_api();
        cx.spawn(async move |this, cx| {
            let failed = |what: &str, error: String| Some(format!("Failed to load {what}: {error}"));
            let (error, apply): (Option<String>, Apply) = match tab {
                ChildTab::Create => return,
                ChildTab::Branch => match (git.list_local_branches(&root).await, git.get_branches_in_worktrees(&root).await) {
                    (Ok(local), Ok(taken)) => {
                        let list = local.into_iter().map(|name| BranchItem { in_worktree: taken.contains(&name), name }).collect();
                        (None, Box::new(move |m| m.branches.listing = Listing::Loaded(list)))
                    }
                    (Err(e), _) | (_, Err(e)) => (failed("branches", e.to_string()), emptied(tab)),
                },
                ChildTab::Remote => {
                    let remote = git.list_remote_branches(&root).await;
                    let taken = git.get_branches_in_worktrees(&root).await;
                    let local = git.list_local_branches(&root).await;
                    match (remote, taken, local) {
                        (Ok(remote), Ok(taken), Ok(local)) => {
                            let list = remote
                                .into_iter()
                                .map(|name| {
                                    let local_name = name.split('/').skip(1).collect::<Vec<_>>().join("/");
                                    let in_worktree =
                                        taken.contains(&name) || taken.contains(&local_name) || local.contains(&local_name);
                                    BranchItem { name, in_worktree }
                                })
                                .collect();
                            (None, Box::new(move |m| m.remotes.listing = Listing::Loaded(list)))
                        }
                        (Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => {
                            (failed("remote branches", e.to_string()), emptied(tab))
                        }
                    }
                }
                ChildTab::Existing | ChildTab::Recent => {
                    let worktrees = git.list_worktrees(&root).await;
                    let entries = match &registry {
                        Some(registry) => registry.list().await.map_err(|e| e.to_string()),
                        None => Ok(Vec::new()),
                    };
                    match (tab, worktrees, entries) {
                        // A failed registry read is reported even when the worktree list
                        // failed too (its own `.catch`).
                        (ChildTab::Existing, worktrees, entries) => {
                            let error = entries.as_ref().err().map(|e| format!("Failed to load worktree registry: {e}"));
                            let by_path: HashMap<String, WorktreeRegistryEntry> =
                                entries.unwrap_or_default().into_iter().map(|e| (e.path.clone(), e)).collect();
                            let worktrees = match worktrees {
                                Ok(worktrees) => worktrees,
                                // Electron shows this as an empty list, with no message.
                                Err(error) => {
                                    tracing::warn!(%error, "cannot list the parent's worktrees");
                                    Vec::new()
                                }
                            };
                            let list = worktrees
                                .into_iter()
                                .map(|info| WorktreeItem { entry: by_path.get(&info.path).cloned(), info })
                                .collect();
                            (error, Box::new(move |m| m.existing.listing = Listing::Loaded(list)))
                        }
                        (_, Ok(worktrees), Ok(entries)) => {
                            let by_path: HashMap<String, WorktreeInfo> =
                                worktrees.into_iter().map(|w| (w.path.clone(), w)).collect();
                            let mut list: Vec<RecentItem> = entries
                                .into_iter()
                                .filter_map(|entry| by_path.get(&entry.path).cloned().map(|info| RecentItem { info, entry }))
                                .collect();
                            list.sort_by_key(|item| std::cmp::Reverse(item.entry.last_used_at));
                            (None, Box::new(move |m| m.recent.listing = Listing::Loaded(list)))
                        }
                        (_, Err(e), _) => (failed("recent workspaces", e.to_string()), emptied(tab)),
                        (_, _, Err(e)) => (failed("recent workspaces", e), emptied(tab)),
                    }
                }
                ChildTab::Pr => {
                    match github.list_open_prs(&root).await.map_err(|e| e.to_string()) {
                        Ok(list) => (None, Box::new(move |m| m.prs.listing = Listing::Loaded(list))),
                        Err(e) => (failed("pull requests", e), emptied(tab)),
                    }
                }
            };
            // Err: the model is gone (its task would have been dropped with it).
            match this.update(cx, |model, cx| {
                apply(model);
                if error.is_some() {
                    model.error = error;
                }
                cx.notify();
            }) {
                Ok(()) | Err(_) => {}
            }
        })
    }

    /// The shown tab's rows for the search text `query`.
    pub fn rows(&self, query: &str, cx: &App) -> Listing<ChildRow> {
        let query = query.trim().to_lowercase();
        let rows = match self.tab {
            ChildTab::Create => Some(Vec::new()),
            ChildTab::Existing => self.existing.loaded().map(|list| existing_rows(list, &self.open_paths(cx))),
            ChildTab::Recent => self.recent.loaded().map(|list| recent_rows(list, &self.open_paths(cx))),
            ChildTab::Branch => self.branches.loaded().map(|list| branch_rows(list, &query)),
            ChildTab::Remote => self.remotes.loaded().map(|list| branch_rows(list, &query)),
            ChildTab::Pr => self.prs.loaded().map(|list| pr_rows(list, &query)),
        };
        match rows {
            Some(rows) => Listing::Loaded(rows),
            None => Listing::Loading,
        }
    }

    /// The shown tab's picked row.
    pub fn selected_key(&self) -> Option<String> {
        match self.tab {
            ChildTab::Create => None,
            ChildTab::Existing => self.existing.picked.as_ref().map(Pickable::key),
            ChildTab::Recent => self.recent.picked.as_ref().map(Pickable::key),
            ChildTab::Branch => self.branches.picked.as_ref().map(Pickable::key),
            ChildTab::Remote => self.remotes.picked.as_ref().map(Pickable::key),
            ChildTab::Pr => self.prs.picked.as_ref().map(Pickable::key),
        }
    }

    /// Pick the shown tab's row at `key`. The worktree tabs fill in the name and description
    /// they remember: Existing only the empty fields, Recent both (`onSelect`).
    pub fn pick(&mut self, key: &str, name: &str, description: &str, cx: &mut Context<Self>) -> Fill {
        let fill = match self.tab {
            ChildTab::Create => Fill::default(),
            ChildTab::Existing => match self.existing.pick(key) {
                Some(WorktreeItem { entry: Some(entry), .. }) => Fill {
                    name: entry.display_name.clone().filter(|n| !n.is_empty() && name.trim().is_empty()),
                    description: entry.description.clone().filter(|d| !d.is_empty() && description.trim().is_empty()),
                },
                Some(WorktreeItem { entry: None, .. }) | None => Fill::default(),
            },
            ChildTab::Recent => match self.recent.pick(key) {
                Some(RecentItem { info, entry }) => Fill {
                    name: Some(entry.display_name.clone().unwrap_or_else(|| info.branch.clone())),
                    description: Some(entry.description.clone().unwrap_or_default()),
                },
                None => Fill::default(),
            },
            ChildTab::Branch => {
                self.branches.pick(key);
                Fill::default()
            }
            ChildTab::Remote => {
                self.remotes.pick(key);
                Fill::default()
            }
            ChildTab::Pr => {
                self.prs.pick(key);
                Fill::default()
            }
        };
        cx.notify();
        fill
    }

    /// Whether the main button is disabled (processing, nothing picked, or an invalid name).
    pub fn submit_disabled(&self, name: &str) -> bool {
        self.processing().is_some()
            || match self.tab {
                ChildTab::Create => name_problem(name).is_some(),
                _ => self.selected_key().is_none(),
            }
    }

    /// The main button's label.
    pub fn submit_label(&self) -> &'static str {
        match (self.tab, self.processing()) {
            (ChildTab::Create, None) => "Create",
            (ChildTab::Create, Some(_)) => "Creating... Please wait",
            (_, None) => "Open",
            (_, Some(_)) => "Opening... Please wait",
        }
    }

    /// Whether Enter submits (`handleKeyDown`): never while processing; on Create always, its
    /// checks showing their errors; on a picker tab only once a row is picked (Electron's
    /// `&& selectedX` guards: no "Please select ..." then).
    pub fn enter_submits(&self) -> bool {
        self.processing().is_none()
            && match self.tab {
                ChildTab::Create => true,
                ChildTab::Existing | ChildTab::Recent | ChildTab::Branch | ChildTab::Remote | ChildTab::Pr => {
                    self.selected_key().is_some()
                }
            }
    }

    /// Enter anywhere in the dialog: the tab's submit when [`Self::enter_submits`], else
    /// nothing.
    pub fn press_enter(&mut self, name: &str, description: &str, cx: &mut Context<Self>) -> Submitted {
        if !self.enter_submits() {
            return Submitted::Ignored;
        }
        self.submit(name, description, cx)
    }

    /// The tab's submit handler. A new child reports `Created` once its creation has started;
    /// an existing or recent worktree once it is open, while [`Self::processing`] shows.
    /// Otherwise the error shows. Ignored while processing.
    pub fn submit(&mut self, name: &str, description: &str, cx: &mut Context<Self>) -> Submitted {
        if self.processing().is_some() {
            return Submitted::Ignored;
        }
        let name = name.trim().to_owned();
        let description = Some(description.trim().to_owned()).filter(|d| !d.is_empty());
        let options = child_options(self.tab, self.detached, description, &self.app);
        let parent = self.parent_id.clone();
        let source = match self.tab {
            ChildTab::Create => {
                if name.is_empty() {
                    return self.fail("Please enter a workspace name", cx);
                }
                if let Some(problem) = name_problem(&name) {
                    return self.fail(problem, cx);
                }
                ChildSource::NewBranch { name }
            }
            ChildTab::Existing => {
                let picked = self.existing.picked.as_ref().map(|w| w.info.clone());
                return self.adopt(picked, name, options, EXISTING, cx);
            }
            ChildTab::Recent => {
                let picked = self.recent.picked.as_ref().map(|r| r.info.clone());
                return self.adopt(picked, name, options, RECENT, cx);
            }
            ChildTab::Branch => match &self.branches.picked {
                Some(branch) => ChildSource::ExistingBranch { branch: branch.name.clone() },
                None => return self.fail("Please select a branch", cx),
            },
            ChildTab::Remote => match &self.remotes.picked {
                Some(remote) => ChildSource::RemoteBranch { remote_branch: remote.name.clone() },
                None => return self.fail("Please select a remote branch", cx),
            },
            ChildTab::Pr => match &self.prs.picked {
                Some(pr) => ChildSource::PullRequest { head_ref: pr.head_ref_name.clone(), title: pr.title.clone() },
                None => return self.fail("Please select a pull request", cx),
            },
        };
        match self.session.update(cx, |s, cx| s.create_child(&parent, source, options, cx)) {
            Ok(id) => {
                cx.emit(ChildDialogEvent::Created(id));
                Submitted::Started
            }
            Err(error) => self.fail(error, cx),
        }
    }

    /// Open the picked worktree (`handleAdoptSubmit`, `handleRecentSubmit`): named after its
    /// branch, with the typed name as its display name when that differs, and the dialog's
    /// description and default application. Waits for it with the tab's message; a failure
    /// shows and the controls come back for another try.
    fn adopt(
        &mut self,
        picked: Option<WorktreeInfo>,
        name: String,
        options: ChildOptions,
        texts: AdoptTexts,
        cx: &mut Context<Self>,
    ) -> Submitted {
        let Some(info) = picked else {
            return self.fail(texts.unpicked, cx);
        };
        let adopt = AdoptOptions {
            display_name: Some(name).filter(|name| !name.is_empty() && *name != info.branch),
            description: options.description,
            default_application_id: options.default_application_id,
        };
        let parent = self.parent_id.clone();
        let opened = self.session.update(cx, |s, cx| s.adopt_worktree(&parent, info, adopt, cx));
        let wait = cx.spawn(async move |this, cx| {
            // Canceled: the adopt went away with the session. (Its errors are never empty, so
            // Electron's `result.error || '…'` fallback shows only then.)
            let outcome = opened.await.unwrap_or_else(|_| Err(texts.failed.to_owned()));
            // Err: the dialog is gone (its task would have been dropped with it).
            match this.update(cx, |model, cx| {
                model.adopting = Adopting::Idle;
                match outcome {
                    Ok(id) => cx.emit(ChildDialogEvent::Created(id)),
                    Err(error) => model.error = Some(error),
                }
                cx.notify();
            }) {
                Ok(()) | Err(_) => {}
            }
        });
        self.error = None;
        self.adopting = Adopting::Waiting { message: texts.processing, _wait: wait };
        cx.notify();
        Submitted::Started
    }

    fn fail(&mut self, message: impl Into<String>, cx: &mut Context<Self>) -> Submitted {
        self.error = Some(message.into());
        cx.notify();
        Submitted::Refused
    }

    /// `getInheritedDefaultApplicationIds`: the parent's own default application, else its
    /// ancestors' — the first that exists, by name.
    pub fn inherited_app(&self, cx: &App) -> Option<String> {
        let session = self.session.read(cx);
        let mut id = Some(self.parent_id.clone());
        let mut seen = 0;
        while let Some(current) = id {
            seen += 1;
            if seen > 64 {
                return None;
            }
            let file = session.workspace_store(&current)?.read(cx).file();
            let app = &file.settings.default_application_id;
            if !app.is_empty()
                && let Some(def) = session.registry().get(app)
            {
                return Some(def.name.clone());
            }
            id = file.parent_id.clone().filter(|p| !p.is_empty());
        }
        None
    }

    /// The applications "Default Application" offers, as `(id, name)` (`showInNewTabMenu`).
    pub fn available_apps(&self, cx: &App) -> Vec<(String, String)> {
        self.session.read(cx).registry().new_tab_menu().map(|app| (app.id.clone(), app.name.clone())).collect()
    }
}

/// `nameValidationError`.
pub fn name_problem(name: &str) -> Option<&'static str> {
    let name = name.trim();
    (!name.is_empty() && name.contains('/')).then_some("Name cannot contain \"/\" - use simple names only")
}

/// The options a tab creates its child with. The PR tab sends no description: it hides the
/// field, and the workspace takes the PR's title (`onCreateFromPr`).
fn child_options(tab: ChildTab, detached: bool, description: Option<String>, app: &AppChoice) -> ChildOptions {
    ChildOptions {
        detached,
        description: description.filter(|_| tab != ChildTab::Pr),
        default_application_id: match app {
            AppChoice::Custom(Some(id)) => Some(id.clone()),
            AppChoice::Custom(None) | AppChoice::Inherited => None,
        },
    }
}

/// Stores an empty list for `tab` (its read failed).
fn emptied(tab: ChildTab) -> Apply {
    Box::new(move |m: &mut ChildDialogModel| match tab {
        ChildTab::Create => {}
        ChildTab::Existing => m.existing.listing = Listing::Loaded(Vec::new()),
        ChildTab::Recent => m.recent.listing = Listing::Loaded(Vec::new()),
        ChildTab::Branch => m.branches.listing = Listing::Loaded(Vec::new()),
        ChildTab::Remote => m.remotes.listing = Listing::Loaded(Vec::new()),
        ChildTab::Pr => m.prs.listing = Listing::Loaded(Vec::new()),
    })
}

/// Open Existing: every worktree not open already, named as remembered.
fn existing_rows(list: &[WorktreeItem], open: &[String]) -> Vec<ChildRow> {
    list.iter()
        .filter(|w| !open.contains(&w.info.path))
        .map(|w| ChildRow {
            key: w.key(),
            name: w.entry.as_ref().and_then(|e| e.display_name.clone()).unwrap_or_else(|| w.info.branch.clone()),
            branch: Some(w.info.branch.clone()),
            description: None,
            state: RowState::Available,
        })
        .collect()
}

/// Recent Workspace: the description shows only when it is not empty.
fn recent_rows(list: &[RecentItem], open: &[String]) -> Vec<ChildRow> {
    list.iter()
        .filter(|r| !open.contains(&r.info.path))
        .map(|r| ChildRow {
            key: r.key(),
            name: r.entry.display_name.clone().unwrap_or_else(|| r.info.branch.clone()),
            branch: Some(r.info.branch.clone()),
            description: r.entry.description.clone().filter(|d| !d.is_empty()),
            state: RowState::Available,
        })
        .collect()
}

/// Open Branch and Open Remote: a branch that has a worktree cannot be picked.
fn branch_rows(list: &[BranchItem], query: &str) -> Vec<ChildRow> {
    list.iter()
        .filter(|b| query.is_empty() || b.name.to_lowercase().contains(query))
        .map(|b| ChildRow {
            key: b.key(),
            name: b.name.clone(),
            branch: None,
            description: None,
            state: if b.available() {
                RowState::Available
            } else {
                RowState::Unavailable { badge: "In Worktree", tooltip: IN_WORKTREE_TOOLTIP }
            },
        })
        .collect()
}

/// Open PR: searched by title, branch and number; a fork's PR cannot be picked.
fn pr_rows(list: &[PrListItem], query: &str) -> Vec<ChildRow> {
    list.iter()
        .filter(|pr| {
            query.is_empty()
                || pr.title.to_lowercase().contains(query)
                || pr.head_ref_name.to_lowercase().contains(query)
                || pr.number.to_string().contains(query)
        })
        .map(|pr| ChildRow {
            key: pr.key(),
            name: format!("#{} {}", pr.number, pr.title),
            branch: Some(if pr.author.is_empty() {
                pr.head_ref_name.clone()
            } else {
                format!("{} \u{b7} {}", pr.head_ref_name, pr.author)
            }),
            description: None,
            state: if pr.available() {
                RowState::Available
            } else {
                RowState::Unavailable { badge: "Fork", tooltip: FORK_TOOLTIP }
            },
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, description: Option<&str>) -> WorktreeRegistryEntry {
        WorktreeRegistryEntry {
            path: path.to_owned(),
            branch: "b".to_owned(),
            display_name: None,
            description: description.map(str::to_owned),
            last_used_at: 1,
            extra: Default::default(),
        }
    }

    fn pr(number: u64, head_repo: HeadRepo) -> PrListItem {
        PrListItem {
            number,
            title: format!("PR {number}"),
            author: "octo".to_owned(),
            head_ref_name: format!("feature-{number}"),
            head_repo,
        }
    }

    #[test]
    fn the_pr_tab_sends_no_description() {
        let description = Some("typed on another tab".to_owned());
        let pr = child_options(ChildTab::Pr, true, description.clone(), &AppChoice::Inherited);
        assert_eq!(pr, ChildOptions { detached: true, description: None, default_application_id: None });
        let branch = child_options(ChildTab::Branch, false, description.clone(), &AppChoice::Custom(Some("x".into())));
        assert_eq!(branch.description, description);
        assert_eq!(branch.default_application_id.as_deref(), Some("x"));
        let unpicked = child_options(ChildTab::Create, false, None, &AppChoice::Custom(None));
        assert_eq!(unpicked.default_application_id, None);
    }

    #[test]
    fn a_fork_pr_is_unavailable_with_its_tooltip() {
        let rows = pr_rows(&[pr(1, HeadRepo::Origin), pr(2, HeadRepo::Fork)], "");
        assert_eq!(rows[0].state, RowState::Available);
        assert_eq!(rows[0].branch.as_deref(), Some("feature-1 \u{b7} octo"));
        assert_eq!(rows[1].state, RowState::Unavailable { badge: "Fork", tooltip: FORK_TOOLTIP });
        assert_eq!(FORK_TOOLTIP, "PR is from a fork — open it manually instead");
        assert_eq!(pr_rows(&[pr(1, HeadRepo::Origin), pr(22, HeadRepo::Fork)], "22").len(), 1);
    }

    #[test]
    fn a_remote_branch_with_a_worktree_is_unavailable_with_its_tooltip() {
        let list = [
            BranchItem { name: "origin/main".into(), in_worktree: true },
            BranchItem { name: "origin/free".into(), in_worktree: false },
        ];
        let rows = branch_rows(&list, "");
        assert_eq!(rows[0].state, RowState::Unavailable { badge: "In Worktree", tooltip: IN_WORKTREE_TOOLTIP });
        assert_eq!(rows[1].state, RowState::Available);
        assert_eq!(branch_rows(&list, "fre").len(), 1);
    }

    #[test]
    fn a_recent_row_shows_only_a_non_empty_description() {
        let item = |path: &str, description| RecentItem {
            info: WorktreeInfo { path: path.to_owned(), branch: "b".to_owned() },
            entry: entry(path, description),
        };
        let list = [item("/a", Some("")), item("/b", None), item("/c", Some("notes")), item("/open", Some("x"))];
        let rows = recent_rows(&list, &["/open".to_owned()]);
        let descriptions: Vec<_> = rows.iter().map(|r| r.description.as_deref()).collect();
        assert_eq!(descriptions, [None, None, Some("notes")]);
    }

    #[test]
    fn a_picked_item_outlives_a_re_read_and_unavailable_rows_cannot_be_picked() {
        let mut picker = Picker::new();
        assert!(picker.pick("free").is_none(), "nothing listed yet");
        picker.listing = Listing::Loaded(vec![
            BranchItem { name: "taken".into(), in_worktree: true },
            BranchItem { name: "free".into(), in_worktree: false },
        ]);
        assert!(picker.pick("taken").is_none());
        assert_eq!(picker.pick("free").map(Pickable::key).as_deref(), Some("free"));
        picker.listing = Listing::Loading;
        assert_eq!(picker.picked.as_ref().map(Pickable::key).as_deref(), Some("free"));
    }

    #[test]
    fn a_name_with_a_slash_is_refused() {
        assert_eq!(name_problem("  "), None);
        assert_eq!(name_problem("ok"), None);
        assert_eq!(name_problem("a/b"), Some("Name cannot contain \"/\" - use simple names only"));
    }
}
