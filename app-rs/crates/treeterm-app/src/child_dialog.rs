//! "New child workspace". Port of `CreateChildDialog.tsx`: a new branch from the parent's
//! current branch, an existing local branch, a remote branch, or an existing worktree.
//! (Pull requests come with the GitHub port.)

use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{App, Context, Entity, SharedString, Window, div, px};
use treeterm_core::git::WorktreeInfo;
use treeterm_model::session_store::{ChildOptions, ChildSource, SessionStore};

use crate::theme::colors;

/// Called with the new child's id once creation has started.
pub type OnCreated = Rc<dyn Fn(String, &mut Window, &mut App)>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    NewBranch,
    ExistingBranch,
    RemoteBranch,
    ExistingWorktree,
}

impl Mode {
    const ALL: [Mode; 4] = [Mode::NewBranch, Mode::ExistingBranch, Mode::RemoteBranch, Mode::ExistingWorktree];

    fn label(self) -> &'static str {
        match self {
            Mode::NewBranch => "New branch",
            Mode::ExistingBranch => "Branch",
            Mode::RemoteBranch => "Remote",
            Mode::ExistingWorktree => "Worktree",
        }
    }
}

/// A list read from git for the picker modes.
enum Listing<T> {
    Loading,
    Failed(String),
    Loaded(Vec<T>),
}

pub struct ChildDialog {
    session: Entity<SessionStore>,
    parent_id: String,
    mode: Mode,
    name: Entity<InputState>,
    description: Entity<InputState>,
    branches: Listing<String>,
    remotes: Listing<String>,
    worktrees: Listing<WorktreeInfo>,
    selected: Option<usize>,
    error: Option<SharedString>,
}

/// Git refuses these in branch names; refuse them up front with a clear message.
fn invalid_branch_name(name: &str) -> Option<&'static str> {
    if name.is_empty() {
        return Some("Enter a name");
    }
    if name.chars().any(|c| c.is_whitespace() || "~^:?*[\\".contains(c))
        || name.contains("..")
        || name.contains("@{")
        || name.starts_with(['-', '/', '.'])
        || name.ends_with(['/', '.'])
        || name.ends_with(".lock")
    {
        return Some("Not a valid git branch name");
    }
    None
}

impl ChildDialog {
    fn new(session: Entity<SessionStore>, parent_id: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut dialog = Self {
            name: cx.new(|cx| InputState::new(window, cx).placeholder("feature-name")),
            description: cx.new(|cx| InputState::new(window, cx).placeholder("What is this for? (optional)")),
            session,
            parent_id,
            mode: Mode::NewBranch,
            branches: Listing::Loading,
            remotes: Listing::Loading,
            worktrees: Listing::Loading,
            selected: None,
            error: None,
        };
        dialog.load_lists(cx);
        dialog
    }

    /// Read branches, remote branches and worktrees of the parent's repository.
    fn load_lists(&mut self, cx: &mut Context<Self>) {
        let session = self.session.read(cx);
        let Some(parent) = session.workspace_store(&self.parent_id) else {
            return;
        };
        let file = parent.read(cx).file();
        let Some(root) = file.git_root_path.clone() else {
            let reason = "Parent workspace is not a git repository".to_owned();
            self.branches = Listing::Failed(reason.clone());
            self.remotes = Listing::Failed(reason.clone());
            self.worktrees = Listing::Failed(reason);
            return;
        };
        let open_paths: Vec<String> = session
            .workspace_ids()
            .filter_map(|id| session.workspace_store(id))
            .map(|w| w.read(cx).workspace().path.clone())
            .collect();
        let git = session.connection().git();
        cx.spawn(async move |this, cx| {
            let local = git.list_local_branches(&root).await;
            let in_worktrees = git.get_branches_in_worktrees(&root).await;
            let remotes = git.list_remote_branches(&root).await;
            let worktrees = git.list_worktrees(&root).await;
            let branches = match (local, in_worktrees) {
                (Ok(local), Ok(taken)) => {
                    Listing::Loaded(local.into_iter().filter(|b| !taken.contains(b)).collect())
                }
                (Err(e), _) | (_, Err(e)) => Listing::Failed(e.to_string()),
            };
            let remotes = match remotes {
                Ok(list) => Listing::Loaded(list.into_iter().filter(|b| !b.ends_with("/HEAD") && b.contains('/')).collect()),
                Err(e) => Listing::Failed(e.to_string()),
            };
            let worktrees = match worktrees {
                Ok(list) => Listing::Loaded(list.into_iter().filter(|w| !open_paths.contains(&w.path)).collect()),
                Err(e) => Listing::Failed(e.to_string()),
            };
            match this.update(cx, |dialog, cx| {
                dialog.branches = branches;
                dialog.remotes = remotes;
                dialog.worktrees = worktrees;
                cx.notify();
            }) {
                Ok(()) | Err(_) => {} // Err: dialog closed
            }
        })
        .detach();
    }

    fn fail(&mut self, message: impl Into<SharedString>, cx: &mut Context<Self>) -> Option<String> {
        self.error = Some(message.into());
        cx.notify();
        None
    }

