//! The file list beside the diff (`DiffFileTree.tsx`): section titles, directories with
//! their totals and "Filter to this folder", files with status, stats, viewed tick and,
//! uncommitted, Stage / Unstage.

use std::collections::HashSet;

use gpui_kit::assets::IconName;
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenuItem};
use gpui_kit::component::v_flex;
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, ClickEvent, Context, FontWeight, SharedString, div, px};
use treeterm_core::git::Staging;
use treeterm_model::review::StageTarget;
use treeterm_model::review_tree::{TreeNode, build_tree};

use super::ReviewView;
use super::parts::{MONO, change_stats, outline_button, plain_icon, status_badge, title, totals};
use super::stacked::ListedFile;
use crate::theme::colors;

/// A file tree beside the diff, each with its own folded directories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tree {
    /// The Committed view's: a click is kept in the tab state (`selectedFilePath`).
    Committed,
    /// The selected commit's, in the Commits view.
    Commit,
    Staged,
    Unstaged,
}

impl Tree {
    /// Keeps the rows of several trees apart.
    fn id(self) -> &'static str {
        match self {
            Tree::Committed => "committed",
            Tree::Commit => "commit",
            Tree::Staged => "Staged",
            Tree::Unstaged => "Unstaged",
        }
    }
}

/// `.diff-file-section`: 11px semibold uppercase, on the secondary background.
pub fn section_title(title: &'static str, cx: &App) -> AnyElement {
    let c = colors(cx);
    div()
        .flex()
        .items_center()
        .gap(px(6.))
        .px(px(12.))
        .py(px(8.))
        .bg(c.bg_secondary)
        .border_b_1()
        .border_color(c.border)
        .text_size(px(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(c.text_secondary)
        .child(SharedString::from(title.to_uppercase()))
        .into_any_element()
}

/// `FavouriteFileSection`: the workspace's favourite files above the changed ones, each
/// marked "changed" when the diff shows it. A changed one scrolls the diff to it; another
/// is shown in a file viewer in place of the diff (and stays selected). Hidden without any.
pub fn favourites(view: &ReviewView, changed: &[ListedFile], cx: &mut Context<ReviewView>) -> Option<AnyElement> {
    use treeterm_model::file_browser::Favourites;
    let c = colors(cx);
    let (secondary, primary, hover, border, active_bg) = (c.text_secondary, c.text_primary, c.bg_hover, c.border, c.bg_active);
    let star = gpui_kit::rgb(0xe3b341);
    let (favourites, selected) = {
        let model = view.model.read(cx);
        (model.favourites().clone(), model.favourite_selection().relative_path().map(str::to_owned))
    };
    let body: Vec<AnyElement> = match favourites {
        Favourites::Empty => return None,
        Favourites::Ready(files) if files.is_empty() => return None,
        Favourites::Loading => vec![div()
            .p(px(20.))
            .flex()
            .justify_center()
            .text_size(px(13.))
            .text_color(secondary)
            .child("Loading favourites...")
            .into_any_element()],
        Favourites::Failed(error) => vec![div()
            .px(px(20.))
            .py(px(40.))
            .flex()
            .justify_center()
            .text_size(px(13.))
            .text_color(gpui_kit::rgb(super::parts::ERROR_RED))
            .child(error)
            .into_any_element()],
        Favourites::Ready(files) => files
            .into_iter()
            .map(|file| {
                let in_diff = changed.iter().any(|f| f.path == file.relative_path);
                let name = treeterm_core::files::favourite_file_name(&file).to_owned();
                let is_selected = selected.as_deref() == Some(file.relative_path.as_str());
                let relative = file.relative_path.clone();
                let tooltip = SharedString::from(file.relative_path.clone());
                div()
                    .id(SharedString::from(format!("favourite-{}", file.relative_path)))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(12.))
                    .py(px(8.))
                    .text_size(px(12.))
                    .line_height(px(14.))
                    .text_color(primary)
                    .border_b_1()
                    .border_color(border)
                    .cursor_pointer()
                    .when(is_selected, |d| d.bg(active_bg))
                    .when(!is_selected, |d| d.hover(move |d| d.bg(hover)))
                    .child(crate::ui::icon(IconName::StarFill, 12., star))
                    .child(div().flex_1().min_w_0().truncate().child(name))
                    .when(in_diff, |d| {
                        d.child(
                            div()
                                .id("changed")
                                .ml_auto()
                                .flex_shrink_0()
                                .text_size(px(10.))
                                .text_color(secondary)
                                .child("CHANGED")
                                .tooltip(title("Changed in this view — jumps to the diff")),
                        )
                    })
                    // `title={file.relativePath}`: two favourites with one name told apart.
                    .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(tooltip.clone()).build(window, cx))
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                        if in_diff {
                            view.select_file(&relative, cx);
                        } else {
                            let file = file.clone();
                            view.model.update(cx, |m, cx| m.select_favourite(file, cx));
                        }
                    }))
                    .into_any_element()
            })
            .collect(),
    };
    let title = div()
        .flex()
        .items_center()
        .gap(px(6.))
        .px(px(12.))
        .py(px(8.))
        .bg(c.bg_secondary)
        .border_b_1()
        .border_color(border)
        .text_size(px(11.))
        .line_height(px(13.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(secondary)
        .child(crate::ui::icon(IconName::StarFill, 11., star))
        .child("FAVOURITES");
    Some(v_flex().child(title).children(body).into_any_element())
}

/// `CommittedDiffFileTree` (`staging: None`) or `UncommittedDiffFileTree` (`Some(busy)`).
pub fn tree(
    view: &mut ReviewView,
    tree: Tree,
    files: &[ListedFile],
    viewed: &HashSet<String>,
    staging: Option<bool>,
    _uncommitted: bool,
    cx: &mut Context<ReviewView>,
) -> AnyElement {
    let entries: Vec<(&str, treeterm_core::git::FileChangeStats)> = files.iter().map(|f| (f.path.as_str(), f.stats)).collect();
    let nodes = build_tree(&entries);
    view.folds.show(tree, &nodes);
    let view = &*view;
    // `selectedFile`: the file at the top of the diff (`activeFile`), else the first until
    // the diff reports it.
    let listed = view.model.read(cx).active_file().filter(|f| files.iter().any(|l| l.path == *f)).map(str::to_owned);
    let active = listed.or_else(|| treeterm_model::review_tree::tree_order(&nodes).first().map(|i| files[*i].path.clone()));
    let mut list = Vec::new();
    for node in &nodes {
        rows(view, tree, node, 0, files, viewed, staging, active.as_deref(), cx, &mut list);
    }
    // Chromium lays 12px text on 14px lines.
    v_flex().line_height(px(14.)).children(list).into_any_element()
}

#[allow(clippy::too_many_arguments)]
fn rows(
    view: &ReviewView,
    tree: Tree,
    node: &TreeNode,
    depth: usize,
    files: &[ListedFile],
    viewed: &HashSet<String>,
    staging: Option<bool>,
    active: Option<&str>,
    cx: &mut Context<ReviewView>,
    list: &mut Vec<AnyElement>,
) {
    let c = colors(cx);
    let (secondary, primary, hover, active_bg, border, accent) =
        (c.text_secondary, c.text_primary, c.bg_hover, c.bg_active, c.border, c.accent);
    let indent = px(depth as f32 * 16. + 12.);
    let id = tree.id();
    match node {
        TreeNode::Dir { name, path, children, additions, deletions } => {
            let open = view.folds.is_open(tree, path);
            let (toggle, filter) = (path.clone(), path.clone());
            let this = cx.weak_entity();
            let row = div()
                .id(SharedString::from(format!("{id}-dir-{path}")))
                .flex()
                .items_center()
                .gap(px(8.))
                .pl(indent)
                .pr(px(12.))
                .py(px(8.))
                .text_size(px(12.))
                .text_color(secondary)
                .cursor_pointer()
                .hover(move |d| d.bg(hover))
                .child(div().w(px(12.)).flex_shrink_0().flex().justify_center().text_size(px(8.)).child(if open { "\u{25BC}" } else { "\u{25B6}" }))
                .child(div().flex_1().min_w_0().truncate().child(name.clone()))
                .child(div().flex().gap(px(4.)).font_family(MONO).text_size(px(11.)).children(totals(*additions, *deletions)))
                .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                    view.folds.toggle(tree, &toggle);
                    cx.notify();
                }))
                .context_menu(move |menu, _, _| {
                    let (this, filter) = (this.clone(), filter.clone());
                    menu.item(PopupMenuItem::new("Filter to this folder").on_click(move |_, _, cx| {
                        let filter = filter.clone();
                        match this.update(cx, |view, cx| {
                            view.dir_filter = Some(filter);
                            cx.notify();
                        }) {
                            Ok(()) | Err(_) => {} // Err: the tab closed
                        }
                    }))
                });
            list.push(row.into_any_element());
            if open {
                for child in children {
                    rows(view, tree, child, depth + 1, files, viewed, staging, active, cx, list);
                }
            }
        }
        TreeNode::File { name, index } => {
            let file = &files[*index];
            let is_viewed = viewed.contains(&file.path);
            let selected = staging.is_none() && active == Some(file.path.as_str());
            let path = file.path.clone();
            let action = staging.map(|busy| {
                let staged = file.staged == Some(Staging::Staged);
                let target = StageTarget::File(file.path.clone());
                outline_button(SharedString::from(format!("{id}-stage-{}", file.path)), 10., (2., 8.), 3., !busy, cx)
                    .ml_auto()
                    .child(if staged { "Unstage" } else { "Stage" })
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                        // Disabled while staging runs, it still keeps the click from the row.
                        cx.stop_propagation();
                        if !busy {
                            let target = target.clone();
                            view.model.update(cx, |m, cx| if staged { m.unstage(target, cx) } else { m.stage(target, cx) });
                        }
                    }))
            });
            let row = div()
                .id(SharedString::from(format!("{id}-file-{}", file.path)))
                .flex()
                .items_center()
                .gap(px(8.))
                .pl(indent)
                .pr(px(12.))
                .py(px(8.))
                .text_size(px(12.))
                .text_color(primary)
                .border_b_1()
                .border_color(border)
                .cursor_pointer()
                .when(selected, |d| d.bg(active_bg))
                // `.viewed` fades the row, a little less while hovered, selected or not (a
                // selected row keeps its fill).
                .when(is_viewed && staging.is_none(), |d| d.opacity(0.6))
                .hover(move |d| {
                    let d = if selected { d } else { d.bg(hover) };
                    if is_viewed && staging.is_none() { d.opacity(0.8) } else { d }
                })
                .child(status_badge(file.status))
                .child(div().flex_1().min_w_0().truncate().child(name.clone()))
                .child(
                    div()
                        .flex()
                        .gap(px(4.))
                        .font_family(MONO)
                        .text_size(px(11.))
                        .children(change_stats(SharedString::from(format!("{id}-stats-{}", file.path)), file.stats, cx)),
                )
                .when(is_viewed && staging.is_none(), |d| {
                    d.child(div().flex_shrink_0().ml_auto().text_color(accent).child(plain_icon(IconName::Check, 12.)))
                })
                .children(action)
                // `title={file.path}`: the whole path of a name cut short or deep in the tree.
                .tooltip(title(file.path.clone()))
                .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                    view.select_file(&path, cx);
                    // `persistViewState({ selectedFilePath })`: a Committed click only.
                    match tree {
                        Tree::Committed => view.model.update(cx, |m, cx| m.remember_selected_file(&path, cx)),
                        Tree::Commit | Tree::Staged | Tree::Unstaged => {}
                    }
                }));
            list.push(row.into_any_element());
        }
    }
}
