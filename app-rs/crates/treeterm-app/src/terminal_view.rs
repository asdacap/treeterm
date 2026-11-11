//! The view of one terminal tab. Port of the view half of `BaseTerminal.tsx`: keyboard,
//! IME, mouse (selection or reporting), wheel, context menu, and the chrome around the
//! grid (scroll badge, ALT SCREEN, size mismatch, overlay, floating buttons).
//!
//! All terminal state lives in [`TerminalModel`]; dropping this view only stops drawing.

use std::cell::Cell;
use std::ops::Range;
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::component::menu::ContextMenuExt as _;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::*;
use gpui_kit::{
    App, Bounds, ClipboardItem, Context, Entity, FocusHandle, KeyBinding, KeyDownEvent,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, ScrollDelta,
    ScrollEvent, SharedString, Subscription, Task, UTF16Selection, Window, actions, div, px, rgba,
};
use treeterm_model::terminal::{CellPoint, Overlay, SelectionKind, TerminalEvent, TerminalModel};
use treeterm_term::alacritty_terminal::vte::ansi::Rgb;
use treeterm_term::fit::WidthLimit;
use treeterm_term::keys::{KeyInput, KeyOptions, Modifiers, encode_key};
use treeterm_term::mouse::{self, MouseAction, MouseInput};
use treeterm_term::palette::Palette;
use treeterm_term::view::ScrollPosition;

use crate::terminal_element::{Geometry, TermFont, TerminalElement};
use crate::theme::colors;

actions!(
    terminal,
    [
        Copy,
        Paste,
        SelectAll,
        ScrollToTop,
        ScrollToBottom,
        RefreshStream,
        ToggleWidthLimit
    ]
);

const CONTEXT: &str = "Terminal";
/// xterm.js's blink interval.
const BLINK_INTERVAL: Duration = Duration::from_millis(600);

pub fn bind_keys(cx: &mut App) {
    if cfg!(target_os = "macos") {
        cx.bind_keys([
            KeyBinding::new("cmd-c", Copy, Some(CONTEXT)),
            KeyBinding::new("cmd-v", Paste, Some(CONTEXT)),
            KeyBinding::new("cmd-a", SelectAll, Some(CONTEXT)),
        ]);
    } else {
        cx.bind_keys([
            KeyBinding::new("ctrl-shift-c", Copy, Some(CONTEXT)),
            KeyBinding::new("ctrl-shift-v", Paste, Some(CONTEXT)),
            KeyBinding::new("ctrl-shift-a", SelectAll, Some(CONTEXT)),
        ]);
    }
}

pub struct TerminalView {
    model: Entity<TerminalModel>,
    focus: FocusHandle,
    geometry: Rc<Cell<Option<Geometry>>>,
    /// IME composition in progress.
    marked_text: Option<String>,
    cursor_on: bool,
    _blink: Task<()>,
    /// Wheel motion below one line, carried to the next event.
    scroll_remainder: f32,
    /// The button a mouse-reporting program saw go down, so the release can be reported.
    reported_button: Option<mouse::MouseButton>,
    selecting: bool,
    /// Dragging the scrollbar thumb, grabbed this far below its top.
    scrollbar_grab: Option<Pixels>,
    _subscriptions: Vec<Subscription>,
}

