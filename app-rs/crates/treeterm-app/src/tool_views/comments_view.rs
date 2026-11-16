//! The Comments tab (`CommentsList.tsx`).
//!
//! Several of its CSS rules use `var(--border)`, which is not defined: those borders are not
//! drawn in Electron, so they are not drawn here either.

use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    App, Axis, ClipboardItem, Context, Div, ElementId, Entity, FocusHandle, FontWeight, Hsla, ScrollHandle, SharedString, Stateful,
    Subscription, Window, div, px, rgb, rgba,
};
use treeterm_core::review::{Freshness, Resolution};
use treeterm_model::session_store::SessionStore;
use treeterm_model::tool_tabs::comments::{CodeLine, CommentsModel, PushState, created_at};

use crate::theme::colors;
use crate::ui::{MONO, ReadingOrder, document_orders, normal_line, scrollbar};

pub struct CommentsView {
    model: Entity<CommentsModel>,
    /// For the workspace's git controller ("Push to GitHub").
    session: Entity<SessionStore>,
    workspace_id: String,
    /// Where the [`ReadingOrder`] of the view's text starts.
    first_order: u64,
    /// The tab's own focus, taken by a mouse-down in it (Electron: a mousedown on the page
    /// blurs the terminal), so that Cmd/Ctrl+C after a drag reaches the Kit root's Copy
    /// rather than a terminal that kept the focus.
    focus: FocusHandle,
    _observe: Subscription,
}

impl CommentsView {
    pub fn new(model: Entity<CommentsModel>, session: Entity<SessionStore>, workspace_id: String, cx: &mut Context<Self>) -> Self {
        let observe = cx.observe(&model, |_, _, cx| cx.notify());
        Self { model, session, workspace_id, first_order: document_orders(), focus: cx.focus_handle(), _observe: observe }
    }
}

/// A code row's height (`font-size: 12px; line-height: 1.5`).
const CODE_ROW: f32 = 18.;

/// The code around a comment (`.comments-code-context`). `overflow-x: auto`: a long line
/// scrolls sideways, under Chromium's 10px scrollbar, while a vertical wheel over the block
/// still scrolls the tab. The line numbers cannot be selected (`user-select: none`); the
/// code is one run of text, so that a selection copies its blank lines too.
fn code_block(id: SharedString, lines: Vec<CodeLine>, text: &mut ReadingOrder, window: &mut Window, cx: &mut App) -> Div {
    let c = colors(cx);
    let (secondary, background) = (c.text_secondary, c.bg_secondary);
    let scroll = window.use_keyed_state((ElementId::from(id.clone()), "scroll"), cx, |_, _| ScrollHandle::new()).read(cx).clone();
    let target = lines.iter().position(|line| line.target);
    let numbers = v_flex()
        .min_w(px(40.))
        .pr(px(12.))
        .flex_shrink_0()
        .items_end()
        .text_color(secondary)
        .children(lines.iter().map(|line| div().child(line.number.to_string())));
    let code = lines.into_iter().map(|line| line.text).collect::<Vec<_>>().join("\n");
    // As wide as the longest line (and at least the block), so that the commented line's band
    // runs under all of it.
    let rows = h_flex()
        .relative()
        .min_w_full()
        .px(px(8.))
        .items_start()
        .children(target.map(|row| {
            div().absolute().left_0().right_0().top(px(CODE_ROW * row as f32)).h(px(CODE_ROW)).bg(rgba(0xffff001a))
        }))
        .child(numbers)
        .child(div().flex_shrink_0().whitespace_nowrap().child(text.text("code", code)));
    let selector = id.clone();
    v_flex()
        .debug_selector(move || selector.to_string())
        .mb(px(8.))
        .rounded(px(4.))
        .bg(background)
        .font_family(MONO)
        .text_size(px(12.))
        .line_height(px(CODE_ROW))
        .child(
            v_flex()
                .id(id.clone())
                .items_start()
                .overflow_x_scroll()
                .restrict_scroll_to_axis()
                .track_scroll(&scroll)
                .py(px(4.))
                .child(rows),
        )
        .children(scrollbar(id, Axis::Horizontal, &scroll, cx))
}

