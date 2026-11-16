//! The view of one terminal tab. Port of the view half of `BaseTerminal.tsx`: keyboard,
//! IME, mouse (selection or reporting), wheel, context menu, and the chrome around the
//! grid (scroll badge, ALT SCREEN, size mismatch, DEBUG badge, overlay, floating buttons).
//!
//! All terminal state lives in [`TerminalModel`]; dropping this view only stops drawing.

use std::cell::Cell;
use std::ops::Range;
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::component::input;
use gpui_kit::component::menu::ContextMenuExt as _;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::*;
use gpui_kit::{
    App, Bounds, ClipboardItem, Context, Entity, FocusHandle, KeyBinding, KeyDownEvent,
    ModifiersChangedEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, NoAction,
    Pixels, Point, ScrollDelta, ScrollEvent, SharedString, Subscription, Task, UTF16Selection,
    Window, actions, div, px, rgba,
};
use treeterm_model::terminal::{
    CellPoint, LinkPress, Overlay, Pointer, TerminalEvent, TerminalModel,
};
use treeterm_model::zoom::ZoomStore;
use treeterm_term::alacritty_terminal::vte::ansi::Rgb;
use treeterm_term::fit::WidthLimit;
use treeterm_term::keys::{KeyInput, KeyOptions, Modifiers, encode_key};
use treeterm_term::mouse::{self, MouseAction, MouseInput, WheelTarget};
use treeterm_term::palette::Palette;
use treeterm_term::view::ScrollPosition;

use crate::terminal_element::{Geometry, TermFont, TerminalElement};
use crate::theme::colors;

actions!(
    terminal,
    [
        ScrollToTop,
        ScrollToBottom,
        RefreshStream,
        ToggleWidthLimit
    ]
);

pub(crate) const CONTEXT: &str = "Terminal";
/// xterm.js's blink interval.
const BLINK_INTERVAL: Duration = Duration::from_millis(600);

pub fn bind_keys(cx: &mut App) {
    // GPUI Kit's Root binds Tab/Shift-Tab to focus navigation, and bindings run before
    // `on_key_down`: unbind them here so the keys reach the PTY (`\t`, `\x1b[Z`).
    cx.bind_keys([
        KeyBinding::new("tab", NoAction, Some(CONTEXT)),
        KeyBinding::new("shift-tab", NoAction, Some(CONTEXT)),
    ]);
    cx.bind_keys(edit_keys(cfg!(target_os = "macos")));
}

/// A focused terminal's Copy, Paste and Select All keys. They run GPUI Kit's edit actions,
/// which Edit's menu items send to whatever has focus. Off macOS they take Shift: Ctrl+C,
/// Ctrl+V and Ctrl+A go to the PTY.
pub(crate) fn edit_keys(mac: bool) -> [KeyBinding; 3] {
    let [copy, paste, select_all] =
        if mac { ["cmd-c", "cmd-v", "cmd-a"] } else { ["ctrl-shift-c", "ctrl-shift-v", "ctrl-shift-a"] };
    [
        KeyBinding::new(copy, input::Copy, Some(CONTEXT)),
        KeyBinding::new(paste, input::Paste, Some(CONTEXT)),
        KeyBinding::new(select_all, input::SelectAll, Some(CONTEXT)),
    ]
}