impl TerminalView {
    pub fn new(model: Entity<TerminalModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let subscriptions = vec![
            cx.observe(&model, |_, _, cx| cx.notify()),
            cx.subscribe(&model, |_, _, event: &TerminalEvent, cx| match event {
                TerminalEvent::Clipboard(text) => {
                    cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                }
                TerminalEvent::CloseRequested { .. }
                | TerminalEvent::Bell
                | TerminalEvent::Activity(_)
                | TerminalEvent::Idle(_) => {}
            }),
            cx.on_focus(&focus, window, |view, window, cx| view.sync_focus(window, cx)),
            cx.on_blur(&focus, window, |view, window, cx| view.sync_focus(window, cx)),
            cx.observe_window_activation(window, |view, window, cx| view.sync_focus(window, cx)),
        ];
        let blink = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(BLINK_INTERVAL).await;
                if this
                    .update(cx, |view, cx| {
                        view.cursor_on = !view.cursor_on;
                        if view.model.read(cx).is_focused() {
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    return;
                }
            }
        });
        Self {
            model,
            focus,
            geometry: Rc::new(Cell::new(None)),
            marked_text: None,
            cursor_on: true,
            _blink: blink,
            scroll_remainder: 0.,
            reported_button: None,
            selecting: false,
            scrollbar_grab: None,
            _subscriptions: subscriptions,
        }
    }

    pub fn model(&self) -> &Entity<TerminalModel> {
        &self.model
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }

    /// Keyboard focus in the focused window, which is what lets this terminal write.
    fn sync_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let focused = self.focus.is_focused(window) && crate::window_counts_as_active(window);
        self.model.update(cx, |model, cx| model.set_focused(focused, cx));
        self.cursor_on = true;
    }

    fn reset_blink(&mut self) {
        self.cursor_on = true;
    }

    fn font(&self, cx: &App) -> TermFont {
        let settings = &self.model.read(cx).options().settings;
        let mut families = settings.font_families();
        if families.is_empty() {
            families.push(default_mono_family().to_owned());
        }
        let family = families.remove(0);
        families.push(default_mono_family().to_owned());
        TermFont {
            family: family.into(),
            fallbacks: families,
            size: px(settings.font_size),
        }
    }

    // ---- keyboard ----------------------------------------------------------------------

    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        // While composing, every key belongs to the input method.
        if self.marked_text.is_some() {
            return;
        }
        let keystroke = &event.keystroke;
        let m = keystroke.modifiers;
        let modifiers = Modifiers {
            control: m.control,
            alt: m.alt,
            shift: m.shift,
            platform: m.platform,
        };
        // Shift+PageUp/PageDown scroll the viewport, as in xterm.js.
        if m.shift && !m.control && !m.alt && !m.platform {
            match keystroke.key.as_str() {
                "pageup" | "pagedown" => {
                    let up = keystroke.key == "pageup";
                    self.model.update(cx, |model, cx| model.scroll_page(up, cx));
                    cx.stop_propagation();
                    return;
                }
                _ => {}
            }
        }
        let mode = *self.model.read(cx).term().lock().mode();
        let input = KeyInput {
            key: &keystroke.key,
            modifiers,
        };
        if let Some(bytes) = encode_key(input, mode, KeyOptions::for_host()) {
            self.reset_blink();
            self.model.update(cx, |model, cx| {
                model.clear_selection(cx);
                model.write_input(&bytes);
            });
            cx.stop_propagation();
        }
    }

    // ---- mouse -------------------------------------------------------------------------

    fn cell_at(&self, position: Point<Pixels>) -> Option<CellPoint> {
        let geometry = self.geometry.get()?;
        let (line, column, right_half) = geometry.cell_at(position);
        Some(CellPoint {
            line,
            column,
            right_half,
        })
    }

    fn report_mouse(&mut self, action: MouseAction, position: Point<Pixels>, m: gpui_kit::Modifiers, cx: &mut Context<Self>) -> bool {
        let mode = *self.model.read(cx).term().lock().mode();
        // Shift bypasses mouse reporting so text can still be selected, as in xterm.js.
        if !mouse::mouse_mode_active(mode) || m.shift {
            return false;
        }
        let Some(at) = self.cell_at(position) else {
            return false;
        };
        let input = MouseInput {
            action,
            column: at.column,
            line: at.line,
            modifiers: Modifiers {
                control: m.control,
                alt: m.alt,
                shift: m.shift,
                platform: m.platform,
            },
        };
        if let Some(bytes) = mouse::encode_mouse(input, mode) {
            self.model.update(cx, |model, _| model.write_input(&bytes));
        }
        true
    }

    fn on_mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        if event.button == MouseButton::Left
            && let Some(scrollbar) = self.geometry.get().and_then(|g| g.scrollbar)
            && scrollbar.track.contains(&event.position)
        {
            // Grab the thumb where it was hit; a click on the track centres it there.
            let grab = if scrollbar.thumb.contains(&event.position) {
                event.position.y - scrollbar.thumb.origin.y
            } else {
                scrollbar.thumb.size.height / 2.
            };
            self.scrollbar_grab = Some(grab);
            let ratio = scrollbar.ratio_at(event.position.y, grab);
            self.model.update(cx, |model, cx| model.scroll_to_ratio(ratio, cx));
            return;
        }
        let button = match event.button {
            MouseButton::Left => mouse::MouseButton::Left,
            MouseButton::Middle => mouse::MouseButton::Middle,
            MouseButton::Right | MouseButton::Navigate(_) => return,
        };
        if self.report_mouse(MouseAction::Press(button), event.position, event.modifiers, cx) {
            self.reported_button = Some(button);
            return;
        }
        if button != mouse::MouseButton::Left {
            return;
        }
        // Cmd/Ctrl+click opens a link under the pointer.
        if (event.modifiers.platform || (cfg!(not(target_os = "macos")) && event.modifiers.control))
            && let Some(url) = self.link_at(event.position, cx)
        {
            cx.open_url(&url);
            return;
        }
        let Some(at) = self.cell_at(event.position) else {
            return;
        };
        let kind = match event.click_count {
            0 | 1 => SelectionKind::Simple,
            2 => SelectionKind::Semantic,
            _ => SelectionKind::Lines,
        };
        self.selecting = true;
        self.model.update(cx, |model, cx| {
            if kind == SelectionKind::Simple {
                model.clear_selection(cx);
            }
            model.start_selection(at, kind, cx);
        });
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(grab) = self.scrollbar_grab {
            if let Some(scrollbar) = self.geometry.get().and_then(|g| g.scrollbar) {
                let ratio = scrollbar.ratio_at(event.position.y, grab);
                self.model.update(cx, |model, cx| model.scroll_to_ratio(ratio, cx));
            }
            return;
        }
        let held = event.pressed_button.and_then(|button| match button {
            MouseButton::Left => Some(mouse::MouseButton::Left),
            MouseButton::Middle => Some(mouse::MouseButton::Middle),
            MouseButton::Right => Some(mouse::MouseButton::Right),
            MouseButton::Navigate(_) => None,
        });
        if (self.reported_button.is_some() || held.is_none())
            && self.report_mouse(MouseAction::Move(held), event.position, event.modifiers, cx)
        {
            return;
        }
        if self.selecting && held == Some(mouse::MouseButton::Left)
            && let Some(at) = self.cell_at(event.position)
        {
            self.model.update(cx, |model, cx| model.extend_selection(at, cx));
        }
    }

    fn on_mouse_up(&mut self, event: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.scrollbar_grab.take().is_some() {
            return;
        }
        if let Some(button) = self.reported_button.take() {
            self.report_mouse(MouseAction::Release(button), event.position, event.modifiers, cx);
            return;
        }
        if self.selecting {
            self.selecting = false;
            // A plain click leaves no selection behind.
            if self.model.read(cx).selection_text().is_none() {
                self.model.update(cx, |model, cx| model.clear_selection(cx));
            }
        }
    }

    fn on_scroll(&mut self, event: &ScrollEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(geometry) = self.geometry.get() else {
            return;
        };
        let lines = match event.delta {
            ScrollDelta::Lines(delta) => delta.y,
            ScrollDelta::Pixels(delta) => f32::from(delta.y) / f32::from(geometry.cell.height),
        } + self.scroll_remainder;
        let whole = lines.trunc();
        self.scroll_remainder = lines - whole;
        let whole = whole as i32;
        if whole == 0 {
            return;
        }
        let mode = *self.model.read(cx).term().lock().mode();
        if mouse::mouse_mode_active(mode) && !event.modifiers.shift {
            let action = if whole > 0 { MouseAction::WheelUp } else { MouseAction::WheelDown };
            for _ in 0..whole.unsigned_abs().min(10) {
                if !self.report_mouse(action, event.position, event.modifiers, cx) {
                    break;
                }
            }
            return;
        }
        if let Some(bytes) = mouse::alternate_scroll(whole, mode) {
            self.model.update(cx, |model, _| model.write_input(&bytes));
            return;
        }
        self.model.update(cx, |model, cx| model.scroll_lines(whole, cx));
    }

    /// An OSC 8 hyperlink or a URL in the text under `position`.
    fn link_at(&self, position: Point<Pixels>, cx: &App) -> Option<String> {
        use treeterm_term::alacritty_terminal::index::{Column, Line, Point as GridPoint};
        let at = self.cell_at(position)?;
        let model = self.model.read(cx);
        let term = model.term().lock();
        let offset = term.grid().display_offset() as i32;
        let line = Line(at.line as i32 - offset);
        let cell = &term.grid()[GridPoint::new(line, Column(at.column))];
        if let Some(link) = cell.hyperlink() {
            return Some(link.uri().to_owned());
        }
        let text = treeterm_term::view::line_text(&*term, line);
        url_at(&text, at.column)
    }

    // ---- actions -----------------------------------------------------------------------

    fn copy(&mut self, _: &Copy, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = self.model.read(cx).selection_text() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    fn paste(&mut self, _: &Paste, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        if text.is_empty() {
            return;
        }
        self.model.update(cx, |model, cx| model.paste(&text, cx));
    }

    fn select_all(&mut self, _: &SelectAll, _window: &mut Window, cx: &mut Context<Self>) {
        self.model.update(cx, |model, cx| model.select_all(cx));
    }

    fn scroll_to_top(&mut self, _: &ScrollToTop, _window: &mut Window, cx: &mut Context<Self>) {
        self.model.update(cx, |model, cx| model.scroll_to_top(cx));
    }

    fn scroll_to_bottom(&mut self, _: &ScrollToBottom, _window: &mut Window, cx: &mut Context<Self>) {
        self.model.update(cx, |model, cx| model.scroll_to_bottom(cx));
    }

    fn refresh_stream(&mut self, _: &RefreshStream, _window: &mut Window, cx: &mut Context<Self>) {
        self.model.update(cx, |model, cx| model.refresh(cx));
    }

    fn toggle_width_limit(&mut self, _: &ToggleWidthLimit, _window: &mut Window, cx: &mut Context<Self>) {
        self.model.update(cx, |model, cx| {
            let limit = match model.width_limit() {
                WidthLimit::Capped(_) => WidthLimit::Off,
                WidthLimit::Off => WidthLimit::Capped(model.options().settings.max_cols),
            };
            model.set_width_limit(limit, cx);
        });
    }

    // ---- chrome ------------------------------------------------------------------------

    fn scroll_badge(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let model = self.model.read(cx);
        let pinned = model.is_pinned();
        let position = model.scroll_position();
        let c = colors(cx);
        let (label, title) = if pinned {
            ("PINNED", "Pinned to bottom (click to unpin)")
        } else if position == ScrollPosition::Bottom {
            (position.label(), "Scroll to top (double-click to pin)")
        } else {
            (position.label(), "Scroll to bottom (double-click to pin)")
        };
        let (border, text) = if pinned {
            (c.accent, c.accent)
        } else {
            (c.border, c.text_secondary)
        };
        badge("scroll-badge", label, rgba(0x000000b3).into(), border, text)
            .right(px(16.))
            .cursor_pointer()
            .tooltip_text(title)
            .on_click(cx.listener(move |view, event: &gpui_kit::ClickEvent, _, cx| {
                view.model.update(cx, |model, cx| {
                    if model.is_pinned() {
                        model.set_pinned(false, cx);
                    } else if event.click_count() >= 2 {
                        model.set_pinned(true, cx);
                    } else if model.scroll_position() == ScrollPosition::Bottom {
                        model.scroll_to_top(cx);
                    } else {
                        model.scroll_to_bottom(cx);
                    }
                });
            }))
    }
}

