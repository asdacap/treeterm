//! "Add Child Workspace". Port of `CreateChildDialog.tsx`: a new branch from the parent's
//! current branch, an existing or recently used worktree, an existing local or remote
//! branch, or an open pull request — each a tab, opened on "Open Branch" as Electron's
//! session panel opens it.

use std::collections::HashMap;
use std::rc::Rc;

use gpui_kit::component::input::{InputEvent, InputState, TextareaState};
use gpui_kit::component::menu::DropdownMenu as _;
use gpui_kit::component::{WindowExt as _, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, Context, Entity, FontWeight, SharedString, Subscription, Window, div, px, rgb, rgba};
use treeterm_core::git::WorktreeInfo;
use treeterm_core::github::{HeadRepo, PrListItem};
use treeterm_core::worktree_registry::WorktreeRegistryEntry;
use treeterm_model::session_store::{ChildOptions, ChildSource, SessionStore};

use crate::theme::colors;
use crate::ui;

/// Called with the new child's id once creation has started.
pub type OnCreated = Rc<dyn Fn(String, &mut Window, &mut App)>;

/// Electron's `TabMode`, in tab order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Mode {
    Create,
    Existing,
    Recent,
    Branch,
    Remote,
    Pr,
}

impl Mode {
    const ALL: [Mode; 6] = [Mode::Create, Mode::Existing, Mode::Recent, Mode::Branch, Mode::Remote, Mode::Pr];

    /// The tab's label; Electron's 400px dialog wraps each onto two lines.
    fn label(self) -> [&'static str; 2] {
        match self {
            Mode::Create => ["Create", "New"],
            Mode::Existing => ["Open", "Existing"],
            Mode::Recent => ["Recent", "Workspace"],
            Mode::Branch => ["Open", "Branch"],
            Mode::Remote => ["Open", "Remote"],
            Mode::Pr => ["Open", "PR"],
        }
    }
}

/// A list read for one of the picker tabs (each loads when first shown, as Electron's
/// loaders mount).
enum Listing<T> {
    Loading,
    Loaded(Vec<T>),
}

/// A local or remote branch and whether it already has a worktree (`BranchInfo`).
#[derive(Clone)]
struct Branch {
    name: String,
    in_worktree: bool,
}

#[derive(Clone)]
struct Worktree {
    info: WorktreeInfo,
    entry: Option<WorktreeRegistryEntry>,
}

/// What a finished load stores in the dialog.
type Apply = Box<dyn FnOnce(&mut ChildDialog)>;

/// One row of `.create-child-existing-list`.
struct Row {
    key: String,
    name: String,
    branch: Option<String>,
    description: Option<String>,
    badge: Option<&'static str>,
    disabled: bool,
}

pub struct ChildDialog {
    session: Entity<SessionStore>,
    parent_id: String,
    parent_name: String,
    mode: Mode,
    name: Entity<InputState>,
    description: Entity<TextareaState>,
    search: Entity<InputState>,
    detached: bool,
    settings_expanded: bool,
    use_custom_app: bool,
    custom_app: Option<String>,
    worktrees: Listing<Worktree>,
    recent: Listing<Worktree>,
    branches: Listing<Branch>,
    remotes: Listing<Branch>,
    prs: Listing<PrListItem>,
    /// The picked row's key, per tab.
    selected: HashMap<Mode, String>,
    error: Option<SharedString>,
    on_created: OnCreated,
    _subscriptions: Vec<Subscription>,
}

