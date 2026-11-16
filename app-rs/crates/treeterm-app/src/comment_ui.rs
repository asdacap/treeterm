//! Review comments as `CommentDisplay.tsx` and `CommentInput.tsx` draw them, shared by the
//! Review tab's diff and the Files tab's viewer. What a button does is the caller's.

use std::rc::Rc;

use gpui_kit::base::SelectableText;
use gpui_kit::component::input::{Enter, Textarea, TextareaState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, Entity, Focusable as _, FontWeight, KeyDownEvent, SharedString, Window, div, px, rgb, rgba};
use treeterm_core::review::{Freshness, ReviewComment};
use treeterm_model::review::GitOp;

use crate::theme::colors;

/// Acts on a comment (by id) or on the comment box's text.
pub type TextAction = Rc<dyn Fn(&str, &mut App)>;
/// Scrolls the viewer to a 1-based line.
pub type LineAction = Rc<dyn Fn(u32, &mut App)>;

/// What the comment buttons do.
#[derive(Clone)]
pub struct CommentActions {
    pub delete: TextAction,
    pub cancel: Rc<dyn Fn(&mut App)>,
    pub submit: TextAction,
}

/// Where a comment is shown.
#[derive(Clone)]
pub enum Placement {
    /// Under its line in a diff (`.pierre-diff-annotation`, `hideLineRef`): a card.
    Diff,
    /// With its line's other comments in the file viewer (`.inline-comment-display-group`):
    /// compact, headed by its line, which `reveal` scrolls the viewer to.
    Viewer { reveal: LineAction },
}

const OUTDATED: u32 = 0xf0a000;
const ERROR_RED: u32 = 0xf85149;

/// `.comment-delete-btn`, titled "Delete comment".
fn delete_button(id: &str, actions: &CommentActions, cx: &App) -> AnyElement {
    let c = colors(cx);
    let (secondary, hover) = (c.text_secondary, c.bg_hover);
    let ui_font = gpui_kit::component::ActiveTheme::theme(cx).font_family.clone();
    let (delete, owned) = (actions.delete.clone(), id.to_owned());
    div()
        .id(SharedString::from(format!("delete-comment-{id}")))
        .font_family(ui_font)
        .size(px(20.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.))
        .text_size(px(18.))
        .text_color(secondary)
        .cursor_pointer()
        .hover(move |d| d.bg(hover).text_color(rgb(0xf14c4c)))
        .tooltip(|window, cx| Tooltip::new("Delete comment").build(window, cx))
        .child("×")
        .on_click(move |_, _, cx| delete(&owned, cx))
        .into_any_element()
}