trait TooltipText {
    fn tooltip_text(self, text: &'static str) -> Self;
}

impl TooltipText for gpui_kit::Stateful<gpui_kit::Div> {
    fn tooltip_text(self, text: &'static str) -> Self {
        self.tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(text).build(window, cx))
    }
}

fn badge(
    id: &'static str,
    label: impl Into<SharedString>,
    background: gpui_kit::Hsla,
    border: gpui_kit::Hsla,
    text: gpui_kit::Hsla,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    div()
        .id(id)
        .absolute()
        .top(px(8.))
        .px(px(8.))
        .py(px(2.))
        .rounded(px(4.))
        .border_1()
        .border_color(border)
        .bg(background)
        .text_color(text)
        .text_size(px(11.))
        .child(label.into())
}

fn circle_button(id: &'static str, label: &'static str, cx: &App) -> gpui_kit::Stateful<gpui_kit::Div> {
    let c = colors(cx);
    let accent = c.accent;
    div()
        .id(id)
        .size(px(32.))
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .border_1()
        .border_color(c.border)
        .bg(c.bg_tertiary)
        .text_color(c.text_primary)
        .opacity(0.7)
        .cursor_pointer()
        .hover(move |style| style.opacity(1.).border_color(accent))
        .child(label)
}

fn default_mono_family() -> &'static str {
    if cfg!(target_os = "macos") { "Menlo" } else { "DejaVu Sans Mono" }
}