impl ChildDialog {
    fn new(
        session: Entity<SessionStore>,
        parent_id: String,
        on_created: OnCreated,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let parent_name = session
            .read(cx)
            .workspace_store(&parent_id)
            .map(|w| w.read(cx).file().name.clone())
            .unwrap_or_default();
        let name = cx.new(|cx| InputState::new(window, cx).placeholder("Workspace name..."));
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search branches..."));
        let description =
            cx.new(|cx| TextareaState::new(window, cx).auto_grow(2, 12).placeholder("Optional description..."));
        let on_input = |dialog: &mut Self, event: &InputEvent, window: &mut Window, cx: &mut Context<Self>| match event {
            // `handleKeyDown`: Enter submits.
            InputEvent::PressEnter { .. } => dialog.submit(window, cx),
            InputEvent::Change => cx.notify(),
            InputEvent::Focus | InputEvent::Blur => {}
        };
        let subscriptions = vec![
            cx.subscribe_in(&name, window, move |dialog, _, event, window, cx| on_input(dialog, event, window, cx)),
            cx.subscribe_in(&search, window, move |dialog, _, event, window, cx| on_input(dialog, event, window, cx)),
        ];
        let mut dialog = Self {
            session,
            parent_id,
            parent_name,
            mode: Mode::Branch,
            name,
            description,
            search,
            detached: false,
            settings_expanded: false,
            use_custom_app: false,
            custom_app: None,
            worktrees: Listing::Loading,
            recent: Listing::Loading,
            branches: Listing::Loading,
            remotes: Listing::Loading,
            prs: Listing::Loading,
            selected: HashMap::new(),
            error: None,
            on_created,
            _subscriptions: subscriptions,
        };
        dialog.load(Mode::Branch, cx);
        dialog
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

    /// Read `mode`'s list if it has not been read yet.
    fn load(&mut self, mode: Mode, cx: &mut Context<Self>) {
        let loading = match mode {
            Mode::Create => return,
            Mode::Existing => matches!(self.worktrees, Listing::Loading),
            Mode::Recent => matches!(self.recent, Listing::Loading),
            Mode::Branch => matches!(self.branches, Listing::Loading),
            Mode::Remote => matches!(self.remotes, Listing::Loading),
            Mode::Pr => matches!(self.prs, Listing::Loading),
        };
        if !loading {
            return;
        }
        let Some(root) = self.git_root(cx) else {
            self.error = Some("Parent workspace is not a git repository".into());
            self.finish(mode, cx);
            return;
        };
        let session = self.session.read(cx);
        let git = session.connection().git();
        let registry = session.worktree_registry();
        let github = session.github_api();
        cx.spawn(async move |this, cx| {
            let failed = |what: &str, error: String| Some(SharedString::from(format!("Failed to load {what}: {error}")));
            let (error, apply): (Option<SharedString>, Apply) = match mode {
                Mode::Create => return,
                Mode::Branch => match (git.list_local_branches(&root).await, git.get_branches_in_worktrees(&root).await) {
                    (Ok(local), Ok(taken)) => {
                        let list = local.into_iter().map(|name| Branch { in_worktree: taken.contains(&name), name }).collect();
                        (None, Box::new(move |d| d.branches = Listing::Loaded(list)))
                    }
                    (Err(e), _) | (_, Err(e)) => {
                        (failed("branches", e.to_string()), Box::new(|d| d.branches = Listing::Loaded(Vec::new())))
                    }
                },
                Mode::Remote => {
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
                                    Branch { name, in_worktree }
                                })
                                .collect();
                            (None, Box::new(move |d| d.remotes = Listing::Loaded(list)))
                        }
                        (Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => (
                            failed("remote branches", e.to_string()),
                            Box::new(|d| d.remotes = Listing::Loaded(Vec::new())),
                        ),
                    }
                }
                Mode::Existing | Mode::Recent => {
                    let worktrees = git.list_worktrees(&root).await;
                    let entries = match &registry {
                        Some(registry) => registry.list().await.map_err(|e| e.to_string()),
                        None => Ok(Vec::new()),
                    };
                    match (mode, worktrees, entries) {
                        (Mode::Existing, Ok(worktrees), entries) => {
                            let error = entries.as_ref().err().map(|e| SharedString::from(format!("Failed to load worktree registry: {e}")));
                            let by_path: HashMap<String, WorktreeRegistryEntry> =
                                entries.unwrap_or_default().into_iter().map(|e| (e.path.clone(), e)).collect();
                            let list = worktrees
                                .into_iter()
                                .map(|info| Worktree { entry: by_path.get(&info.path).cloned(), info })
                                .collect();
                            (error, Box::new(move |d| d.worktrees = Listing::Loaded(list)))
                        }
                        (Mode::Existing, Err(_), _) => (None, Box::new(|d| d.worktrees = Listing::Loaded(Vec::new()))),
                        (_, Ok(worktrees), Ok(entries)) => {
                            let by_path: HashMap<String, WorktreeInfo> =
                                worktrees.into_iter().map(|w| (w.path.clone(), w)).collect();
                            let mut list: Vec<Worktree> = entries
                                .into_iter()
                                .filter_map(|entry| {
                                    by_path.get(&entry.path).cloned().map(|info| Worktree { info, entry: Some(entry) })
                                })
                                .collect();
                            list.sort_by_key(|w| std::cmp::Reverse(w.entry.as_ref().map_or(0, |e| e.last_used_at)));
                            (None, Box::new(move |d| d.recent = Listing::Loaded(list)))
                        }
                        (_, Err(e), _) => (
                            failed("recent workspaces", e.to_string()),
                            Box::new(|d| d.recent = Listing::Loaded(Vec::new())),
                        ),
                        (_, _, Err(e)) => {
                            (failed("recent workspaces", e), Box::new(|d| d.recent = Listing::Loaded(Vec::new())))
                        }
                    }
                }
                Mode::Pr => {
                    let listed = match github {
                        Some(github) => github.list_open_prs(&root).await.map_err(|e| e.to_string()),
                        None => Err("GitHub is not available".to_owned()),
                    };
                    match listed {
                        Ok(list) => (None, Box::new(move |d| d.prs = Listing::Loaded(list))),
                        Err(e) => (failed("pull requests", e), Box::new(|d| d.prs = Listing::Loaded(Vec::new()))),
                    }
                }
            };
            match this.update(cx, |dialog, cx| {
                apply(dialog);
                if error.is_some() {
                    dialog.error = error;
                }
                cx.notify();
            }) {
                Ok(()) | Err(_) => {} // Err: the dialog closed
            }
        })
        .detach();
    }

    /// Mark `mode`'s list loaded (empty) when it cannot be read.
    fn finish(&mut self, mode: Mode, cx: &mut Context<Self>) {
        match mode {
            Mode::Create => {}
            Mode::Existing => self.worktrees = Listing::Loaded(Vec::new()),
            Mode::Recent => self.recent = Listing::Loaded(Vec::new()),
            Mode::Branch => self.branches = Listing::Loaded(Vec::new()),
            Mode::Remote => self.remotes = Listing::Loaded(Vec::new()),
            Mode::Pr => self.prs = Listing::Loaded(Vec::new()),
        }
        cx.notify();
    }

    fn switch(&mut self, mode: Mode, window: &mut Window, cx: &mut Context<Self>) {
        self.mode = mode;
        self.error = None;
        // Electron resets the pick when entering these tabs, and each search box is new.
        if matches!(mode, Mode::Recent | Mode::Branch | Mode::Remote | Mode::Pr) {
            self.selected.remove(&mode);
        }
        let (name_placeholder, search_placeholder) = match mode {
            Mode::Existing => ("Leave empty to use branch name...", ""),
            Mode::Remote => ("Workspace name...", "Search remote branches..."),
            Mode::Pr => ("Workspace name...", "Search pull requests..."),
            Mode::Create | Mode::Recent | Mode::Branch => ("Workspace name...", "Search branches..."),
        };
        self.name.update(cx, |input, cx| input.set_placeholder(name_placeholder, window, cx));
        self.search.update(cx, |input, cx| {
            input.set_placeholder(search_placeholder, window, cx);
            input.set_value("", window, cx);
        });
        // `autoFocus` on the Create tab's name.
        if mode == Mode::Create {
            self.name.update(cx, |input, cx| input.focus(window, cx));
        }
        self.load(mode, cx);
        cx.notify();
    }

    fn name_text(&self, cx: &App) -> String {
        self.name.read(cx).value().trim().to_owned()
    }

    /// `nameValidationError`.
    fn name_problem(&self, cx: &App) -> Option<&'static str> {
        let name = self.name_text(cx);
        (!name.is_empty() && name.contains('/')).then_some("Name cannot contain \"/\" - use simple names only")
    }

    fn rows(&self, cx: &App) -> Option<Vec<Row>> {
        let query = self.search.read(cx).value().trim().to_lowercase();
        let open = self.open_paths(cx);
        let branch_rows = |list: &Vec<Branch>| {
            list.iter()
                .filter(|b| query.is_empty() || b.name.to_lowercase().contains(&query))
                .map(|b| Row {
                    key: b.name.clone(),
                    name: b.name.clone(),
                    branch: None,
                    description: None,
                    badge: b.in_worktree.then_some("In Worktree"),
                    disabled: b.in_worktree,
                })
                .collect()
        };
        let worktree_rows = |list: &Vec<Worktree>, with_description: bool| {
            list.iter()
                .filter(|w| !open.contains(&w.info.path))
                .map(|w| Row {
                    key: w.info.path.clone(),
                    name: w.entry.as_ref().and_then(|e| e.display_name.clone()).unwrap_or_else(|| w.info.branch.clone()),
                    branch: Some(w.info.branch.clone()),
                    description: if with_description { w.entry.as_ref().and_then(|e| e.description.clone()) } else { None },
                    badge: None,
                    disabled: false,
                })
                .collect()
        };
        match self.mode {
            Mode::Create => None,
            Mode::Existing => match &self.worktrees {
                Listing::Loading => None,
                Listing::Loaded(list) => Some(worktree_rows(list, false)),
            },
            Mode::Recent => match &self.recent {
                Listing::Loading => None,
                Listing::Loaded(list) => Some(worktree_rows(list, true)),
            },
            Mode::Branch => match &self.branches {
                Listing::Loading => None,
                Listing::Loaded(list) => Some(branch_rows(list)),
            },
            Mode::Remote => match &self.remotes {
                Listing::Loading => None,
                Listing::Loaded(list) => Some(branch_rows(list)),
            },
            Mode::Pr => match &self.prs {
                Listing::Loading => None,
                Listing::Loaded(list) => Some(
                    list.iter()
                        .filter(|pr| {
                            query.is_empty()
                                || pr.title.to_lowercase().contains(&query)
                                || pr.head_ref_name.to_lowercase().contains(&query)
                                || pr.number.to_string().contains(&query)
                        })
                        .map(|pr| {
                            let fork = pr.head_repo == HeadRepo::Fork;
                            Row {
                                key: pr.number.to_string(),
                                name: format!("#{} {}", pr.number, pr.title),
                                branch: Some(if pr.author.is_empty() {
                                    pr.head_ref_name.clone()
                                } else {
                                    format!("{} · {}", pr.head_ref_name, pr.author)
                                }),
                                description: None,
                                badge: fork.then_some("Fork"),
                                disabled: fork,
                            }
                        })
                        .collect(),
                ),
            },
        }
    }

    /// The picked worktree of the Existing or Recent tab.
    fn picked_worktree(&self) -> Option<&Worktree> {
        let list = match (self.mode, &self.worktrees, &self.recent) {
            (Mode::Existing, Listing::Loaded(list), _) | (Mode::Recent, _, Listing::Loaded(list)) => list,
            _ => return None,
        };
        let key = self.selected.get(&self.mode)?;
        list.iter().find(|w| &w.info.path == key)
    }

    /// Picking a row; the worktree tabs fill in the name and description they remember.
    fn pick(&mut self, key: String, window: &mut Window, cx: &mut Context<Self>) {
        self.selected.insert(self.mode, key);
        if let Some(entry) = self.picked_worktree().and_then(|w| w.entry.clone()) {
            let branch = self.picked_worktree().map(|w| w.info.branch.clone()).unwrap_or_default();
            let (name, description) = match self.mode {
                // `onSelect` of the Existing tab only fills what is empty.
                Mode::Existing => (
                    entry.display_name.filter(|_| self.name_text(cx).is_empty()),
                    entry.description.filter(|_| self.description.read(cx).value().trim().is_empty()),
                ),
                _ => (Some(entry.display_name.unwrap_or(branch)), Some(entry.description.unwrap_or_default())),
            };
            if let Some(name) = name {
                self.name.update(cx, |input, cx| input.set_value(name, window, cx));
            }
            if let Some(description) = description {
                self.description.update(cx, |input, cx| input.set_value(description, window, cx));
            }
        }
        cx.notify();
    }

    /// Whether the main button is disabled (nothing picked, or an invalid name).
    fn submit_disabled(&self, cx: &App) -> bool {
        match self.mode {
            Mode::Create => self.name_problem(cx).is_some(),
            mode => !self.selected.contains_key(&mode),
        }
    }

    /// The tab's submit handler; closes the dialog once creation has started.
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let description = self.description.read(cx).value().trim().to_owned();
        let description = (!description.is_empty()).then_some(description);
        let default_application_id = self.custom_app.clone().filter(|_| self.use_custom_app);
        let options = ChildOptions { detached: self.detached, description: description.clone(), default_application_id };
        let parent = self.parent_id.clone();
        let source = match self.mode {
            Mode::Create => {
                let name = self.name_text(cx);
                if name.is_empty() {
                    return self.fail("Please enter a workspace name", cx);
                }
                if let Some(problem) = self.name_problem(cx) {
                    return self.fail(problem, cx);
                }
                ChildSource::NewBranch { name }
            }
            Mode::Existing | Mode::Recent => {
                let Some(picked) = self.picked_worktree().cloned() else {
                    return self.fail(
                        if self.mode == Mode::Existing { "Please select a worktree" } else { "Please select a recent workspace" },
                        cx,
                    );
                };
                // `onAdopt`: the typed name becomes the display name when it differs.
                let name = self.name_text(cx);
                let mut metadata = std::collections::BTreeMap::new();
                metadata.insert("branchIsUserDefined".to_owned(), "true".to_owned());
                if let Some(description) = description {
                    metadata.insert("description".to_owned(), description);
                }
                if !name.is_empty() && name != picked.info.branch {
                    metadata.insert("displayName".to_owned(), name);
                }
                let result = self.session.update(cx, |s, cx| s.adopt_worktree_with(&parent, picked.info, metadata, cx));
                return self.started(result, window, cx);
            }
            Mode::Branch => match self.selected.get(&Mode::Branch) {
                Some(branch) => ChildSource::ExistingBranch { branch: branch.clone() },
                None => return self.fail("Please select a branch", cx),
            },
            Mode::Remote => match self.selected.get(&Mode::Remote) {
                Some(remote_branch) => ChildSource::RemoteBranch { remote_branch: remote_branch.clone() },
                None => return self.fail("Please select a remote branch", cx),
            },
            Mode::Pr => {
                let picked = match (&self.prs, self.selected.get(&Mode::Pr)) {
                    (Listing::Loaded(list), Some(key)) => list.iter().find(|pr| &pr.number.to_string() == key),
                    _ => None,
                };
                match picked {
                    Some(pr) => ChildSource::PullRequest { head_ref: pr.head_ref_name.clone(), title: pr.title.clone() },
                    None => return self.fail("Please select a pull request", cx),
                }
            }
        };
        let result = self.session.update(cx, |s, cx| s.create_child(&parent, source, options, cx));
        self.started(result, window, cx);
    }

    fn started(&mut self, result: Result<String, String>, window: &mut Window, cx: &mut Context<Self>) {
        match result {
            Ok(id) => {
                window.close_dialog(cx);
                (self.on_created)(id, window, cx);
            }
            Err(error) => self.fail(error, cx),
        }
    }

    fn fail(&mut self, message: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.error = Some(message.into());
        cx.notify();
    }

    /// `.create-child-tabs`: 0 20px padding, a bottom border; tabs 12px 16px, the active
    /// one primary with a 2px accent underline.
    fn tabs(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = colors(cx);
        let (primary, secondary, accent) = (c.text_primary, c.text_secondary, c.accent);
        let mut row = div().flex().px(px(20.)).border_b_1().border_color(c.border);
        for mode in Mode::ALL {
            let active = self.mode == mode;
            let [first, second] = mode.label();
            row = row.child(
                div()
                    .id(SharedString::from(format!("child-tab-{}", first.to_owned() + second)))
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .items_center()
                    .px(px(16.))
                    .pt(px(12.))
                    .pb(px(10.))
                    .border_b_2()
                    .text_size(px(ui::BODY))
                    .line_height(px(15.5))
                    .cursor_pointer()
                    .when(active, |d| d.border_color(accent).text_color(primary))
                    .when(!active, |d| d.border_color(gpui_kit::transparent_black()).text_color(secondary).hover(move |d| d.text_color(primary)))
                    .child(first)
                    .child(second)
                    .on_click(cx.listener(move |dialog, _, window, cx| dialog.switch(mode, window, cx))),
            );
        }
        row.into_any_element()
    }

    /// `.create-child-existing-list`: the picker's rows, or its loading/empty message.
    fn list(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = colors(cx);
        let (loading, empty) = match self.mode {
            Mode::Create => return div().into_any_element(),
            Mode::Existing => ("Loading worktrees...", "No available child worktrees found"),
            Mode::Recent => ("Loading recent workspaces...", "No recent workspaces found"),
            Mode::Branch | Mode::Remote | Mode::Pr => {
                let searching = !self.search.read(cx).value().trim().is_empty();
                match (self.mode, searching) {
                    (Mode::Pr, true) => ("Loading pull requests...", "No pull requests match your search"),
                    (Mode::Pr, false) => ("Loading pull requests...", "No open pull requests found"),
                    (Mode::Remote, true) => ("Loading remote branches...", "No branches match your search"),
                    (Mode::Remote, false) => ("Loading remote branches...", "No remote branches found"),
                    (_, true) => ("Loading branches...", "No branches match your search"),
                    (_, false) => ("Loading branches...", "No local branches found"),
                }
            }
        };
        let message = |text: &'static str| {
            div()
                .p(px(16.))
                .flex()
                .justify_center()
                .text_size(px(ui::BODY))
                .text_color(c.text_secondary)
                .child(text)
                .into_any_element()
        };
        let body: Vec<AnyElement> = match self.rows(cx) {
            None => vec![message(loading)],
            Some(rows) if rows.is_empty() => vec![message(empty)],
            Some(rows) => {
                let count = rows.len();
                let selected = self.selected.get(&self.mode).cloned();
                let (hover, active) = (c.bg_hover, c.bg_active);
                rows.into_iter()
                    .enumerate()
                    .map(|(index, row)| {
                        let is_selected = selected.as_deref() == Some(row.key.as_str());
                        let key = row.key.clone();
                        div()
                            .id(SharedString::from(format!("child-row-{index}")))
                            .flex()
                            .justify_between()
                            .items_center()
                            .px(px(12.))
                            .py(px(10.))
                            .when(index + 1 < count, |d| d.border_b_1().border_color(c.border))
                            .when(is_selected, |d| d.bg(active))
                            .when(row.disabled, |d| d.opacity(0.5).cursor_not_allowed())
                            .when(!row.disabled, |d| {
                                d.cursor_pointer()
                                    .when(!is_selected, |d| d.hover(move |d| d.bg(hover)))
                                    .on_click(cx.listener(move |dialog, _, window, cx| dialog.pick(key.clone(), window, cx)))
                            })
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(ui::BODY))
                                    .line_height(px(15.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(c.text_primary)
                                    .child(row.name),
                            )
                            .when_some(row.branch, |d, branch| {
                                d.child(
                                    div()
                                        .max_w(px(180.))
                                        .truncate()
                                        .text_size(px(ui::SMALL))
                                        .text_color(c.accent)
                                        .child(branch),
                                )
                            })
                            .when_some(row.description, |d, description| {
                                d.child(
                                    div()
                                        .max_w(px(180.))
                                        .truncate()
                                        .opacity(0.7)
                                        .text_size(px(ui::SMALL))
                                        .text_color(c.accent)
                                        .child(description),
                                )
                            })
                            .when_some(row.badge, |d, badge| {
                                d.child(
                                    div()
                                        .ml(px(8.))
                                        .flex_shrink_0()
                                        .px(px(6.))
                                        .py(px(2.))
                                        .rounded(px(3.))
                                        .bg(c.bg_tertiary)
                                        .text_size(px(10.))
                                        .line_height(px(12.))
                                        .text_color(c.text_secondary)
                                        .child(badge),
                                )
                            })
                            .into_any_element()
                    })
                    .collect()
            }
        };
        div()
            .id("child-list")
            .flex()
            .flex_col()
            .max_h(px(200.))
            .overflow_y_scroll()
            .border_1()
            .border_color(c.border)
            .rounded(px(4.))
            .bg(c.bg_primary)
            .children(body)
            .into_any_element()
    }

    /// `.create-child-dialog-field`: a 13px secondary label 6px above its control, 16px below.
    fn field(label: &'static str, control: impl IntoElement, cx: &App) -> gpui_kit::Div {
        div()
            .mb(px(16.))
            .child(div().mb(px(6.)).text_size(px(ui::BODY)).text_color(colors(cx).text_secondary).child(label))
            .child(control)
    }

    /// The dialog's text inputs: bg-primary (`.create-child-dialog-field input`).
    fn input(state: &Entity<InputState>, size: f32, cx: &App) -> gpui_kit::component::input::Input {
        let height = if size > 13. { 35. } else { 33.5 };
        gpui_kit::Styled::h(ui::text_input(state, cx), px(height)).text_size(px(size)).bg(colors(cx).bg_primary)
    }

    /// The collapsible "Settings" section: the inherited default application and a custom one.
    fn settings_section(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = colors(cx);
        let (primary, secondary) = (c.text_primary, c.text_secondary);
        let toggle = div()
            .id("child-settings-toggle")
            .flex()
            .items_center()
            .gap(px(8.))
            .py(px(4.))
            .text_size(px(ui::BODY))
            .line_height(px(16.))
            .text_color(secondary)
            .cursor_pointer()
            .hover(move |d| d.text_color(primary))
            .child(div().text_size(px(15.)).child(if self.settings_expanded { "▼" } else { "▶" }))
            .child("Settings")
            .on_click(cx.listener(|dialog, _, _, cx| {
                dialog.settings_expanded = !dialog.settings_expanded;
                cx.notify();
            }));
        let content = self.settings_expanded.then(|| {
            let session = self.session.read(cx);
            let registry = session.registry().clone();
            let inherited = self.inherited_app(cx).unwrap_or_else(|| "Global Default".to_owned());
            let apps: Vec<(String, String)> =
                registry.new_tab_menu().map(|app| (app.id.clone(), app.name.clone())).collect();
            let view = cx.entity().downgrade();
            let custom = ui::checkbox_label(
                "child-custom-app",
                self.use_custom_app,
                false,
                "Use custom default application",
                move |on, _, cx| {
                    if let Err(error) = view.update(cx, |dialog, cx| {
                        dialog.use_custom_app = on;
                        if !on {
                            dialog.custom_app = None;
                        }
                        cx.notify();
                    }) {
                        tracing::debug!(%error, "dialog closed");
                    }
                },
            );
            let current = self
                .custom_app
                .as_ref()
                .and_then(|id| apps.iter().find(|(app, _)| app == id).map(|(_, name)| name.clone()))
                .unwrap_or_else(|| "Select an application...".to_owned());
            let picker = self.use_custom_app.then(|| {
                let view = cx.entity().downgrade();
                let selected = self.custom_app.clone();
                let boxed = ui::select_box("child-app", current, cx).h(px(31.)).pl(px(10.));
                let menu = ui::Trigger::new(boxed).dropdown_menu(move |mut menu, _, _| {
                    let none = view.clone();
                    menu = menu.item(ui::menu_item("Select an application...").checked(selected.is_none()).on_click(
                        move |_, _, cx| {
                            if let Err(error) = none.update(cx, |dialog, cx| {
                                dialog.custom_app = None;
                                cx.notify();
                            }) {
                                tracing::debug!(%error, "dialog closed");
                            }
                        },
                    ));
                    for (id, name) in &apps {
                        let (view, id) = (view.clone(), id.clone());
                        menu = menu.item(ui::menu_item(name.clone()).checked(selected.as_ref() == Some(&id)).on_click(
                            move |_, _, cx| {
                                if let Err(error) = view.update(cx, |dialog, cx| {
                                    dialog.custom_app = Some(id.clone());
                                    cx.notify();
                                }) {
                                    tracing::debug!(%error, "dialog closed");
                                }
                            },
                        ));
                    }
                    menu.min_w(px(200.))
                });
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(div().text_size(px(12.)).text_color(secondary).child("Default Application"))
                    .child(menu)
            });
            div()
                .mt(px(12.))
                .pl(px(16.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .mb(px(12.))
                        .text_size(px(ui::BODY))
                        .text_color(secondary)
                        .child(div().font_weight(FontWeight::MEDIUM).child("Inherited:"))
                        .child(div().text_color(primary).child(inherited)),
                )
                .child(div().mb(px(12.)).child(custom))
                .children(picker)
        });
        div()
            .mt(px(16.))
            .pt(px(16.))
            .border_t_1()
            .border_color(c.border)
            .child(toggle)
            .children(content)
            .into_any_element()
    }

    /// `getInheritedDefaultApplicationIds`: the parent's own default application, else its
    /// ancestors' — the first that exists, by name.
    fn inherited_app(&self, cx: &App) -> Option<String> {
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
}

impl Render for ChildDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (primary, secondary, tertiary, bg_primary) = {
            let c = colors(cx);
            (c.text_primary, c.text_secondary, c.bg_tertiary, c.bg_primary)
        };
        let info = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .mb(px(16.))
            .px(px(12.))
            .py(px(8.))
            .rounded(px(4.))
            .bg(tertiary)
            .text_size(px(ui::BODY))
            .child(div().text_color(secondary).child("Parent:"))
            .child(div().text_color(primary).font_weight(FontWeight::MEDIUM).child(self.parent_name.clone()));
        let search = || div().mb(px(12.)).child(Self::input(&self.search, ui::BODY, cx));
        let body: Vec<AnyElement> = match self.mode {
            Mode::Create => {
                let problem = self.name_problem(cx);
                vec![
                    div()
                        .mb(px(16.))
                        .child(div().mb(px(6.)).text_size(px(ui::BODY)).text_color(secondary).child("Name"))
                        .child(Self::input(&self.name, 14., cx))
                        .when_some(problem, |d, problem| {
                            d.child(div().mt(px(4.)).text_size(px(12.)).text_color(rgb(0xf14c4c)).child(problem))
                        })
                        .into_any_element(),
                ]
            }
            Mode::Existing => vec![
                Self::field("Name (optional)", Self::input(&self.name, 14., cx), cx).into_any_element(),
                self.list(cx),
            ],
            Mode::Recent => {
                vec![Self::field("Name", Self::input(&self.name, 14., cx), cx).into_any_element(), self.list(cx)]
            }
            Mode::Branch | Mode::Remote | Mode::Pr => vec![search().into_any_element(), self.list(cx)],
        };
        let description = (self.mode != Mode::Pr).then(|| {
            Self::field(
                "Description",
                ui::text_area(&self.description, cx).bg(bg_primary),
                cx,
            )
        });
        // `.create-child-detached-checkbox`: a 14px checkbox, secondary label; its 12px top
        // margin collapses into the 16px above it.
        let detached = (!matches!(self.mode, Mode::Existing | Mode::Recent)).then(|| {
            let on = self.detached;
            div()
                .id("child-detached")
                .mt(px(if description.is_some() { 0. } else { 12. }))
                .mb(px(4.))
                .flex()
                .items_center()
                .gap(px(8.))
                .text_size(px(ui::BODY))
                .text_color(secondary)
                .cursor_pointer()
                .child(ui::checkbox_box(on, false, 14.))
                .child("Detached worktree (no merge, only close and clean)")
                .on_click(cx.listener(move |dialog, _, _, cx| {
                    dialog.detached = !on;
                    cx.notify();
                }))
        });
        let error = self.error.clone().map(|error| {
            div()
                .mt(px(12.))
                .px(px(12.))
                .py(px(8.))
                .rounded(px(4.))
                .bg(rgba(0xf14c4c1a))
                .text_color(rgb(0xf14c4c))
                .text_size(px(ui::BODY))
                .child(error)
        });
        let action = if self.mode == Mode::Create { "Create" } else { "Open" };
        let disabled = self.submit_disabled(cx);
        let submit = ui::dialog_button("child-submit", action, ui::ButtonKind::Primary, disabled, cx)
            .when(!disabled, |b| b.on_click(cx.listener(|dialog, _, window, cx| dialog.submit(window, cx))));
        ui::dialog_root(cx)
            .child(ui::dialog_header("Add Child Workspace", "x", primary, cx))
            .child(self.tabs(cx))
            .child(
                v_flex()
                    .p(px(20.))
                    .child(info)
                    .children(body)
                    .children(description)
                    .children(detached)
                    .child(self.settings_section(cx))
                    .children(error),
            )
            .child(
                ui::dialog_actions(cx)
                    .child(
                        ui::dialog_button("child-cancel", "Cancel", ui::ButtonKind::Secondary, false, cx)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(submit),
            )
    }
}

pub fn open(
    session: Entity<SessionStore>,
    parent_id: String,
    _parent_label: SharedString,
    on_created: OnCreated,
    window: &mut Window,
    cx: &mut App,
) {
    let view = cx.new(|cx| ChildDialog::new(session, parent_id, on_created, window, cx));
    window.open_dialog(cx, move |dialog, _window, cx| ui::dialog_frame(dialog, 400., cx).child(view.clone()));
}
