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
use treeterm_model::search_panel::SearchRow;

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
    list: ListState,
    rows: Vec<SearchRow>,
}

impl ResultsList {
    pub fn new() -> Self {
        Self { list: ListState::new(0, ListAlignment::Top, px(200.)), rows: Vec::new() }
    }

    fn sync(&mut self, rows: &[SearchRow]) {
        let unchanged = self.rows.iter().zip(rows).take_while(|(a, b)| a == b).count();
        if unchanged != self.rows.len() || unchanged != rows.len() {
            // One more row than results: the bottom padding.
            let old = if self.rows.is_empty() { self.list.item_count() } else { self.rows.len() + 1 };
            self.list.splice(unchanged..old, rows.len() + 1 - unchanged);
            self.rows = rows.to_vec();
        }
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
            .child(div().min_w_0().truncate().child(preview.replace('\t', "    ")))
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
        SearchRow::File(path) => div()
            .pt(px(4.))
            .px(px(8.))
            .pb(px(2.))
            .text_size(px(ui::SMALL))
            .text_color(primary)
            .truncate()
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
        list(results.list.clone(), move |index, _, cx| match rows.get(index) {
            Some(found) => row(found.clone(), index, &actions, cx),
            None => div().h(px(8.)).into_any_element(),
        })
        .flex_1()
        .min_h_0()
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
