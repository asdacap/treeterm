//! `SearchPanel.tsx` as the Files viewer and Review's diff show it: the find bar (box,
//! counter, previous / next, close) over the results (`.search-panel`). What a row or button
//! does is the caller's.

use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Escape, Input, InputState};
use gpui_kit::component::{Icon, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, Entity, Focusable as _, FontWeight, ListAlignment, ListState, Window, div, list, px, relative, rgb};
use treeterm_core::review::Side;
use treeterm_core::search::RepoHit;
use treeterm_model::search_panel::{RowsChange, SearchRow, expand_tabs, rows_change};

use crate::theme::colors;
use crate::ui;

/// Menlo, as `.search-panel-hit` sets it.
const MONO: &str = "Menlo";

/// Selects a match in view (the open file, the diff), by its index.
pub type SelectAction = Rc<dyn Fn(usize, &mut App)>;
/// Opens a hit elsewhere in the repo.
pub type RepoAction = Rc<dyn Fn(&RepoHit, &mut App)>;
/// Moves to the next (`true`) or previous match.
pub type StepAction = Rc<dyn Fn(bool, &mut App)>;

/// What the panel's rows and buttons do.
#[derive(Clone)]
pub struct PanelActions {
    pub select: SelectAction,
    pub open_repo: RepoAction,
    pub step: StepAction,
    pub close: Rc<dyn Fn(&mut App)>,
}

/// The results list, told which rows changed so its scroll position survives appended hits.
pub struct ResultsList {
    /// One item more than `rows`: the bottom padding.
    list: ListState,
    rows: Vec<SearchRow>,
}

impl ResultsList {
    pub fn new() -> Self {
        // Every row is measured, so the scrollbar knows the whole height, as Chromium's does:
        // the first layout measures them all, and a later one the rows `sync` marks.
        Self { list: ListState::new(1, ListAlignment::Top, px(200.)).measure_all(), rows: Vec::new() }
    }

    fn sync(&mut self, rows: &[SearchRow]) {
        match rows_change(&self.rows, rows) {
            RowsChange::Unchanged => return,
            RowsChange::Remeasure(changed) => {
                for index in changed {
                    self.list.remeasure_items(index..index + 1);
                }
            }
            RowsChange::Splice { old, count } => {
                let start = old.start;
                self.list.splice(old, count);
                // A splice leaves the new rows unmeasured (no height) without asking for
                // another measure-all pass, so rows arriving off screen would be missing
                // from the thumb until scrolled into view. Marking them asks for that pass;
                // the other rows keep their heights and the list its scroll position.
                self.list.remeasure_items(start..start + count);
            }
        }
        self.rows = rows.to_vec();
    }
}

/// What the bar shows.
pub struct Bar {
    pub input: Entity<InputState>,
    /// `3/12`, `12 found`.
    pub count: String,
    /// Previous / next are enabled.
    pub has_matches: bool,
    /// Rows are listed only once there is a query.
    pub has_query: bool,
}