/// The commented file's path (`.comments-card-file`), cut short with an ellipsis where it
/// does not fit; its `title` shows it whole.
fn file_path(id: SharedString, path: String, text: &mut ReadingOrder, cx: &App) -> Stateful<Div> {
    let title = SharedString::from(path.clone());
    div()
        .id(id)
        .flex_1()
        .min_w_0()
        .truncate()
        .font_family(MONO)
        .text_color(colors(cx).text_primary)
        .child(text.ellipsized("path", path))
        .tooltip(move |window, cx| Tooltip::new(title.clone()).build(window, cx))
}

/// The tab's borderless text buttons (`.comments-push-btn`, `.comments-reprompt-btn`, …):
/// secondary text, and [`lit_on_hover`] where Electron's `:hover` applies.
fn flat_button(id: impl Into<ElementId>, label: impl Into<SharedString>, size: f32, pad: (f32, f32), cx: &App) -> Stateful<Div> {
    div()
        .id(id)
        .flex_shrink_0()
        .px(px(pad.1))
        .py(px(pad.0))
        .rounded(px(3.))
        .text_size(px(size))
        .line_height(normal_line(size))
        .font_weight(FontWeight::NORMAL)
        .text_color(colors(cx).text_secondary)
        .cursor_pointer()
        .child(label.into())
}

/// The buttons' `:hover`: filled, with primary text. The active filter tab keeps its accent
/// (`.active` comes after `:hover`) and a disabled Push button stays as it is
/// (`:hover:not(:disabled)`), so neither gets it.
fn lit_on_hover(button: Stateful<Div>, cx: &App) -> Stateful<Div> {
    let c = colors(cx);
    let (hover, primary) = (c.bg_hover, c.text_primary);
    button.hover(move |d| d.bg(hover).text_color(primary))
}