/// The URL around `column` in `line`, if any.
pub fn url_at(line: &str, column: usize) -> Option<String> {
    const SCHEMES: [&str; 3] = ["https://", "http://", "file://"];
    let chars: Vec<char> = line.chars().collect();
    if column >= chars.len() {
        return None;
    }
    let is_url_char = |c: char| !c.is_whitespace() && !matches!(c, '"' | '\'' | '<' | '>' | '`' | '|');
    let mut start = column;
    while start > 0 && is_url_char(chars[start - 1]) {
        start -= 1;
    }
    let mut end = column;
    while end < chars.len() && is_url_char(chars[end]) {
        end += 1;
    }
    let word: String = chars[start..end].iter().collect();
    let found = SCHEMES.iter().filter_map(|scheme| word.find(scheme)).min()?;
    let url = word[found..].trim_end_matches(['.', ',', ';', ':', ')', ']', '}', '!', '?']);
    (url.len() > "https://".len()).then(|| url.to_owned())
}

fn rgb_hsla(color: Rgb) -> gpui_kit::Hsla {
    gpui_kit::rgb((u32::from(color.r) << 16) | (u32::from(color.g) << 8) | u32::from(color.b)).into()
}

impl Render for TerminalView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let model = self.model.read(cx);
        let background = model.options().background;
        let palette = Palette::new(background);
        let overlay = model.overlay().clone();
        let alternate = model.is_alternate_screen();
        let mismatch = model.size_mismatch();
        let focused = self.focus.is_focused(window) && crate::window_counts_as_active(window);
        // Focus listeners only fire on focus *changes* seen between frames; a view focused
        // in the frame that created it can miss its own. Reconcile after rendering instead.
        if focused != model.is_focused() {
            cx.defer_in(window, |view, window, cx| view.sync_focus(window, cx));
        }
        let element = TerminalElement::new(
            self.model.clone(),
            cx.entity(),
            self.focus.clone(),
            self.font(cx),
            palette,
            self.geometry.clone(),
            self.cursor_on,
            focused,
            self.marked_text.clone(),
        );

        let mut root = div()
            .id("terminal")
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(rgb_hsla(background))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::scroll_to_top))
            .on_action(cx.listener(Self::scroll_to_bottom))
            .on_action(cx.listener(Self::refresh_stream))
            .on_action(cx.listener(Self::toggle_width_limit))
            .on_key_down(cx.listener(Self::on_key_down))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_down(MouseButton::Middle, cx.listener(Self::on_mouse_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up(MouseButton::Middle, cx.listener(Self::on_mouse_up))
            .on_scroll(cx.listener(Self::on_scroll))
            .cursor_text()
            .child(element);

        if let Some(message) = overlay.message() {
            let c = colors(cx);
            let (bg, fg): (gpui_kit::Hsla, gpui_kit::Hsla) = match overlay {
                Overlay::Loading => (rgba(0xffffff0d).into(), c.text_secondary),
                Overlay::Hidden
                | Overlay::Error(_)
                | Overlay::ExitedImmediately(_)
                | Overlay::Disconnected => (rgba(0xcd313126).into(), c.error),
            };
            root = root.child(
                div()
                    .id("terminal-overlay")
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .px(px(12.))
                    .py(px(6.))
                    .text_size(px(12.))
                    .text_center()
                    .bg(bg)
                    .text_color(fg)
                    .cursor_pointer()
                    .child(message)
                    .on_click(cx.listener(|view, _, _, cx| {
                        view.model.update(cx, |model, cx| model.dismiss_overlay(cx));
                    })),
            );
        }

        root = root.child(self.scroll_badge(cx));
        if alternate {
            root = root.child(
                badge(
                    "alt-screen",
                    "ALT SCREEN",
                    rgba(0xb47800b3).into(),
                    gpui_kit::rgb(0xb47800).into(),
                    gpui_kit::rgb(0xffd080).into(),
                )
                .right(px(100.))
                .tooltip_text("Terminal is in alternate screen mode (no scrollback)"),
            );
        }
        if let Some(mismatch) = mismatch {
            let title: SharedString = format!(
                "Requested: {}x{}, Stream: {}x{}",
                mismatch.requested.cols, mismatch.requested.rows, mismatch.actual.cols, mismatch.actual.rows
            )
            .into();
            root = root.child(
                badge(
                    "size-mismatch",
                    format!("{}x{}", mismatch.actual.cols, mismatch.actual.rows),
                    rgba(0xb42828b3).into(),
                    gpui_kit::rgb(0xb42828).into(),
                    gpui_kit::rgb(0xff9090).into(),
                )
                .right(px(200.))
                .tooltip(move |window, cx| {
                    gpui_kit::component::tooltip::Tooltip::new(title.clone()).build(window, cx)
                }),
            );
        }

        root = root.child(
            v_flex()
                .absolute()
                .bottom(px(16.))
                .right(px(16.))
                .gap(px(8.))
                .items_end()
                .child(
                    circle_button("refresh-stream", "↻", cx)
                        .tooltip_text("Refresh stream")
                        .on_click(cx.listener(|view, _, window, cx| {
                            view.refresh_stream(&RefreshStream, window, cx);
                        })),
                )
                .child(
                    circle_button("scroll-down", "↓", cx)
                        .tooltip_text("Scroll to bottom")
                        .on_click(cx.listener(|view, _, window, cx| {
                            view.scroll_to_bottom(&ScrollToBottom, window, cx);
                        })),
                ),
        );

        root.context_menu(|menu, _window, _cx| {
            menu.menu("Copy", Box::new(Copy)).menu("Paste", Box::new(Paste))
        })
    }
}