/// Give a focused terminal back the app shortcuts it encodes, from `accelerators` and GPUI
/// Kit Root's Ctrl+C (copy): Ctrl+Q (XON), Ctrl+W, Ctrl+M (CR), F11 and Ctrl+C (SIGINT)
/// reach the PTY, as xterm.js consumed them before Electron's menu saw them.
///
/// Bind this after the shortcuts themselves. GPUI ranks a binding with no context as deep
/// as the focused terminal's own context, and of two bindings at one depth the one bound
/// later wins.
pub fn keep_terminal_keys(accelerators: &[&'static str], options: KeyOptions, cx: &mut App) {
    let accelerators = accelerators.iter().copied().chain(["ctrl-c"]);
    cx.bind_keys(
        terminal_shadowed(accelerators, options)
            .into_iter()
            .map(|keys| KeyBinding::new(keys, NoAction, Some(CONTEXT))),
    );
}

/// The app shortcuts a focused terminal keeps for its PTY: those it encodes. Ctrl+Shift+
/// letter is not among them, as xterm.js leaves it to the app (Exit and Kill Daemon's
/// Ctrl+Shift+Q works from a terminal).
fn terminal_shadowed(accelerators: impl IntoIterator<Item = &'static str>, options: KeyOptions) -> Vec<&'static str> {
    accelerators
        .into_iter()
        .filter(|keys| {
            let keystroke = match gpui_kit::Keystroke::parse(keys) {
                Ok(keystroke) => keystroke,
                Err(error) => panic!("invalid shortcut {keys:?}: {error}"),
            };
            let m = keystroke.modifiers;
            let ctrl_shift_letter = m.control && m.shift && keystroke.key.chars().all(|c| c.is_ascii_alphabetic());
            let input = KeyInput {
                key: &keystroke.key,
                modifiers: term_modifiers(m),
            };
            !ctrl_shift_letter && encode_key(input, Default::default(), options).is_some()
        })
        .collect()
}

/// Whether a terminal coming into view takes keyboard focus: only the workspace's active tab
/// does (`BaseTerminal` focuses on mount when `activeTabId` is its tab), not another tabset's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    Take,
    Leave,
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
    /// Dragging the scrollbar thumb, grabbed this far below its top.
    scrollbar_grab: Option<Pixels>,
    _subscriptions: Vec<Subscription>,
}

impl TerminalView {
    pub fn new(model: Entity<TerminalModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let subscriptions = vec![
            cx.observe(&model, |_, _, cx| cx.notify()),
            // View > Zoom: the text is drawn at the new size, and the grid refits to it.
            cx.observe_global::<ZoomStore>(|_, cx| cx.notify()),
            cx.subscribe(&model, |_, _, event: &TerminalEvent, cx| match event {
                TerminalEvent::Clipboard(text) => {
                    cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                }
                TerminalEvent::CloseRequested { .. }
                | TerminalEvent::Bell
                | TerminalEvent::Activity(_)
                | TerminalEvent::Idle(_)
                | TerminalEvent::Input(_)
                | TerminalEvent::Exited(_) => {}
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
            scrollbar_grab: None,
            _subscriptions: subscriptions,
        }
    }

    /// For the automation socket's state snapshot.
    #[cfg(feature = "automation")]
    pub fn model(&self) -> &Entity<TerminalModel> {
        &self.model
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }

    /// This terminal came into view in its tabset, as an Electron terminal mounts: it fits its
    /// PTY to itself again, taking it back from a size another attachment set meanwhile, and,
    /// when it is the workspace's active tab (`Focus::Take`), takes keyboard focus. Not from
    /// an open dialog, which gives focus back when it closes.
    pub fn on_shown(&mut self, focus: Focus, window: &mut Window, cx: &mut Context<Self>) {
        use gpui_kit::component::WindowExt as _;
        self.model.update(cx, |model, cx| model.refit(cx));
        if focus == Focus::Take && !window.has_active_dialog(cx) {
            window.focus(&self.focus, cx);
        }
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
            size: px(settings.font_size * cx.global::<ZoomStore>().zoom().factor()),
        }
    }

    // ---- keyboard ----------------------------------------------------------------------

    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        self.model.read(cx).key_pressed();
        // While composing, every key belongs to the input method.
        if self.marked_text.is_some() {
            return;
        }
        let keystroke = &event.keystroke;
        let m = keystroke.modifiers;
        let modifiers = term_modifiers(m);
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
                model.write_input(&bytes, cx);
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

    /// The cell under `position` only when it is over the grid: a link is under the pointer
    /// there alone, as xterm.js's linkifier listens on its screen and not on the padding.
    fn grid_cell_at(&self, position: Point<Pixels>) -> Option<CellPoint> {
        let geometry = self.geometry.get()?;
        if !geometry.contains(position) {
            return None;
        }
        self.cell_at(position)
    }

    fn pointer_at(&self, position: Point<Pixels>, modifiers: gpui_kit::Modifiers, cx: &App) -> Option<Pointer> {
        Some(Pointer {
            at: self.grid_cell_at(position)?,
            secondary: modifiers.secondary(cx),
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
            modifiers: term_modifiers(m),
        };
        if let Some(bytes) = mouse::encode_mouse(input, mode) {
            self.model.update(cx, |model, cx| model.write_input(&bytes, cx));
        }
        true
    }

    /// A press of any button, before it is handled (Electron's capture-phase pointerdown):
    /// a click into a terminal that kept focus claims the PTY's input back, and a right-click
    /// claims it before the context menu's Paste.
    fn claim_on_press(&mut self, _: &MouseDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        self.model.read(cx).claim_input();
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
        if button == mouse::MouseButton::Left {
            let pointer = self.pointer_at(event.position, event.modifiers, cx);
            match self.model.update(cx, |model, _| model.press_link(pointer)) {
                LinkPress::Taken => return,
                LinkPress::Shared => {}
            }
        }
        if self.report_mouse(MouseAction::Press(button), event.position, event.modifiers, cx) {
            self.reported_button = Some(button);
            return;
        }
        if button != mouse::MouseButton::Left {
            return;
        }
        let Some(at) = self.cell_at(event.position) else {
            return;
        };
        let modifiers = term_modifiers(event.modifiers);
        self.model
            .update(cx, |model, cx| model.press_selection(at, event.click_count, modifiers, cx));
    }

    /// A right press, before the context menu opens: on macOS it selects the word under the
    /// pointer, as xterm.js's `rightClickSelectsWord` does there, so the menu's Copy copies it.
    fn on_right_mouse_down(&mut self, event: &MouseDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if !cfg!(target_os = "macos") {
            return;
        }
        let Some(at) = self.cell_at(event.position) else {
            return;
        };
        self.model.update(cx, |model, cx| model.select_word_unless_selected(at, cx));
    }

    /// The pointer moved anywhere in the window (the terminal element listens on the whole
    /// window, as xterm.js listens on the document while selecting). A selection follows it
    /// past the grid, which scrolls when it is above or below.
    pub(crate) fn on_window_mouse_move(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        if !self.model.read(cx).is_selecting() {
            return;
        }
        if event.pressed_button != Some(MouseButton::Left) {
            // The release went where this terminal did not hear it (it was not drawn then).
            self.release_selection(event.position, event.modifiers, cx);
            return;
        }
        let Some(geometry) = self.geometry.get() else {
            return;
        };
        let Some(at) = self.cell_at(event.position) else {
            return;
        };
        let overshoot = geometry.overshoot(event.position.y);
        self.model.update(cx, |model, cx| model.drag_selection(at, overshoot, cx));
    }

    /// The left button came up anywhere in the window: a selection ends there.
    pub(crate) fn on_window_mouse_up(&mut self, event: &MouseUpEvent, cx: &mut Context<Self>) {
        if event.button == MouseButton::Left && self.model.read(cx).is_selecting() {
            self.release_selection(event.position, event.modifiers, cx);
        }
    }

    fn release_selection(&mut self, position: Point<Pixels>, modifiers: gpui_kit::Modifiers, cx: &mut Context<Self>) {
        let Some(at) = self.cell_at(position) else {
            return;
        };
        let modifiers = term_modifiers(modifiers);
        self.model.update(cx, |model, cx| model.release_selection(at, modifiers, cx));
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let pointer = self.pointer_at(event.position, event.modifiers, cx);
        self.model.update(cx, |model, cx| model.hover(pointer, cx));
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
        // A selection drag is the window's listener's (`on_window_mouse_move`).
        if self.reported_button.is_some() || held.is_none() {
            self.report_mouse(MouseAction::Move(held), event.position, event.modifiers, cx);
        }
    }

    fn on_mouse_up(&mut self, event: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.scrollbar_grab.take().is_some() {
            return;
        }
        if event.button == MouseButton::Left {
            let at = self.grid_cell_at(event.position);
            self.model.update(cx, |model, cx| model.release_link(at, cx));
        }
        // A selection ends in the window's listener (`on_window_mouse_up`).
        if let Some(button) = self.reported_button.take() {
            self.report_mouse(MouseAction::Release(button), event.position, event.modifiers, cx);
        }
    }

    /// A left release off the terminal opens no link.
    fn on_mouse_up_out(&mut self, _: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        self.model.update(cx, |model, cx| model.release_link(None, cx));
    }

    fn on_hover(&mut self, hovered: &bool, _window: &mut Window, cx: &mut Context<Self>) {
        if !hovered {
            self.model.update(cx, |model, cx| model.hover(None, cx));
        }
    }

    /// Cmd/Ctrl going down over a URL in the text makes it a link, and going up, text again.
    fn on_modifiers_changed(&mut self, event: &ModifiersChangedEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let secondary = event.modifiers.secondary(cx);
        self.model.update(cx, |model, cx| model.hover_secondary(secondary, cx));
    }

    fn on_scroll(&mut self, event: &ScrollEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(geometry) = self.geometry.get() else {
            return;
        };
        let delta = match event.delta {
            ScrollDelta::Lines(delta) => delta.y,
            ScrollDelta::Pixels(delta) => f32::from(delta.y) / f32::from(geometry.cell.height),
        };
        let mode = *self.model.read(cx).term().lock().mode();
        let target = WheelTarget::of(mode, event.modifiers.shift);
        let delta = target.lines(delta, event.modifiers.alt);
        // Positive is toward older output.
        if delta > 0. {
            self.model.update(cx, |model, cx| model.wheel_up(cx));
        }
        let lines = delta + self.scroll_remainder;
        let whole = lines.trunc();
        self.scroll_remainder = lines - whole;
        let whole = whole as i32;
        if whole == 0 {
            return;
        }
        match target {
            WheelTarget::Report => {
                let action = if whole > 0 { MouseAction::WheelUp } else { MouseAction::WheelDown };
                for _ in 0..whole.unsigned_abs().min(10) {
                    if !self.report_mouse(action, event.position, event.modifiers, cx) {
                        break;
                    }
                }
            }
            WheelTarget::Arrows => {
                if let Some(bytes) = mouse::alternate_scroll(whole, mode) {
                    self.model.update(cx, |model, cx| model.write_input(&bytes, cx));
                }
            }
            WheelTarget::Scrollback => self.model.update(cx, |model, cx| model.scroll_lines(whole, cx)),
        }
    }

    // ---- actions -----------------------------------------------------------------------

    fn copy(&mut self, _: &input::Copy, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = self.model.read(cx).selection_text() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    fn paste(&mut self, _: &input::Paste, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        if text.is_empty() {
            return;
        }
        self.model.update(cx, |model, cx| model.paste(&text, cx));
    }

    fn select_all(&mut self, _: &input::SelectAll, _window: &mut Window, cx: &mut Context<Self>) {
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
        let (accent, primary) = (c.accent, c.text_primary);
        badge("scroll-badge", label, rgba(0x000000b3).into(), border, text)
            .right(px(16.))
            .opacity(if pinned { 0.8 } else { 0.6 })
            .hover(move |d| d.opacity(1.).border_color(accent).text_color(primary))
            .cursor_pointer()
            .tooltip_text(title)
            // Each click of a double-click arrives here; the model tells them apart by time.
            .on_click(cx.listener(|view, _, _, cx| {
                view.model.update(cx, |model, cx| model.click_scroll_badge(cx));
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
        .font_family(default_mono_family())
        .opacity(0.6)
        .child(label.into())
}

/// `.terminal-circle-btn` in `.terminal-floating-buttons`: 70% opaque until hovered.
fn circle_button(id: &'static str, label: &'static str, size: f32, cx: &App) -> gpui_kit::Stateful<gpui_kit::Div> {
    let c = colors(cx);
    let (accent, hover) = (c.accent, c.bg_hover);
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
        .text_size(px(size))
        .hover(move |style| style.opacity(1.).bg(hover).border_color(accent))
        .child(label)
}

fn term_modifiers(m: gpui_kit::Modifiers) -> Modifiers {
    Modifiers {
        control: m.control,
        alt: m.alt,
        shift: m.shift,
        platform: m.platform,
    }
}

fn default_mono_family() -> &'static str {
    if cfg!(target_os = "macos") { "Menlo" } else { "DejaVu Sans Mono" }
}

/// `.terminal-overlay`'s `'Monaco', 'Menlo', 'Ubuntu Mono', monospace`: Monaco ships with
/// macOS; elsewhere the monospace default.
fn overlay_font_family() -> &'static str {
    if cfg!(target_os = "macos") { "Monaco" } else { default_mono_family() }
}

fn rgb_hsla(color: Rgb) -> gpui_kit::Hsla {
    gpui_kit::rgb((u32::from(color.r) << 16) | (u32::from(color.g) << 8) | u32::from(color.b)).into()
}

impl Render for TerminalView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let model = self.model.read(cx);
        let background = model.options().background;
        let palette = Palette::new(background);
        let overlay = model.banner();
        let alternate = model.is_alternate_screen();
        let mismatch = model.size_mismatch();
        let debug_badge = model.show_debug_badge();
        let hovered_link = model.hovered_link();
        // xterm.js's `xterm-cursor-pointer` over a hovered link.
        let pointer_cursor = hovered_link.is_some();
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
            hovered_link,
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
            .capture_any_mouse_down(cx.listener(Self::claim_on_press))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_down(MouseButton::Middle, cx.listener(Self::on_mouse_down))
            // Before the context menu's listener, which wraps this element: the menu's Copy
            // then copies what the press selected.
            .on_mouse_down(MouseButton::Right, cx.listener(Self::on_right_mouse_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up(MouseButton::Middle, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up_out))
            .on_hover(cx.listener(Self::on_hover))
            .on_modifiers_changed(cx.listener(Self::on_modifiers_changed))
            .on_scroll(cx.listener(Self::on_scroll))
            .child(element);
        root = if pointer_cursor { root.cursor_pointer() } else { root.cursor_text() };

        if let Some(message) = overlay.message() {
            // `.terminal-overlay-info` and `.terminal-overlay-error`.
            let (bg, fg): (gpui_kit::Hsla, gpui_kit::Hsla) = match overlay {
                Overlay::Loading => (rgba(0xffffff0d).into(), gpui_kit::rgb(0x888888).into()),
                Overlay::Hidden
                | Overlay::Error(_)
                | Overlay::AttachFailed(_)
                | Overlay::ExitedImmediately(_)
                | Overlay::Disconnected => (rgba(0xcd313126).into(), gpui_kit::rgb(0xf48771).into()),
            };
            // Every banner shows `.terminal-overlay`'s pointer; the model keeps "Loading
            // terminal..." on a click, as Electron gives it no click handler.
            root = root.child(
                div()
                    .id("terminal-overlay")
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .px(px(12.))
                    .py(px(6.))
                    .font_family(overlay_font_family())
                    .text_size(px(12.))
                    // Chromium's `line-height: normal` for 12px Monaco.
                    .line_height(px(16.))
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

        if debug_badge {
            // `.debug-badge`: after the overlay, which it draws over (both are at z-index 10).
            root = root.child(
                badge(
                    "debug-badge",
                    "DEBUG",
                    rgba(0x8c28b4b3).into(),
                    gpui_kit::rgb(0x8c28b4).into(),
                    gpui_kit::rgb(0xe0a0ff).into(),
                )
                .right(px(300.))
                .opacity(0.8),
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
                    circle_button("refresh-stream", "↻", 13.33, cx)
                        .tooltip_text("Refresh stream")
                        .on_click(cx.listener(|view, _, window, cx| {
                            view.refresh_stream(&RefreshStream, window, cx);
                        })),
                )
                .child(
                    circle_button("scroll-down", "↓", 16., cx)
                        .tooltip_text("Scroll to bottom")
                        .on_click(cx.listener(|view, _, window, cx| {
                            view.scroll_to_bottom(&ScrollToBottom, window, cx);
                        })),
                ),
        );

        // `BaseTerminal`'s menu: Copy and Paste, without shortcut hints.
        let view = cx.entity().downgrade();
        root.context_menu(move |menu, _window, _cx| {
            let (copy, paste) = (view.clone(), view.clone());
            menu.item(crate::ui::menu_item("Copy").on_click(move |_, window, cx| {
                match copy.update(cx, |view, cx| view.copy(&input::Copy, window, cx)) {
                    Ok(()) | Err(_) => {}
                }
            }))
            .item(crate::ui::menu_item("Paste").on_click(move |_, window, cx| {
                match paste.update(cx, |view, cx| view.paste(&input::Paste, window, cx)) {
                    Ok(()) | Err(_) => {}
                }
            }))
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
            // Text from the input method: its key presses may not have reached `on_key_down`.
            self.model.read(cx).key_pressed();
            let bytes = text.as_bytes().to_vec();
            self.model.update(cx, |model, cx| {
                model.clear_selection(cx);
                model.write_input(&bytes, cx);
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
    #[test]
    fn a_terminal_keeps_the_shortcuts_it_encodes_off_macos() {
        let linux = treeterm_term::keys::KeyOptions {
            alt_sends_escape: true,
            mac_word_motion: false,
        };
        let accelerators = crate::menus::ACCELERATORS.iter().copied().chain(["ctrl-c", "cmd-w", "ctrl-cmd-f"]);
        assert_eq!(
            super::terminal_shadowed(accelerators, linux),
            ["ctrl-q", "ctrl-m", "ctrl-w", "f11", "ctrl-c"],
            "Ctrl+, and Ctrl+Shift+Q stay app shortcuts; Cmd never reaches the PTY"
        );
    }
}
