//! Find in the diff (`StackedDiffList`'s find bar): Cmd/Ctrl+F opens [`crate::search_panel`]
//! under the diff, over the matches the model finds; the chosen match is scrolled to and
//! selected in the diff. The diff's context menu offers "Search" and "Copy" for the
//! selected text.

use std::rc::Rc;

use gpui_kit::component::diff::{DiffLineRange, DiffLinePosition, DiffSide, DiffState};
use gpui_kit::component::input::{InputEvent, InputState, Search};
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenu, PopupMenuItem};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, ClipboardItem, Context, Entity, KeyBinding, Window, actions, div};
use treeterm_core::review::Side;
use treeterm_core::search::{DiffStyle, UnchangedRegions};
use treeterm_model::review::DiffFind;
use treeterm_model::review_find::DiffLayout;

use super::ReviewView;

actions!(review, [FindInDiff]);

/// The Review tab, where Cmd/Ctrl+F finds in the diff.
pub(super) const CONTEXT: &str = "Review";

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new(if cfg!(target_os = "macos") { "cmd-f" } else { "ctrl-f" }, FindInDiff, Some(CONTEXT)),
    ]);
}

fn diff_side(side: Side) -> DiffSide {
    match side {
        Side::Original => DiffSide::Original,
        Side::Modified => DiffSide::Modified,
    }
}

/// A single-line selection to search for (`searchText`); empty when there is none.
fn search_text(selected: &str) -> &str {
    let trimmed = selected.trim();
    if trimmed.contains('\n') { "" } else { trimmed }
}

impl ReviewView {
    /// How the diff lays its rows out now.
    pub(super) fn layout(&self) -> DiffLayout {
        DiffLayout {
            style: if self.split { DiffStyle::Split } else { DiffStyle::Unified },
            unchanged: if self.hide_unchanged { UnchangedRegions::Collapsed } else { UnchangedRegions::Expanded },
        }
    }

    /// The file being read: under the pointer last, else the one last selected, else the
    /// first of the diff (Electron took the top one on screen).
    fn reading(&self, cx: &App) -> String {
        let order = self.diff.as_ref().map(|diff| diff.paths()).unwrap_or_default();
        self.hovered_file
            .clone()
            .filter(|file| order.contains(file))
            .or_else(|| self.model.read(cx).focused_file().map(str::to_owned).filter(|file| order.contains(file)))
            .or_else(|| order.first().cloned())
            .unwrap_or_default()
    }

    /// Cmd/Ctrl+F (also from a text box in the tab: its own Search action bubbles here).
    pub(super) fn on_find(&mut self, _: &FindInDiff, _: &mut Window, cx: &mut Context<Self>) {
        self.open_find(cx);
    }

    pub(super) fn on_search(&mut self, _: &Search, _: &mut Window, cx: &mut Context<Self>) {
        self.open_find(cx);
    }

    fn open_find(&mut self, cx: &mut Context<Self>) {
        // Only where a diff is shown (not the commit list alone, not a favourite's viewer).
        if self.diff.is_none() {
            return;
        }
        let file = self.reading(cx);
        self.model.update(cx, |m, cx| m.open_find(&file, cx));
    }