impl Render for CommentsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors(cx);
        let (error, tertiary, hover_bg) = (c.error, c.bg_tertiary, c.bg_hover);
        let (secondary, primary, accent, border) = (c.text_secondary, c.text_primary, c.accent, c.border);
        // Restricted to its axis, so that a sideways swipe over a code block scrolls only the block.
        let root =
            v_flex().id("comments-tab").track_focus(&self.focus).size_full().p(px(8.)).overflow_y_scroll().restrict_scroll_to_axis();
        let empty = |text: String| {
            div()
                .p(px(24.))
                .flex()
                .justify_center()
                .text_size(px(13.))
                .line_height(normal_line(13.))
                .text_color(secondary)
                .child(text)
        };
        let model = self.model.read(cx);
        let comments = match model.comments(cx) {
            Ok(comments) => comments,
            Err(message) => return root.child(empty(message.to_string()).text_color(error)),
        };
        if comments.is_empty() {
            return root.child(empty("No review comments yet".to_owned()));
        }
        let filter = model.filter();
        let (push, failure, prompt, expanded) =
            (model.push_state().clone(), model.error().map(str::to_owned), model.prompt(cx), model.prompt_expanded());
        let unprompted = comments.iter().filter(|c| c.resolution == Resolution::Open).count();
        let prompted = comments.len() - unprompted;
        let shown: Vec<_> = comments.iter().filter(|c| c.resolution == filter).cloned().collect();
        let contexts: Vec<_> = shown.iter().map(|comment| model.code_context(comment)).collect();
        // The paths, code, comments and prompt can be selected and copied, as in the browser.
        let mut text = ReadingOrder::after(self.first_order);

        let tab = |id: &'static str, label: String, target: Resolution, cx: &mut Context<Self>| {
            let active = filter == target;
            flat_button(id, label, 12., (3., 10.), cx)
                .font_weight(FontWeight::MEDIUM)
                .when(active, |d| d.bg(accent).text_color(primary))
                .when(!active, |d| lit_on_hover(d, cx))
                .on_click(cx.listener(move |view, _, _, cx| view.model.update(cx, |m, cx| m.set_filter(target, cx))))
        };
        let pushing = push == PushState::Pushing;
        let can_push = unprompted > 0 && !pushing;
        let header = h_flex()
            .justify_between()
            .gap(px(8.))
            .px(px(4.))
            .py(px(8.))
            .mb(px(4.))
            .child(
                h_flex()
                    .gap(px(4.))
                    .child(tab("comments-unprompted", format!("Unprompted ({unprompted})"), Resolution::Open, cx))
                    .child(tab("comments-prompted", format!("Prompted ({prompted})"), Resolution::Addressed, cx)),
            )
            .child(
                flat_button(
                    "comments-push",
                    if pushing { "Pushing…".to_owned() } else { format!("Push to GitHub ({unprompted})") },
                    12.,
                    (3., 10.),
                    cx,
                )
                .when(!can_push, |d| d.opacity(0.5).cursor_default())
                .when(can_push, |d| lit_on_hover(d, cx))
                .tooltip(|window, cx| {
                    Tooltip::new("Push unprompted comments to GitHub as inline PR review comments").build(window, cx)
                })
                .when(can_push, |d| {
                    d.on_click(cx.listener(|view, _, _, cx| {
                        let controller = view.session.read(cx).git_controller(&view.workspace_id).cloned();
                        view.model.update(cx, |m, cx| m.push_to_github(controller, cx));
                    }))
                }),
            );
        let status = |text: String, color: Hsla| {
            div().px(px(8.)).py(px(4.)).mb(px(4.)).text_size(px(12.)).line_height(normal_line(12.)).text_color(color).child(text)
        };
        let mut cards = v_flex().gap(px(8.));
        if shown.is_empty() {
            let which = match filter {
                Resolution::Open => "unprompted",
                Resolution::Addressed => "prompted",
            };
            cards = cards.child(empty(format!("No {which} comments")));
        }
        for (comment, context) in shown.into_iter().zip(contexts) {
            let addressed = comment.resolution == Resolution::Addressed;
            let outdated = comment.freshness == Freshness::Outdated;
            let (delete_id, prompt_id) = (comment.id.clone(), comment.id.clone());
            let goto = comment.clone();
            let mut card = v_flex()
                .p(px(12.))
                .rounded(px(4.))
                .border_1()
                .border_color(if outdated { rgb(0xf0a000).into() } else { border })
                .bg(if outdated { rgba(0xf0a0000d).into() } else { tertiary })
                .when(addressed, |d| d.opacity(0.7))
                .child(
                    h_flex()
                        .gap(px(8.))
                        .mb(px(8.))
                        .text_size(px(12.))
                        .line_height(normal_line(12.))
                        .child(file_path(
                            SharedString::from(format!("comment-path-{}", comment.id)),
                            comment.file_path.clone(),
                            &mut text,
                            cx,
                        ))
                        .child(
                            div()
                                .font_family(MONO)
                                .text_color(accent)
                                .child(format!("L{} ({})", comment.line_number, comment.side.as_str())),
                        )
                        .when(outdated, |d| {
                            d.child(
                                div()
                                    .id(SharedString::from(format!("comment-outdated-{}", comment.id)))
                                    .px(px(6.))
                                    .py(px(2.))
                                    .rounded(px(3.))
                                    .bg(rgb(0xf0a000))
                                    .text_size(px(10.))
                                    .line_height(normal_line(10.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(0x1e1e1e))
                                    .child("OUTDATED")
                                    .tooltip(|window, cx| Tooltip::new("This comment may be outdated").build(window, cx)),
                            )
                        })
                        .child({
                            let hover = hover_bg;
                            div()
                                .id(SharedString::from(format!("comment-delete-{}", comment.id)))
                                .ml_auto()
                                .size(px(20.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(4.))
                                .text_size(px(18.))
                                .text_color(secondary)
                                .cursor_pointer()
                                .hover(move |d| d.bg(hover).text_color(rgb(0xf14c4c)))
                                .child("×")
                                .tooltip(|window, cx| Tooltip::new("Delete comment").build(window, cx))
                                .on_click(cx.listener(move |view, _, _, cx| {
                                    view.model.update(cx, |m, cx| m.delete(&delete_id, cx));
                                }))
                        }),
                );
            if let Some(lines) = context {
                card = card.child(code_block(
                    SharedString::from(format!("comment-code-{}", comment.id)),
                    lines,
                    &mut text,
                    window,
                    cx,
                ));
            }
            card = card
                .child(
                    div()
                        .mb(px(8.))
                        .text_size(px(13.))
                        .line_height(px(19.5))
                        .text_color(primary)
                        .child(text.text(SharedString::from(format!("comment-text-{}", comment.id)), comment.text.clone())),
                )
                .child(
                    h_flex()
                        .gap(px(8.))
                        .text_size(px(11.))
                        .line_height(normal_line(11.))
                        .child(div().flex_1().min_w_0().text_color(secondary).child(created_at(&comment)))
                        .child(
                            lit_on_hover(
                                flat_button(
                                    SharedString::from(format!("comment-prompt-{}", comment.id)),
                                    if addressed { "Re-prompt" } else { "Prompt" },
                                    11.,
                                    (3., 8.),
                                    cx,
                                ),
                                cx,
                            )
                            .tooltip(move |window, cx| {
                                Tooltip::new(if addressed {
                                    "Re-send this comment to the AI harness"
                                } else {
                                    "Send this comment to the AI harness"
                                })
                                .build(window, cx)
                            })
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.model.update(cx, |m, cx| m.prompt_comment(&prompt_id, cx));
                            })),
                        )
                        .child(
                            lit_on_hover(
                                flat_button(SharedString::from(format!("comment-goto-{}", comment.id)), "Go to file", 11., (3., 8.), cx),
                                cx,
                            )
                            .tooltip(|window, cx| Tooltip::new("Open file at this line").build(window, cx))
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.model.update(cx, |m, cx| m.go_to_file(&goto, cx));
                            })),
                        ),
                );
            cards = cards.child(card);
        }
        let prompt_panel = (!prompt.is_empty()).then(|| {
            let copy = prompt.clone();
            v_flex()
                .mt(px(12.))
                .rounded(px(4.))
                .bg(tertiary)
                .overflow_hidden()
                .child(
                    h_flex()
                        .id("comments-prompt-toggle")
                        .justify_between()
                        .px(px(12.))
                        .py(px(8.))
                        .border_b_1()
                        // `border-bottom-color: var(--border)` is invalid, so an open preview's
                        // rule takes the text colour.
                        .border_color(if expanded { secondary } else { gpui_kit::transparent_black() })
                        .text_size(px(12.))
                        .line_height(normal_line(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(secondary)
                        .cursor_pointer()
                        .hover(move |d| {
                            let d = d.text_color(primary);
                            if expanded { d.border_color(primary) } else { d }
                        })
                        .child(format!("{} Generated Prompt", if expanded { "▼" } else { "▶" }))
                        .child(
                            lit_on_hover(flat_button("comments-copy", "Copy to Clipboard", 11., (2., 8.), cx), cx)
                                .tooltip(|window, cx| Tooltip::new("Copy to clipboard").build(window, cx))
                                .on_click(move |_, _, cx| {
                                    cx.stop_propagation();
                                    cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()));
                                }),
                        )
                        .on_click(cx.listener(|view, _, _, cx| view.model.update(cx, |m, cx| m.toggle_prompt(cx)))),
                )
                .when(expanded, |d| {
                    d.child(
                        div()
                            .id("comments-prompt")
                            .max_h(px(300.))
                            .overflow_y_scroll()
                            .p(px(12.))
                            .font_family(MONO)
                            .text_size(px(12.))
                            .line_height(px(18.))
                            .text_color(secondary)
                            .child(text.text("text", prompt)),
                    )
                })
        });
        root.child(header)
            .when_some(failure, |d, text| d.child(status(text, error)))
            .when_some(
                match push {
                    PushState::Done(message) => Some(message),
                    PushState::Idle | PushState::Pushing => None,
                },
                |d, message| d.child(status(message, secondary)),
            )
            .child(cards)
            .children(prompt_panel)
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::base::TextSelection;
    use gpui_kit::{
        Bounds, Modifiers, MouseButton, Pixels, Point, ScrollDelta, ScrollEvent, ScrollSource, TestAppContext, TouchPhase,
        VisualTestContext, WindowOptions, point,
    };
    use treeterm_model::tool_tabs::comments::code_context;

    use super::*;
    use crate::ui::draw_settled;

    /// A 300px card: the code around line 2 of `file`, then the comment.
    struct Card {
        file: String,
    }

    impl Render for Card {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let mut text = ReadingOrder::after(0);
            let lines = code_context(&self.file, 2).expect("line 2");
            v_flex()
                .w(px(300.))
                .child(code_block("code".into(), lines, &mut text, window, cx))
                .child(text.text("comment", "the comment"))
        }
    }

    fn open<V: Render>(view: impl FnOnce() -> V + 'static, cx: &mut TestAppContext) -> &mut VisualTestContext {
        let window = cx.update(|cx| {
            gpui_kit::init(cx);
            crate::theme::install(cx);
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| cx.new(|_| view())).expect("open a window").0
        });
        let cx = VisualTestContext::from_window(window, cx).into_mut();
        draw(cx);
        cx
    }

    fn card<'a>(file: &str, cx: &'a mut TestAppContext) -> &'a mut VisualTestContext {
        let file = file.to_owned();
        open(|| Card { file }, cx)
    }

    fn draw(cx: &mut VisualTestContext) {
        cx.run_until_parked();
        draw_settled(cx);
    }

    /// The middle of code row `row` (from 0), below the block's 4px padding.
    fn row(row: usize) -> Pixels {
        px(4. + CODE_ROW * row as f32 + 9.)
    }

    /// Whether `at` is over selectable text (hit testing follows the pointer).
    fn selectable_at(at: Point<Pixels>, cx: &mut VisualTestContext) -> bool {
        cx.simulate_mouse_move(at, None, Modifiers::none());
        cx.update(|window, cx| TextSelection::is_selectable_at(at, window, cx))
    }

    /// The first point at height `row_y` with selectable text, past the line numbers.
    fn line_start(row_y: Pixels, cx: &mut VisualTestContext) -> Point<Pixels> {
        (0..300).map(|x| point(px(x as f32), row_y)).find(|at| selectable_at(*at, cx)).expect("text on the row")
    }

    fn wheel(at: Point<Pixels>, delta: Point<Pixels>, cx: &mut VisualTestContext) {
        let modifiers = Modifiers::none();
        cx.simulate_event(ScrollEvent {
            position: at,
            delta: ScrollDelta::Pixels(delta),
            modifiers,
            touch_phase: TouchPhase::Moved,
            source: ScrollSource::Pointer,
        });
        draw(cx);
    }

    /// Drag from `from` to `to`; the text the Kit root's Copy copies then.
    fn copy_drag(from: Point<Pixels>, to: Point<Pixels>, cx: &mut VisualTestContext) -> String {
        cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
        draw(cx);
        cx.update(TextSelection::selected_text)
    }

    fn code(cx: &mut VisualTestContext) -> Bounds<Pixels> {
        cx.debug_bounds("code").expect("the code is drawn")
    }

    /// 200 characters and `END`.
    fn long_line() -> String {
        format!("{}END", "x".repeat(200))
    }

    /// `.comments-code-context { overflow-x: auto }`: the end of a long line can be scrolled
    /// to, and a vertical wheel over the block is not turned into a sideways scroll.
    #[gpui_kit::test]
    fn a_long_code_line_scrolls_sideways_and_a_vertical_wheel_leaves_it(cx: &mut TestAppContext) {
        let cx = card(&format!("short\n{}\nthird", long_line()), cx);
        let short = line_start(row(0), cx);

        wheel(short, point(px(0.), px(-100.)), cx);
        assert_eq!(line_start(row(0), cx), short, "a vertical wheel leaves the code where it was");
        assert!(!copy_drag(point(px(150.), row(1)), point(px(299.), row(1)), cx).contains("END"), "the end is out of sight");

        wheel(short, point(px(-2000.), px(0.)), cx);
        assert_ne!(line_start(row(0), cx), short, "the code scrolled sideways");
        let copied = copy_drag(point(px(150.), row(1)), point(px(299.), row(1)), cx);
        assert!(copied.contains("xEND"), "the end of the long line: {copied:?}");
    }

    /// Code that fits its block shows no scrollbar.
    #[gpui_kit::test]
    fn code_that_fits_shows_no_scrollbar(cx: &mut TestAppContext) {
        let cx = card("one\ntwo\nthree", cx);
        assert_eq!(code(cx).size.height, px(4. + 3. * CODE_ROW + 4.));
    }

    /// Chromium's 10px scrollbar shows under the code once a line overflows the block, which
    /// it makes 10px taller; its thumb scrolls the code to the end of the line, and a press
    /// on its track pages it.
    #[gpui_kit::test]
    fn a_long_code_line_shows_a_scrollbar_under_the_code(cx: &mut TestAppContext) {
        let cx = card(&format!("short\n{}\nthird", long_line()), cx);
        let block = code(cx);
        assert_eq!(block.size.height, px(4. + 3. * CODE_ROW + 4. + 10.), "the bar under the code");
        let bar = |x: f32| point(px(x), block.bottom() - px(5.));
        let end_in_sight =
            |cx: &mut VisualTestContext| copy_drag(point(px(150.), row(1)), point(px(299.), row(1)), cx).contains("xEND");

        // The thumb, at the start of the bar, dragged to its end.
        cx.simulate_mouse_move(bar(5.), None, Modifiers::none());
        cx.simulate_mouse_down(bar(5.), MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_move(bar(30.), MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_move(bar(299.), MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_up(bar(299.), MouseButton::Left, Modifiers::none());
        draw(cx);
        assert!(end_in_sight(cx), "the end of the long line");

        // A press on the track before the thumb pages the code back.
        cx.simulate_mouse_down(bar(5.), MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_up(bar(5.), MouseButton::Left, Modifiers::none());
        draw(cx);
        assert!(!end_in_sight(cx), "paged back");
    }

    /// The code and the comment can be selected and copied in reading order, a line each,
    /// without the line numbers (`user-select: none`).
    #[gpui_kit::test]
    fn a_drag_over_the_code_copies_its_lines_without_their_numbers(cx: &mut TestAppContext) {
        let cx = card("one\ntwo\nthree", cx);
        let from = line_start(row(0), cx);
        assert_eq!(copy_drag(from, point(px(299.), row(2)), cx), "one\ntwo\nthree");
        // On to the comment under the block.
        assert_eq!(copy_drag(from, point(px(299.), row(3) + px(8.)), cx), "one\ntwo\nthree\nthe comment");
    }

    /// A blank code line is copied too, as Chromium copies it from the `<pre>`.
    #[gpui_kit::test]
    fn a_drag_over_a_blank_code_line_copies_it(cx: &mut TestAppContext) {
        let cx = card("one\n\nthree", cx);
        let from = line_start(row(0), cx);
        assert_eq!(copy_drag(from, point(px(299.), row(2)), cx), "one\n\nthree");
    }

    /// A card's header row, 200px wide: the commented file's path, then the line.
    struct Header {
        path: String,
    }

    impl Render for Header {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let mut text = ReadingOrder::after(0);
            h_flex()
                .w(px(200.))
                .gap(px(8.))
                .child(file_path("path".into(), self.path.clone(), &mut text, cx).debug_selector(|| "path".to_owned()))
                .child(div().child("L12 (RIGHT)"))
        }
    }

    /// `.comments-card-file`: a path too long for its row is cut short with an ellipsis, and
    /// can still be selected: a drag over the part shown copies that part, and one taking in
    /// the ellipsis copies the rest of the path too.
    #[gpui_kit::test]
    fn a_path_cut_short_by_its_ellipsis_can_be_selected_and_copied_whole(cx: &mut TestAppContext) {
        let path = format!("src/{}main.rs", "nested/".repeat(20));
        let shown = path.clone();
        let cx = open(move || Header { path: shown }, cx);
        // Some 1400px of path in its 100-odd.
        let bounds = cx.debug_bounds("path").expect("the path is drawn");
        let (from, end) = (point(bounds.left() + px(1.), bounds.center().y), point(bounds.right() - px(1.), bounds.center().y));
        assert!(selectable_at(from, cx), "the path is selectable");

        assert_eq!(copy_drag(from, end, cx), path, "the whole path");
        let start = copy_drag(from, point(bounds.left() + px(60.), bounds.center().y), cx);
        assert!(!start.is_empty() && path.starts_with(&start) && start.len() < 12, "the start shown: {start:?}");
    }

    /// A path that fits is copied as shown.
    #[gpui_kit::test]
    fn a_path_that_fits_is_copied_as_shown(cx: &mut TestAppContext) {
        let cx = open(|| Header { path: "src/a.rs".to_owned() }, cx);
        let bounds = cx.debug_bounds("path").expect("the path is drawn");
        let from = point(bounds.left() + px(1.), bounds.center().y);
        assert_eq!(copy_drag(from, point(bounds.right() - px(1.), bounds.center().y), cx), "src/a.rs");
    }
}