/// `CommentDisplay`: the text, its time, and × to delete. The header carries the line
/// (`Placement::Viewer`) and the "Outdated" badge; with neither, × sits beside the text.
pub fn comment_display(comment: &ReviewComment, placement: &Placement, actions: &CommentActions, cx: &App) -> AnyElement {
    let c = colors(cx);
    let (secondary, primary, tertiary, border, accent) = (c.text_secondary, c.text_primary, c.bg_tertiary, c.border, c.accent);
    let outdated = comment.freshness == Freshness::Outdated;
    let reveal = match placement {
        Placement::Diff => None,
        Placement::Viewer { reveal } => Some(reveal.clone()),
    };
    let header = reveal.is_some() || outdated;
    let card = match placement {
        // `.comment-display` (`.outdated`).
        Placement::Diff => v_flex()
            .mb(px(8.))
            .p(px(12.))
            .rounded(px(4.))
            .border_1()
            .border_color(if outdated { rgb(OUTDATED).into() } else { border })
            .bg(if outdated { rgba(0xf0a0000d).into() } else { tertiary }),
        // `.inline-comment-display-group .comment-display`: no border, 4px 8px.
        Placement::Viewer { .. } => v_flex().px(px(8.)).py(px(4.)).when(outdated, |d| d.bg(rgba(0xf0a0000d))),
    };
    let gap = match placement {
        Placement::Diff => px(8.),
        Placement::Viewer { .. } => px(2.),
    };
    let line = comment.line_number;
    card.when(header, |d| {
        d.child(
            h_flex()
                .gap(px(8.))
                .items_center()
                .mb(gap)
                .text_size(px(12.))
                .when_some(reveal, |d, reveal| {
                    // `.comment-line-ref`.
                    d.child(
                        div()
                            .id(SharedString::from(format!("comment-line-{}", comment.id)))
                            .font_family(crate::ui::MONO)
                            .text_color(accent)
                            .cursor_pointer()
                            .hover(|d| d.underline())
                            .child(format!("Line {} ({})", comment.line_number, comment.side.as_str()))
                            .on_click(move |_, _, cx| reveal(line, cx)),
                    )
                })
                .when(outdated, |d| {
                    // `.comment-outdated-badge`.
                    d.child(
                        div()
                            .id(SharedString::from(format!("comment-outdated-{}", comment.id)))
                            .px(px(6.))
                            .py(px(2.))
                            .rounded(px(3.))
                            .bg(rgb(OUTDATED))
                            .text_color(rgb(0x1e1e1e))
                            .text_size(px(10.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .tooltip(|window, cx| Tooltip::new("This comment may be outdated").build(window, cx))
                            .child("OUTDATED"),
                    )
                })
                .child(div().ml_auto().child(delete_button(&comment.id, actions, cx))),
        )
    })
    .child(
        h_flex()
            .items_start()
            .mb(gap)
            .text_size(px(13.))
            .line_height(gpui_kit::relative(1.5))
            .text_color(primary)
            // `white-space: pre-wrap`, and selectable as page text.
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(SelectableText::new(SharedString::from(format!("comment-text-{}", comment.id)), comment.text.clone())),
            )
            .when(!header, |d| d.child(div().ml(px(8.)).child(delete_button(&comment.id, actions, cx)))),
    )
    .child(div().text_size(px(11.)).text_color(secondary).child(treeterm_core::clock::locale_date_time(comment.created_at)))
    .into_any_element()
}

/// `CommentInput`: "Comment on line N", the text (Cmd/Ctrl+Enter submits, Esc cancels),
/// Cancel and Add Comment, and why the last save failed.
///
/// Electron submits on Enter with Cmd or Ctrl held, on every platform, and types no newline.
/// The Kit binds only the platform's own (Cmd on macOS, Ctrl elsewhere) to its secondary
/// Enter, which a textarea answers with a newline: that action is taken before the textarea
/// sees it, and the other modifier, which no binding takes, arrives as a key press.
pub fn comment_input(
    input: &Entity<TextareaState>,
    has_text: bool,
    label: String,
    adding: &GitOp,
    actions: &CommentActions,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let c = colors(cx);
    let (secondary, primary, tertiary, border, hover, accent, bg_primary) =
        (c.text_secondary, c.text_primary, c.bg_tertiary, c.border, c.bg_hover, c.accent, c.bg_primary);
    let ui_font = gpui_kit::component::ActiveTheme::theme(cx).font_family.clone();
    let saving = *adding == GitOp::Running;
    let enabled = has_text && !saving;
    let focused = input.read(cx).focus_handle(cx).is_focused(window);
    let (on_escape, on_cancel) = (actions.cancel.clone(), actions.cancel.clone());
    let submit = {
        let (submit, text) = (actions.submit.clone(), input.clone());
        Rc::new(move |cx: &mut App| {
            let value = text.read(cx).value().to_string();
            submit(&value, cx);
        })
    };
    let (on_secondary_enter, on_enter_key, on_submit) = (submit.clone(), submit.clone(), submit);
    v_flex()
        .child(div().mb(px(8.)).text_size(px(13.)).text_color(secondary).child(label))
        .child(
            div()
                .capture_action(move |action: &Enter, _, cx| {
                    if action.secondary {
                        cx.stop_propagation();
                        on_secondary_enter(cx);
                    }
                })
                .on_key_down(move |event: &KeyDownEvent, _, cx| {
                    let modifiers = &event.keystroke.modifiers;
                    match event.keystroke.key.as_str() {
                        "escape" => on_escape(cx),
                        "enter" if modifiers.control || modifiers.platform => {
                            cx.stop_propagation();
                            on_enter_key(cx);
                        }
                        _ => {}
                    }
                })
                .child(
                    Textarea::new(input)
                        .appearance(false)
                        .w_full()
                        .min_h(px(60.))
                        .px(px(12.))
                        .py(px(8.))
                        .text_size(px(13.))
                        .bg(bg_primary)
                        .border_1()
                        .border_color(if focused { accent } else { border })
                        .rounded(px(4.)),
                ),
        )
        .child(
            h_flex()
                .gap(px(8.))
                .mt(px(12.))
                .child(
                    div()
                        .id("comment-cancel")
                        .font_family(ui_font.clone())
                        .px(px(16.))
                        .py(px(6.))
                        .rounded(px(4.))
                        .text_size(px(13.))
                        .bg(tertiary)
                        .text_color(primary)
                        .cursor_pointer()
                        .hover(move |d| d.bg(hover))
                        .child("Cancel")
                        .on_click(move |_, _, cx| on_cancel(cx)),
                )
                .child(
                    div()
                        .id("comment-submit")
                        .font_family(ui_font)
                        .px(px(16.))
                        .py(px(6.))
                        .rounded(px(4.))
                        .text_size(px(13.))
                        .bg(accent)
                        .text_color(gpui_kit::white())
                        .when(!enabled, |d| d.opacity(0.5))
                        .when(enabled, |d| d.cursor_pointer().hover(|d| d.bg(rgb(0x006cbf))))
                        .child("Add Comment")
                        .when(enabled, |d| d.on_click(move |_, _, cx| on_submit(cx))),
                ),
        )
        .when_some(
            match adding {
                GitOp::Failed(message) => Some(message.clone()),
                GitOp::Idle | GitOp::Running => None,
            },
            |d, message| d.child(div().mt(px(8.)).text_size(px(12.)).text_color(rgb(ERROR_RED)).child(message)),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use gpui_kit::base::TextSelection;
    use gpui_kit::{
        Context, FocusHandle, KeyUpEvent, Keystroke, Modifiers, MouseButton, Pixels, Point, Render, TestAppContext,
        VisualTestContext, WindowOptions, point,
    };
    use treeterm_core::review::{CommitAnchor, LineAnchor, Resolution, Side};

    use super::*;

    const COPY: &str = if cfg!(target_os = "macos") { "cmd-c" } else { "ctrl-c" };

    /// What the comment's buttons and keys asked for.
    #[derive(Debug, PartialEq)]
    enum Asked {
        Submit(String),
        Cancel,
        Delete(String),
    }

    /// A 400px column: `comment` as `placement` shows it, over the comment box. The window's
    /// focus is the app's: a mouse-down anywhere else takes it from the comment box.
    struct Host {
        focus: FocusHandle,
        comment: ReviewComment,
        placement: Placement,
        input: Entity<TextareaState>,
        actions: CommentActions,
    }

    impl Render for Host {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            v_flex()
                .w(px(400.))
                .h_full()
                .track_focus(&self.focus)
                .child(comment_display(&self.comment, &self.placement, &self.actions, cx))
                .child(comment_input(&self.input, true, "Comment on line 3".to_owned(), &GitOp::Idle, &self.actions, window, cx))
        }
    }

    fn comment(text: &str, freshness: Freshness) -> ReviewComment {
        ReviewComment {
            id: "c1".to_owned(),
            file_path: "src/a.rs".to_owned(),
            line_number: 3,
            text: text.to_owned(),
            commit: CommitAnchor::Head("abc".to_owned()),
            line: LineAnchor::Untracked,
            created_at: 0,
            freshness,
            resolution: Resolution::Open,
            side: Side::Modified,
        }
    }

    fn draw(cx: &mut VisualTestContext) {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
    }

    /// An active window (in GPUI Kit's root, which selects and copies text) showing `comment`
    /// over the focused comment box; what was asked of the buttons and keys.
    fn host(
        comment: ReviewComment,
        placement: Placement,
        cx: &mut TestAppContext,
    ) -> (Entity<Host>, Rc<RefCell<Vec<Asked>>>, &mut VisualTestContext) {
        let asked = Rc::new(RefCell::new(Vec::new()));
        let actions = {
            let (submit, cancel, delete) = (asked.clone(), asked.clone(), asked.clone());
            CommentActions {
                delete: Rc::new(move |id, _| delete.borrow_mut().push(Asked::Delete(id.to_owned()))),
                cancel: Rc::new(move |_| cancel.borrow_mut().push(Asked::Cancel)),
                submit: Rc::new(move |text, _| submit.borrow_mut().push(Asked::Submit(text.to_owned()))),
            }
        };
        let (window, view) = cx.update(|cx| {
            gpui_kit::init(cx);
            crate::theme::install(cx);
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                let input = cx.new(|cx| TextareaState::new(window, cx));
                cx.new(|cx| Host { focus: cx.focus_handle(), comment, placement, input, actions })
            })
            .expect("open a window")
        });
        let cx = VisualTestContext::from_window(window, cx).into_mut();
        cx.update(|window, cx| {
            window.activate_window();
            let input = view.read(cx).input.clone();
            input.update(cx, |input, cx| input.focus(window, cx));
        });
        draw(cx);
        (view, asked, cx)
    }

    fn typed(view: &Entity<Host>, cx: &mut VisualTestContext) -> String {
        cx.update(|_, cx| view.read(cx).input.read(cx).value().to_string())
    }

    fn take(asked: &Rc<RefCell<Vec<Asked>>>) -> Vec<Asked> {
        std::mem::take(&mut *asked.borrow_mut())
    }

    /// Press and release `key` as a platform reports it: a key no binding takes types nothing
    /// (`simulate_keystrokes` would type Enter's newline, as an IME).
    fn press(key: &str, cx: &mut VisualTestContext) {
        let keystroke = cx.update(|_, cx| Keystroke::parse_with_os(key, cx.platform_info().os).expect("a keystroke"));
        cx.simulate_event(KeyDownEvent { keystroke: keystroke.clone(), is_held: false, prefer_character_input: false });
        cx.simulate_event(KeyUpEvent { keystroke });
        draw(cx);
    }

    /// Electron: Enter with Cmd (meta) or Ctrl held submits, on every platform and whatever
    /// else is held, and `preventDefault`s the newline.
    #[gpui_kit::test]
    fn cmd_or_ctrl_enter_submits_on_every_platform_and_types_no_newline(cx: &mut TestAppContext) {
        let (view, asked, cx) = host(comment("noted", Freshness::Current), Placement::Diff, cx);
        cx.simulate_input("looks off");
        draw(cx);
        for key in ["cmd-enter", "ctrl-enter", "cmd-shift-enter", "ctrl-alt-enter"] {
            press(key, cx);
            assert_eq!(take(&asked), vec![Asked::Submit("looks off".to_owned())], "{key} submits once");
            assert_eq!(typed(&view, cx), "looks off", "{key} types no newline");
        }
    }

    /// Enter and Shift+Enter type a newline and submit nothing; Esc cancels.
    #[gpui_kit::test]
    fn enter_and_shift_enter_type_a_newline_and_escape_cancels(cx: &mut TestAppContext) {
        let (view, asked, cx) = host(comment("noted", Freshness::Current), Placement::Diff, cx);
        cx.simulate_input("a");
        press("enter", cx);
        cx.simulate_input("b");
        press("shift-enter", cx);
        cx.simulate_input("c");
        draw(cx);
        assert_eq!(typed(&view, cx), "a\nb\nc");
        assert_eq!(take(&asked), vec![]);

        press("escape", cx);
        assert_eq!(take(&asked), vec![Asked::Cancel]);
    }

    /// The top left of the window's first selectable text (hit testing follows the pointer).
    fn first_text(cx: &mut VisualTestContext) -> Point<Pixels> {
        (0..200)
            .flat_map(|y| (0..200).map(move |x| point(px(x as f32 * 2.), px(y as f32))))
            .find(|at| {
                cx.simulate_mouse_move(*at, None, Modifiers::none());
                cx.update(|window, cx| TextSelection::is_selectable_at(*at, window, cx))
            })
            .expect("selectable text")
    }

    /// Drag from `from` to `to`, then Cmd/Ctrl+C: what the clipboard holds.
    fn drag_and_copy(from: Point<Pixels>, to: Point<Pixels>, cx: &mut VisualTestContext) -> Option<String> {
        cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
        draw(cx);
        cx.simulate_keystrokes(COPY);
        cx.read_from_clipboard().and_then(|item| item.text())
    }

    /// Three lines, the second blank, with runs of spaces.
    const TEXT: &str = "first  line\n\n  third line";

    /// Drag over [`TEXT`] shown in `placement`, after typing in the comment box, then
    /// Cmd/Ctrl+C: what the clipboard holds.
    fn copy_the_comment(placement: Placement, cx: &mut TestAppContext) -> Option<String> {
        let (_, asked, cx) = host(comment(TEXT, Freshness::Current), placement, cx);
        cx.simulate_input("draft");
        draw(cx);
        let start = first_text(cx);
        // Past the end of the third line: a line is 19.5px (13px text, `line-height: 1.5`).
        let copied = drag_and_copy(start, start + point(px(300.), px(45.)), cx);
        assert_eq!(take(&asked), vec![]);
        copied
    }

    /// Electron's comment text is page text (`white-space: pre-wrap`): a drag selects it and
    /// Cmd/Ctrl+C copies it with its line breaks, also right after typing in the comment box
    /// (a mousedown on the text blurs it).
    #[gpui_kit::test]
    fn the_comment_text_can_be_selected_and_copied_in_the_diff(cx: &mut TestAppContext) {
        assert_eq!(copy_the_comment(Placement::Diff, cx).as_deref(), Some(TEXT));
    }

    #[gpui_kit::test]
    fn the_comment_text_can_be_selected_and_copied_in_the_file_viewer(cx: &mut TestAppContext) {
        let placement = Placement::Viewer { reveal: Rc::new(|_, _| {}) };
        assert_eq!(copy_the_comment(placement, cx).as_deref(), Some(TEXT));
    }
}