fn row(row: SearchRow, index: usize, actions: &PanelActions, cx: &App) -> AnyElement {
    let c = colors(cx);
    let (secondary, primary, hover, active) = (c.text_secondary, c.text_primary, c.bg_hover, c.bg_active);
    let hit = |label: String, preview: String, is_active: bool| {
        h_flex()
            .id(("search-hit", index))
            .w_full()
            .gap(px(8.))
            .pl(px(16.))
            .pr(px(8.))
            .py(px(2.))
            .font_family(MONO)
            .text_size(px(ui::SMALL))
            .text_color(if is_active { primary } else { secondary })
            .cursor_pointer()
            .when(is_active, |d| d.bg(active))
            .when(!is_active, |d| d.hover(move |d| d.bg(hover).text_color(primary)))
            .child(div().flex_shrink_0().min_w(px(32.)).flex().justify_end().child(label))
            .child(div().min_w_0().truncate().child(expand_tabs(&preview)))
    };
    match row {
        SearchRow::Section(title) => div()
            .pt(px(8.))
            .px(px(8.))
            .pb(px(4.))
            .text_size(px(ui::SMALL))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(secondary)
            .child(title.to_uppercase())
            .into_any_element(),
        // `direction: rtl`: a long path loses its start, so the file name stays in view.
        SearchRow::File(path) => div()
            .pt(px(4.))
            .px(px(8.))
            .pb(px(2.))
            .text_size(px(ui::SMALL))
            .text_color(primary)
            .overflow_hidden()
            .whitespace_nowrap()
            .text_ellipsis_start()
            .child(path)
            .into_any_element(),
        SearchRow::Note(text) => h_flex()
            .gap(px(4.))
            .px(px(8.))
            .py(px(4.))
            .text_size(px(ui::SMALL))
            .text_color(secondary)
            .when(text == "Searching repo…", |d| {
                use gpui_kit::component::Sizable as _;
                d.child(
                    gpui_kit::component::spinner::Spinner::new()
                        .icon(Icon::new(IconName::LoaderCircle))
                        .with_size(px(12.))
                        .color(secondary),
                )
            })
            .child(text)
            .into_any_element(),
        SearchRow::Error(text) => {
            div().px(px(8.)).py(px(4.)).text_size(px(ui::SMALL)).text_color(rgb(ui::FILE_ERROR)).child(text).into_any_element()
        }
        SearchRow::InFile { index: at, line_number, preview, active: is_active } => {
            let select = actions.select.clone();
            hit(line_number.to_string(), preview, is_active).on_click(move |_, _, cx| select(at, cx)).into_any_element()
        }
        SearchRow::InDiff { index: at, side, line_number, preview, active: is_active } => {
            let select = actions.select.clone();
            // `−` marks a line of the old side.
            let label = match side {
                Side::Original => format!("\u{2212}{line_number}"),
                Side::Modified => line_number.to_string(),
            };
            hit(label, preview, is_active).on_click(move |_, _, cx| select(at, cx)).into_any_element()
        }
        SearchRow::InRepo(found) => {
            let open = actions.open_repo.clone();
            hit(found.line_number.to_string(), found.preview.clone(), false)
                .on_click(move |_, _, cx| open(&found, cx))
                .into_any_element()
        }
    }
}

