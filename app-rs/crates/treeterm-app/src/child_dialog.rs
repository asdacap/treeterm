//! "New child workspace": a git worktree branched from the parent's current branch.
//! Port of the Create tab of `CreateChildDialog.tsx` (Existing/Recent/Branch/PR follow).

use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{App, Context, Entity, SharedString, Window, div, px};
use treeterm_model::session_store::SessionStore;

use crate::theme::colors;

/// Called with the new child's id once creation has started.
pub type OnCreated = Rc<dyn Fn(String, &mut Window, &mut App)>;

pub struct ChildDialog {
    session: Entity<SessionStore>,
    parent_id: String,
    name: Entity<InputState>,
    description: Entity<InputState>,
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
        Self {
            session,
            parent_id,
            name: cx.new(|cx| InputState::new(window, cx).placeholder("feature-name")),
            description: cx.new(|cx| InputState::new(window, cx).placeholder("What is this for? (optional)")),
            error: None,
        }
    }

    /// Start creating; returns the child's id, or shows why not.
    fn create(&mut self, cx: &mut Context<Self>) -> Option<String> {
        let name = self.name.read(cx).value().trim().to_owned();
        if let Some(problem) = invalid_branch_name(&name) {
            self.error = Some(problem.into());
            cx.notify();
            return None;
        }
        let description = self.description.read(cx).value().trim().to_owned();
        let parent = self.parent_id.clone();
        let result = self.session.update(cx, |s, cx| {
            s.add_child_workspace(&parent, name, false, Some(description), cx)
        });
        match result {
            Ok(id) => Some(id),
            Err(error) => {
                self.error = Some(error.into());
                cx.notify();
                None
            }
        }
    }
}

impl Render for ChildDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors(cx);
        let label = |text: &'static str| div().text_sm().text_color(c.text_secondary).child(text);
        v_flex()
            .gap_2()
            .child(label("Branch and worktree name"))
            .child(Input::new(&self.name).small())
            .child(label("Description"))
            .child(Input::new(&self.description).small())
            .when_some(self.error.clone(), |d, error| d.child(div().text_sm().text_color(c.error).child(error)))
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
            .w(px(460.))
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