    fn find_input(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<InputState> {
        if let Some((input, _)) = &self.find_input {
            return input.clone();
        }
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Search"));
        let events = cx.subscribe_in(&input, window, |view, input, event: &InputEvent, _, cx| match event {
            InputEvent::Change => {
                let query = input.read(cx).value().to_string();
                view.model.update(cx, |m, cx| m.set_find_query(&query, cx));
            }
            InputEvent::PressEnter { shift, .. } => {
                let (forward, matches) = (!*shift, view.find_matches.clone());
                view.model.update(cx, |m, cx| m.step_find(&matches, forward, cx));
            }
            InputEvent::Focus | InputEvent::Blur => cx.notify(),
        });
        self.find_input = Some((input.clone(), events));
        input
    }

    /// Bring the chosen match into view and select its line, once per choice; focus the box
    /// when the bar was (re)opened.
    fn apply_find(&mut self, state: &Entity<DiffState>, window: &mut Window, cx: &mut Context<Self>) {
        let DiffFind::Open(find) = self.model.read(cx).find().clone() else {
            return;
        };
        if find.focus != self.find_focused {
            self.find_focused = find.focus;
            let input = self.find_input(window, cx);
            input.update(cx, |input, cx| {
                if input.value().as_ref() != find.query {
                    input.set_value(find.query.clone(), window, cx);
                }
                input.focus(window, cx);
                input.select_all(window, cx);
            });
        }
        if let Some(target) = find.active
            && target.seq != self.find_revealed
        {
            self.find_revealed = target.seq;
            let side = diff_side(target.side);
            let line = target.line as usize;
            state.update(cx, |s, cx| {
                s.scroll_to_line(DiffLinePosition::new(target.path.clone(), side, line), cx);
                s.set_selected_lines(Some(DiffLineRange::new(target.path.clone(), side, line, line)), cx);
            });
        }
    }

    /// The find bar under the diff of `patch` (whose files are `paths`, in list order).
    pub(super) fn find_panel(
        &mut self,
        state: &Entity<DiffState>,
        patch: &str,
        paths: &[String],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let DiffFind::Open(find) = self.model.read(cx).find().clone() else {
            self.find_matches = Rc::new(Vec::new());
            return None;
        };
        let layout = self.layout();
        let (matches, rows, count) = {
            let model = self.model.read(cx);
            let matches = model.find_matches(patch, paths, layout);
            let rows = model.find_rows(&matches, paths);
            let count = model.find_count_label(&matches);
            (Rc::new(matches), rows, count)
        };
        self.find_matches = matches.clone();
        self.apply_find(state, window, cx);
        let input = self.find_input(window, cx);
        let (select, step, open, close) = (self.model.clone(), self.model.clone(), self.model.clone(), self.model.clone());
        let (select_matches, step_matches) = (matches.clone(), matches.clone());
        let actions = crate::search_panel::PanelActions {
            select: Rc::new(move |index, cx| select.update(cx, |m, cx| m.select_find_match(&select_matches, index, cx))),
            open_repo: Rc::new(move |hit, cx| open.update(cx, |m, cx| m.open_find_hit(hit, cx))),
            step: Rc::new(move |forward, cx| step.update(cx, |m, cx| m.step_find(&step_matches, forward, cx))),
            close: Rc::new(move |cx| close.update(cx, |m, cx| m.close_find(cx))),
        };
        let bar = crate::search_panel::Bar { input, count, has_matches: !matches.is_empty(), has_query: !find.query.is_empty() };
        Some(crate::search_panel::panel(bar, rows, &mut self.find_results, &actions, window, cx))
    }

    /// The diff with its right-click menu (`contextTarget`): "Search “…”" for a one-line
    /// selection and "Copy" for any. GPUI Kit's diff does not say which word is under the
    /// pointer, so without a selection there is no menu.
    pub(super) fn with_diff_menu(&self, diff: AnyElement, state: &Entity<DiffState>, cx: &Context<Self>) -> AnyElement {
        let (state, model, this) = (state.clone(), self.model.clone(), cx.weak_entity());
        div()
            .id("review-diff")
            .size_full()
            .child(diff)
            .context_menu(move |menu: PopupMenu, _, cx| {
                let selected = state.read(cx).selected_text(cx);
                if selected.is_empty() {
                    return menu;
                }
                let query = search_text(&selected).to_owned();
                let file = match this.upgrade() {
                    Some(view) => view.read(cx).reading(cx),
                    None => String::new(),
                };
                let model = model.clone();
                let menu = if query.is_empty() {
                    menu
                } else {
                    menu.item(PopupMenuItem::new(format!("Search \u{201c}{query}\u{201d}")).on_click(move |_, _, cx| {
                        model.update(cx, |m, cx| m.search_in_diff(&query, &file, cx));
                    }))
                };
                menu.item(PopupMenuItem::new("Copy").on_click(move |_, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(selected.clone()));
                }))
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_one_line_selection_is_searched() {
        assert_eq!(search_text("  needle "), "needle");
        assert_eq!(search_text("two\nlines"), "");
        assert_eq!(search_text(""), "");
    }
}
