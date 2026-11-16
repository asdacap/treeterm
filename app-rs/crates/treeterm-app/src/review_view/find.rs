//! Find in the diff (`StackedDiffList`'s find bar): Cmd/Ctrl+F opens [`crate::search_panel`]
//! under the diff, over the matches the model finds; the chosen match is scrolled to and
//! selected in the diff. The diff's context menu offers "Search" and "Copy" for the
//! selected text or the word under the pointer.

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::component::diff::{DiffLineRange, DiffLinePosition, DiffSide, DiffState};
use gpui_kit::component::input::{InputEvent, InputState, Search};
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenu, PopupMenuItem};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClipboardItem, Context, Entity, KeyBinding, MouseButton, MouseDownEvent, Pixels, Point, SharedString, Window,
    actions, div,
};
use treeterm_core::review::Side;
use treeterm_core::search::{DiffStyle, UnchangedRegions, word_at};
use treeterm_model::review::DiffFind;
use treeterm_model::review_find::{DiffLayout, DiffMenu};

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

impl ReviewView {
    /// How the diff lays its rows out now.
    pub(super) fn layout(&self) -> DiffLayout {
        DiffLayout {
            style: if self.split { DiffStyle::Split } else { DiffStyle::Unified },
            unchanged: if self.hide_unchanged { UnchangedRegions::Collapsed } else { UnchangedRegions::Expanded },
        }
    }

    /// The file being read: under the pointer last, else the one at the top of the diff,
    /// else the first of the diff (Electron took the top one on screen).
    fn reading(&self, cx: &App) -> String {
        let order = self.diff.as_ref().map(|diff| diff.paths()).unwrap_or_default();
        self.hovered_file
            .clone()
            .filter(|file| order.contains(file))
            .or_else(|| self.model.read(cx).active_file().map(str::to_owned).filter(|file| order.contains(file)))
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
    /// selection, else the word under the pointer, its "In this file" the file right-clicked;
    /// "Copy" for the selection, else that word. With neither, or below the files, there is
    /// no menu; file headers and comments have none (they keep the right button).
    pub(super) fn with_diff_menu(&self, diff: AnyElement, state: &Entity<DiffState>) -> AnyElement {
        // What the press under way points at, for the menu the Kit builds after it.
        let pressed: Rc<Cell<Option<(SharedString, DiffMenu)>>> = Rc::default();
        let (target, state, model) = (pressed.clone(), state.clone(), self.model.clone());
        div()
            .id("review-diff")
            .size_full()
            .child(diff)
            .on_mouse_down(MouseButton::Right, move |event: &MouseDownEvent, _, cx| {
                let found = diff_menu_at(&state, event.position, cx);
                if found.is_none() {
                    // No menu: the Kit's would open empty, and hold focus unseen.
                    cx.stop_propagation();
                }
                pressed.set(found);
            })
            .context_menu(move |menu: PopupMenu, _, _| {
                let Some((file, DiffMenu { copy, search })) = target.take() else {
                    return menu;
                };
                let model = model.clone();
                let menu = if search.is_empty() {
                    menu
                } else {
                    menu.item(PopupMenuItem::new(format!("Search \u{201c}{search}\u{201d}")).on_click(move |_, _, cx| {
                        model.update(cx, |m, cx| m.search_in_diff(&search, &file, cx));
                    }))
                };
                menu.item(PopupMenuItem::new("Copy").on_click(move |_, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()));
                }))
            })
            .into_any_element()
    }
}

/// The diff's menu for a right-click at `point` (window coordinates), and the file it is
/// on: the selection and the word under the pointer, on a file's rows.
pub(super) fn diff_menu_at(state: &Entity<DiffState>, point: Point<Pixels>, cx: &App) -> Option<(SharedString, DiffMenu)> {
    let state = state.read(cx);
    let file = state.file_at(point)?;
    let word = state.text_at(point).map(|(_, text, offset)| word_at(&text, offset).to_owned()).unwrap_or_default();
    DiffMenu::new(&state.selected_text(cx), &word).map(|menu| (file, menu))
}