impl gpui_kit::EntityInputHandler for TerminalView {
    fn text_for_range(
        &mut self,
        _range: Range<usize>,
        _adjusted_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        None
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: 0..0,
            reversed: false,
        })
    }

    fn marked_text_range(&self, _window: &mut Window, _cx: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked_text
            .as_ref()
            .map(|text| 0..text.encode_utf16().count())
    }

    fn unmark_text(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.marked_text = None;
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        _range: Option<Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.marked_text = None;
        self.reset_blink();
        if !text.is_empty() {
            let bytes = text.as_bytes().to_vec();
            self.model.update(cx, |model, cx| {
                model.clear_selection(cx);
                model.write_input(&bytes);
            });
        }
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _range: Option<Range<usize>>,
        new_text: &str,
        _new_selected_range: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.marked_text = (!new_text.is_empty()).then(|| new_text.to_owned());
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        _range_utf16: Range<usize>,
        _element_bounds: Bounds<Pixels>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        // The IME candidate window goes under the cursor.
        let geometry = self.geometry.get()?;
        let model = self.model.read(cx);
        let term = model.term().lock();
        let cursor = term.grid().cursor.point;
        let row = (cursor.line.0 + term.grid().display_offset() as i32).max(0) as usize;
        Some(geometry.cell_bounds(row, cursor.column.0))
    }

    fn character_index_for_point(
        &mut self,
        _point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::url_at;

    #[test]
    fn finds_the_url_under_a_column() {
        let line = "see https://example.com/a?b=1. for details";
        assert_eq!(url_at(line, 10).as_deref(), Some("https://example.com/a?b=1"));
        assert_eq!(url_at(line, 1), None);
        assert_eq!(url_at("(http://x.io/y)", 3).as_deref(), Some("http://x.io/y"));
    }
}
