//! "Auto Open All Recent Worktrees" (`AutoOpenWorktreesDialog.tsx`): the root workspace's
//! worktrees that are recorded in the worktree registry (optionally all of them), placed in a
//! hierarchy by git ancestry, opened as child workspaces. The detection, the checked rows and
//! the batch are [`treeterm_model::auto_open`]; this view renders them. Ok waits for the
//! whole batch: the dialog closes once every worktree is open, or shows why the batch
//! stopped.

use gpui_kit::component::WindowExt as _;
use gpui_kit::prelude::*;
use gpui_kit::{App, Context, Entity, SharedString, Subscription, Window, div, px};
use treeterm_model::auto_open::{AutoOpenEvent, AutoOpenModel};
use treeterm_model::child_dialog::Listing;
use treeterm_model::session_store::SessionStore;

use crate::theme::colors;
use crate::ui;

pub struct AutoOpenDialog {
    model: Entity<AutoOpenModel>,
    /// The dialog has been closed; a later event must not close the dialog under it.
    closed: bool,
    _subscriptions: Vec<Subscription>,
}

impl AutoOpenDialog {
    fn new(session: Entity<SessionStore>, root_id: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let model = cx.new(|cx| AutoOpenModel::new(session, root_id, cx));
        let subscriptions = vec![
            cx.observe(&model, |_, _, cx| cx.notify()),
            cx.subscribe_in(&model, window, |dialog, _, event: &AutoOpenEvent, window, cx| match event {
                AutoOpenEvent::Opened | AutoOpenEvent::RootGone => dialog.close(window, cx),
            }),
        ];
        Self { model, closed: false, _subscriptions: subscriptions }
    }

    /// Close the dialog, once.
    fn close(&mut self, window: &mut Window, cx: &mut App) {
        if !self.closed {
            self.closed = true;
            window.close_dialog(cx);
        }
    }
}

impl Render for AutoOpenDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors(cx);
        let (primary, secondary, border, hover, accent, tertiary, bg) =
            (c.text_primary, c.text_secondary, c.border, c.bg_hover, c.accent, c.bg_tertiary, c.bg_primary);
        let model = self.model.read(cx);
        let root_name = model.root_name(cx);
        // `disabled={isProcessing}` on the row checkboxes, the include-unknown checkbox and
        // Cancel; the model ignores their clicks meanwhile.
        let processing = model.processing();
        let message = |text: &'static str| {
            div().p(px(16.)).flex().justify_center().text_size(px(ui::BODY)).text_color(secondary).child(text).into_any_element()
        };
        let rows: Vec<gpui_kit::AnyElement> = match model.rows() {
            Listing::Loading => vec![message("Detecting worktrees...")],
            Listing::Loaded(rows) if rows.is_empty() => vec![message("No recent worktrees found")],
            Listing::Loaded(rows) => {
                let count = rows.len();
                rows.into_iter()
                    .enumerate()
                    .map(|(i, row)| {
                        let checked = model.is_checked(&row.path);
                        let name = row.name();
                        let path = row.path;
                        // `.create-child-worktree-item`, indented 20px per level; the whole
                        // row toggles its (default, 13px) checkbox.
                        div()
                            .id(SharedString::from(format!("auto-open-{i}")))
                            .flex()
                            .items_center()
                            .pl(px(12. + 20. * row.depth as f32))
                            .pr(px(12.))
                            .py(px(10.))
                            .when(i + 1 < count, |d| d.border_b_1().border_color(border))
                            .cursor_pointer()
                            .hover(move |d| d.bg(hover))
                            .child(div().mr(px(8.)).child(ui::checkbox_box(checked, processing, 13.)))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(ui::BODY))
                                    .line_height(px(15.5))
                                    .font_weight(gpui_kit::FontWeight::MEDIUM)
                                    .text_color(primary)
                                    .child(name),
                            )
                            .child(div().max_w(px(180.)).truncate().text_size(px(ui::SMALL)).text_color(accent).child(row.branch))
                            .when(row.entry.is_none(), |d| {
                                d.child(
                                    div()
                                        .ml(px(8.))
                                        .flex_shrink_0()
                                        .px(px(6.))
                                        .py(px(2.))
                                        .rounded(px(3.))
                                        .bg(tertiary)
                                        .text_size(px(10.))
                                        .line_height(px(12.))
                                        .text_color(secondary)
                                        .child("unknown"),
                                )
                            })
                            .on_click(cx.listener(move |dialog, _, _, cx| {
                                dialog.model.update(cx, |m, cx| m.toggle(&path, cx));
                            }))
                            .into_any_element()
                    })
                    .collect()
            }
        };
        let error = model.error().map(|e| SharedString::from(e.to_owned()));
        let include = model.include_unknown();
        let can_confirm = model.can_confirm();
        let ok = model.confirm_label();
        ui::dialog_root(cx)
            .child(ui::dialog_header("Auto Open All Recent Worktrees", "x", primary, cx))
            .child(
                div()
                    .p(px(20.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .mb(px(16.))
                            .px(px(12.))
                            .py(px(8.))
                            .rounded(px(4.))
                            .bg(tertiary)
                            .text_size(px(ui::BODY))
                            .child(div().text_color(secondary).child("Under:"))
                            .child(div().text_color(primary).font_weight(gpui_kit::FontWeight::MEDIUM).child(root_name)),
                    )
                    .child(
                        div()
                            .id("auto-open-rows")
                            .flex()
                            .flex_col()
                            .max_h(px(200.))
                            .overflow_y_scroll()
                            .border_1()
                            .border_color(border)
                            .rounded(px(4.))
                            .bg(bg)
                            .children(rows),
                    )
                    .child(
                        div()
                            .id("include-unknown")
                            .mt(px(12.))
                            .mb(px(4.))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .text_size(px(ui::BODY))
                            .text_color(secondary)
                            .cursor_pointer()
                            .child(ui::checkbox_box(include, processing, 14.))
                            .child("Include unknown worktrees (not in recent history)")
                            .on_click(cx.listener(move |dialog, _, _, cx| {
                                dialog.model.update(cx, |m, cx| m.set_include_unknown(!include, cx));
                            })),
                    )
                    .when_some(error, |d, error| {
                        d.child(
                            div()
                                .mt(px(12.))
                                .px(px(12.))
                                .py(px(8.))
                                .rounded(px(4.))
                                .bg(gpui_kit::rgba(0xf14c4c1a))
                                .text_color(gpui_kit::rgb(0xf14c4c))
                                .text_size(px(ui::BODY))
                                .child(error),
                        )
                    }),
            )
            .child(
                ui::dialog_actions(cx)
                    .child(
                        ui::dialog_button("auto-open-cancel", "Cancel", ui::ButtonKind::Secondary, processing, cx)
                            .when(!processing, |b| b.on_click(cx.listener(|dialog, _, window, cx| dialog.close(window, cx)))),
                    )
                    .child(
                        ui::dialog_button("auto-open-ok", ok, ui::ButtonKind::Primary, !can_confirm, cx).when(can_confirm, |b| {
                            b.on_click(cx.listener(|dialog, _, _, cx| dialog.model.update(cx, |m, cx| m.confirm(cx))))
                        }),
                    ),
            )
    }
}

pub fn open(session: Entity<SessionStore>, root_id: String, window: &mut Window, cx: &mut App) {
    let view = cx.new(|cx| AutoOpenDialog::new(session, root_id, window, cx));
    window.open_dialog(cx, move |dialog, _window, cx| ui::dialog_frame(dialog, 400., cx).child(view.clone()));
}