/// `.search-panel`: the find bar over `rows`, a third of the height it is given.
pub fn panel(bar: Bar, rows: Vec<SearchRow>, results: &mut ResultsList, actions: &PanelActions, window: &Window, cx: &App) -> AnyElement {
    let c = colors(cx);
    let focused = bar.input.read(cx).focus_handle(cx).is_focused(window);
    results.sync(&rows);
    let rows = Rc::new(rows);
    let listed = bar.has_query.then(|| {
        let (rows, actions) = (rows.clone(), actions.clone());
        let list = list(results.list.clone(), move |index, _, cx| match rows.get(index) {
            Some(found) => row(found.clone(), index, &actions, cx),
            None => div().h(px(8.)).into_any_element(),
        })
        .flex_1()
        .min_w_0()
        .h_full();
        h_flex().flex_1().min_h_0().child(list).children(ui::scrollbar("search-results", &results.list, cx))
    });
    let step = |forward: bool| {
        let step = actions.step.clone();
        move |_: &gpui_kit::ClickEvent, _: &mut Window, cx: &mut App| step(forward, cx)
    };
    let prev = ui::find_button("search-prev", IconName::ChevronUp, !bar.has_matches, cx)
        .tooltip(|window, cx| gpui_kit::component::tooltip::Tooltip::new("Previous match (Shift+Enter)").build(window, cx));
    let next = ui::find_button("search-next", IconName::ChevronDown, !bar.has_matches, cx)
        .tooltip(|window, cx| gpui_kit::component::tooltip::Tooltip::new("Next match (Enter)").build(window, cx));
    let close = {
        let close = actions.close.clone();
        ui::find_button("search-close", IconName::X, false, cx)
            .tooltip(|window, cx| gpui_kit::component::tooltip::Tooltip::new("Close (Escape)").build(window, cx))
            .on_click(move |_, _, cx| close(cx))
    };
    let on_escape = actions.close.clone();
    v_flex()
        .h(relative(0.35))
        .min_h(px(160.))
        .flex_shrink_0()
        .bg(c.bg_secondary)
        .border_t_1()
        .border_color(c.border)
        .on_action(move |_: &Escape, _, cx| on_escape(cx))
        .child(
            h_flex()
                .flex_shrink_0()
                .gap(px(4.))
                .px(px(8.))
                .py(px(4.))
                .bg(c.bg_secondary)
                .border_b_1()
                .border_color(c.border)
                .child(div().flex_shrink_0().text_color(c.text_secondary).child(Icon::new(IconName::Search).size(px(12.))))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .max_w(px(360.))
                        .h(px(22.))
                        .px(px(6.))
                        .flex()
                        .items_center()
                        .border_1()
                        .border_color(if focused { c.accent } else { c.border })
                        .rounded(px(3.))
                        .bg(c.bg_primary)
                        .text_size(px(12.))
                        .text_color(c.text_primary)
                        .child(Styled::h(Input::new(&bar.input).appearance(false).text_size(px(12.)).px_0().py_0(), px(14.))),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .min_w(px(64.))
                        .whitespace_nowrap()
                        .text_size(px(ui::SMALL))
                        .text_color(c.text_secondary)
                        .child(bar.count),
                )
                .child(if bar.has_matches { prev.on_click(step(false)) } else { prev })
                .child(if bar.has_matches { next.on_click(step(true)) } else { next })
                .child(close),
        )
        .children(listed)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use gpui_kit::{Context, ListOffset, Render, TestAppContext, VisualTestContext, point, size};

    use super::*;

    /// Every row (and the bottom padding) is this tall, so the heights add up exactly.
    const ROW: f32 = 20.;
    /// Shorter than the rows, and than the rows plus the list's 200px overdraw.
    const VIEWPORT: f32 = 100.;

    struct Results(ListState);

    impl Render for Results {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            list(self.0.clone(), |_, _, _| div().h(px(ROW)).w_full().into_any_element()).size_full()
        }
    }

    fn draw(view: &Entity<Results>, cx: &mut VisualTestContext) {
        let view = view.clone();
        cx.draw(point(px(0.), px(0.)), size(px(200.), px(VIEWPORT)), move |_, _| view.into_any_element());
    }

    /// What the thumb is sized for: the rows' height (and the padding) less the viewport.
    fn max_offset(rows: usize) -> gpui_kit::Pixels {
        px((rows + 1) as f32 * ROW - VIEWPORT)
    }

    fn repo_hit(line_number: u64) -> SearchRow {
        SearchRow::InRepo(RepoHit { file_path: "a.rs".into(), line_number, column: 1, preview: "x".into() })
    }

    /// Repo hits arriving below the in-file rows, out of view, count in the scrollbar in the
    /// frame they arrive, while the list stays where it was scrolled to.
    #[gpui_kit::test]
    fn rows_spliced_in_off_screen_count_in_the_scrollbar(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        let mut results = ResultsList::new();
        let view = cx.update(|_, cx| cx.new(|_| Results(results.list.clone())));

        let in_file = (0..30).map(|index| SearchRow::InFile { index, line_number: index + 1, preview: "x".into(), active: index == 0 });
        let mut rows: Vec<SearchRow> = std::iter::once(SearchRow::Section("In this file")).chain(in_file).collect();
        rows.extend([SearchRow::Section("In repo"), SearchRow::Note("Searching repo…".into())]);
        results.sync(&rows);
        draw(&view, cx);
        assert_eq!(results.list.max_offset_for_scrollbar().y, max_offset(rows.len()));

        let scrolled = ListOffset { item_ix: 3, offset_in_item: px(5.) };
        results.list.scroll_to(scrolled);
        draw(&view, cx);

        // A batch of hits goes in before the trailing note, 640px down.
        let note = rows.pop().expect("the trailing note");
        rows.push(SearchRow::File("a.rs".into()));
        rows.extend((1..=10).map(repo_hit));
        rows.push(note);
        results.sync(&rows);
        draw(&view, cx);
        assert_eq!(results.list.max_offset_for_scrollbar().y, max_offset(rows.len()));
        assert_eq!(results.list.logical_scroll_top(), scrolled);

        // The last batch and the end of the search in one frame: the note gives way to hits.
        rows.pop();
        rows.extend((11..=20).map(repo_hit));
        results.sync(&rows);
        draw(&view, cx);
        assert_eq!(results.list.max_offset_for_scrollbar().y, max_offset(rows.len()));
        assert_eq!(results.list.logical_scroll_top(), scrolled);
    }
}