    /// Start creating; returns the child's id, or shows why not.
    fn create(&mut self, cx: &mut Context<Self>) -> Option<String> {
        let description = self.description.read(cx).value().trim().to_owned();
        let options = ChildOptions {
            detached: false,
            description: Some(description.clone()),
        };
        let parent = self.parent_id.clone();
        let pick = |list: &Listing<String>, selected: Option<usize>| match (list, selected) {
            (Listing::Loaded(items), Some(i)) => items.get(i).cloned(),
            _ => None,
        };
        let source = match self.mode {
            Mode::NewBranch => {
                let name = self.name.read(cx).value().trim().to_owned();
                if let Some(problem) = invalid_branch_name(&name) {
                    return self.fail(problem, cx);
                }
                ChildSource::NewBranch { name }
            }
            Mode::ExistingBranch => match pick(&self.branches, self.selected) {
                Some(branch) => ChildSource::ExistingBranch { branch },
                None => return self.fail("Pick a branch", cx),
            },
            Mode::RemoteBranch => match pick(&self.remotes, self.selected) {
                Some(remote_branch) => ChildSource::RemoteBranch { remote_branch },
                None => return self.fail("Pick a remote branch", cx),
            },
            Mode::ExistingWorktree => {
                let worktree = match (&self.worktrees, self.selected) {
                    (Listing::Loaded(items), Some(i)) => items.get(i).cloned(),
                    _ => None,
                };
                let Some(worktree) = worktree else {
                    return self.fail("Pick a worktree", cx);
                };
                let result = self
                    .session
                    .update(cx, |s, cx| s.adopt_existing_worktree(&parent, worktree, Some(description), cx));
                return match result {
                    Ok(id) => Some(id),
                    Err(error) => self.fail(error, cx),
                };
            }
        };
        match self.session.update(cx, |s, cx| s.create_child(&parent, source, options, cx)) {
            Ok(id) => Some(id),
            Err(error) => self.fail(error, cx),
        }
    }

    fn list<T>(&self, listing: &Listing<T>, label: impl Fn(&T) -> String, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let c = colors(cx);
        match listing {
            Listing::Loading => div().text_sm().text_color(c.text_secondary).child("Loading…").into_any_element(),
            Listing::Failed(error) => div().text_sm().text_color(c.error).child(error.clone()).into_any_element(),
            Listing::Loaded(items) if items.is_empty() => {
                div().text_sm().text_color(c.text_secondary).child("Nothing to pick").into_any_element()
            }
            Listing::Loaded(items) => {
                let mut list = v_flex().id("child-picker").max_h(px(240.)).overflow_y_scroll();
                for (i, item) in items.iter().enumerate() {
                    let selected = self.selected == Some(i);
                    list = list.child(
                        div()
                            .id(SharedString::from(format!("pick-{i}")))
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .text_sm()
                            .cursor_pointer()
                            .when(selected, |d| d.bg(c.bg_active))
                            .when(!selected, |d| d.hover(move |d| d.bg(c.bg_hover)))
                            .child(label(item))
                            .on_click(cx.listener(move |dialog, _, _, cx| {
                                dialog.selected = Some(i);
                                cx.notify();
                            })),
                    );
                }
                list.into_any_element()
            }
        }
    }
}

impl Render for ChildDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (secondary, error_color) = {
            let c = colors(cx);
            (c.text_secondary, c.error)
        };
        let label = move |text: &'static str| div().text_sm().text_color(secondary).child(text);
        let mut modes = h_flex().gap_1();
        for mode in Mode::ALL {
            let button = Button::new(mode.label()).label(mode.label()).small();
            let button = if self.mode == mode { button.primary() } else { button.ghost() };
            modes = modes.child(button.on_click(cx.listener(move |dialog, _, _, cx| {
                dialog.mode = mode;
                dialog.selected = None;
                dialog.error = None;
                cx.notify();
            })));
        }
        let body = match self.mode {
            Mode::NewBranch => v_flex()
                .gap_2()
                .child(label("Branch and worktree name"))
                .child(Input::new(&self.name).small())
                .into_any_element(),
            Mode::ExistingBranch => {
                let list = self.list(&self.branches, String::clone, cx);
                v_flex().gap_2().child(label("Local branches without a worktree")).child(list).into_any_element()
            }
            Mode::RemoteBranch => {
                let list = self.list(&self.remotes, String::clone, cx);
                v_flex().gap_2().child(label("Remote branches")).child(list).into_any_element()
            }
            Mode::ExistingWorktree => {
                let list = self.list(&self.worktrees, |w: &WorktreeInfo| format!("{}  ({})", w.path, w.branch), cx);
                v_flex().gap_2().child(label("Worktrees not open in TreeTerm")).child(list).into_any_element()
            }
        };
        v_flex()
            .gap_3()
            .child(modes)
            .child(body)
            .child(v_flex().gap_2().child(label("Description")).child(Input::new(&self.description).small()))
            .when_some(self.error.clone(), |d, error| d.child(div().text_sm().text_color(error_color).child(error)))
    }
}

pub fn open(
    session: Entity<SessionStore>,
    parent_id: String,
    parent_label: SharedString,
    on_created: OnCreated,
    window: &mut Window,
    cx: &mut App,
) {
    let view = cx.new(|cx| ChildDialog::new(session, parent_id, window, cx));
    window.open_dialog(cx, move |dialog, _window, _cx| {
        let (create, on_created) = (view.clone(), on_created.clone());
        dialog
            .title(format!("New child of “{parent_label}”"))
            .w(px(520.))
            .child(view.clone())
            .footer(h_flex().justify_end().child(Button::new("create-child").primary().label("Create").on_click(
                move |_, window, cx| {
                    if let Some(id) = create.update(cx, |dialog, cx| dialog.create(cx)) {
                        window.close_dialog(cx);
                        on_created(id, window, cx);
                    }
                },
            )))
    });
}

#[cfg(test)]
mod tests {
    use super::invalid_branch_name;

    #[test]
    fn branch_names_follow_git_rules() {
        assert_eq!(invalid_branch_name("feature-x"), None);
        assert_eq!(invalid_branch_name("user/feature"), None);
        for bad in ["", "has space", "a..b", "-x", "x/", "x.lock", "a:b", "x@{1}"] {
            assert!(invalid_branch_name(bad).is_some(), "{bad}");
        }
    }
}
