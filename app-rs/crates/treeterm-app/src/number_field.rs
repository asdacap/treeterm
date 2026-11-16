//! `<input type="number" class="settings-input" min max step>` as Chromium draws it: a text
//! field that takes numbers, with spin buttons (shown while it is hovered or focused),
//! ArrowUp/ArrowDown and, while it is focused, the wheel over it, which step its value as
//! Chromium's do. Typed values are never clamped: `min` and `max` only bound the stepping.

use gpui_kit::assets::IconName;
use gpui_kit::base::{Button, NumberInput, StepAction};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::prelude::*;
use gpui_kit::{
    App, DispatchPhase, Entity, Focusable as _, HitboxBehavior, ScrollDelta, ScrollEvent,
    ScrollSource, SharedString, TouchPhase, Window, canvas, div, px,
};

use crate::theme::colors;
use crate::ui;

/// `input[type=number].settings-input`: 100px wide.
pub const WIDTH: f32 = 100.;

/// The field's height, as Chromium lays out `.settings-input`.
const HEIGHT: f32 = 33.5;

/// The field's `min`, `max` and `step` attributes. The step base is `min`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NumberSpec {
    pub min: f64,
    /// HTML's default when there is no `max` is the largest finite number: `f64::MAX`.
    pub max: f64,
    pub step: f64,
}

/// What `<input type="number">` reports as its value for `text`: the number, if `text` is
/// a valid floating-point number (HTML: `-?(\d+|\d*\.\d+)([eE][+-]?\d+)?`, finite);
/// otherwise its value is the empty string.
pub fn number_value(text: &str) -> Option<f64> {
    let unsigned = text.strip_prefix('-').unwrap_or(text);
    let (mantissa, exponent) = match unsigned.find(['e', 'E']) {
        Some(at) => (&unsigned[..at], Some(&unsigned[at + 1..])),
        None => (unsigned, None),
    };
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let mantissa_ok = match mantissa.split_once('.') {
        Some((whole, fraction)) => (whole.is_empty() || digits(whole)) && digits(fraction),
        None => digits(mantissa),
    };
    let exponent_ok = match exponent {
        None => true,
        Some(exponent) => digits(exponent.strip_prefix(['+', '-']).unwrap_or(exponent)),
    };
    if !(mantissa_ok && exponent_ok) {
        return None;
    }
    match text.parse::<f64>() {
        Ok(value) if value.is_finite() => Some(value),
        // Too large (`1e400`): Chromium's value is empty too.
        Ok(_) | Err(_) => None,
    }
}

/// Chromium's spin button and ArrowUp/ArrowDown (`InputType::StepUpFromLayoutObject`): the
/// field's new text, or `None` when the step does nothing.
///
/// An empty or invalid value counts as 0, first moved so that one step lands in range. A
/// value below `min` steps up to `min`, one above `max` steps down to `max`; stepping
/// further out does nothing. A value off the step grid moves to the next grid value in the
/// step's direction; the result is clamped to the grid values in range, and a step that
/// would not move in its direction does nothing.
pub fn spin(text: &str, action: StepAction, spec: NumberSpec) -> Option<String> {
    let NumberSpec { min, max, step } = spec;
    let up = match action {
        StepAction::Increment => true,
        StepAction::Decrement => false,
    };
    let delta = if up { step } else { -step };
    let (current, replaced) = match number_value(text) {
        Some(value) => (value, false),
        None => ((0f64).max(min - delta).min(max - delta), true),
    };
    // Chromium writes the substitute value before stepping from it.
    let unchanged = || replaced.then(|| format_number(current));
    if up && current < min {
        return Some(format_number(min));
    }
    if !up && current > max {
        return Some(format_number(max));
    }
    if (up && current >= max) || (!up && current <= min) || min > max {
        return unchanged();
    }
    let grid = |steps: f64| min + steps * step;
    let offset = (current - min) / step;
    let mut next = if offset.fract() == 0. {
        current + delta
    } else if up {
        grid(offset.ceil())
    } else {
        grid(offset.floor())
    };
    if next > max {
        next = grid(((max - min) / step).floor());
    } else if next < min {
        next = min;
    }
    if (up && next <= current) || (!up && next >= current) {
        return unchanged();
    }
    Some(format_number(next))
}

/// A number as Chromium writes it into the field: `1600`, `13.5`.
fn format_number(value: f64) -> String {
    format!("{value}")
}

/// Where the latest trackpad scroll sequence began, as far as one field is concerned.
/// Chromium (Blink's `MouseWheelEventManager`) picks the wheel target when a sequence begins
/// and sends it every later event of the sequence, momentum included. GPUI sends each event
/// to whatever is under the pointer then, so a field keeps this record itself.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum WheelLatch {
    /// It began elsewhere, or over the field while the field was not focused, or no
    /// sequence has begun since the field was first drawn.
    #[default]
    Elsewhere,
    /// It began over the field while the field was focused.
    Field,
}

/// The step a wheel event over the focused field takes (Chromium's
/// `SpinButtonElement::ForwardEvent` with `DoStepAction(wheelDeltaY)`): every event that
/// scrolls up steps up once and every one that scrolls down steps down once, however far it
/// scrolls. A trackpad's events (`ScrollDelta::Pixels`, which come in sequences) step only
/// when their sequence began over the focused field (`latch`); a mouse wheel's
/// (`ScrollDelta::Lines`, which come one by one) step whenever they are over it. `None` for an
/// event that moves on neither (a gesture's start or end, a sideways scroll), for a
/// trackpad sequence that began elsewhere and for a touchscreen pan, which is no wheel
/// event: these scroll the page.
fn wheel_step(event: &ScrollEvent, latch: WheelLatch) -> Option<StepAction> {
    let y = match (event.source, event.delta, latch) {
        (ScrollSource::Touch, _, _) => return None,
        (ScrollSource::Pointer, ScrollDelta::Pixels(_), WheelLatch::Elsewhere) => return None,
        (ScrollSource::Pointer, ScrollDelta::Pixels(delta), WheelLatch::Field) => delta.y.as_f32(),
        (ScrollSource::Pointer, ScrollDelta::Lines(delta), _) => delta.y,
    };
    if y > 0. {
        Some(StepAction::Increment)
    } else if y < 0. {
        Some(StepAction::Decrement)
    } else {
        None
    }
}

/// Step `state` (focusing it, as a press on Chromium's spin button does).
fn step(
    state: &Entity<InputState>,
    action: StepAction,
    spec: NumberSpec,
    window: &mut Window,
    cx: &mut App,
) {
    state.update(cx, |input, cx| {
        input.focus(window, cx);
        if let Some(text) = spin(&input.value(), action, spec) {
            input.set_value(text, window, cx);
        }
    });
}

/// Records in `latch` where each trackpad scroll sequence begins: over the field (this
/// element's bounds, which it is placed to cover) while `state` is focused, or elsewhere. It
/// listens to the whole window, so it also sees sequences that begin away from the field,
/// and in the capture phase, so the record is made before the field's own wheel listener
/// sees a sequence's first event.
fn wheel_latch(state: Entity<InputState>, latch: Entity<WheelLatch>) -> impl IntoElement {
    canvas(
        |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
        move |_, hitbox, window, _| {
            window.on_mouse_event(move |event: &ScrollEvent, phase, window, cx| {
                if phase != DispatchPhase::Capture || event.touch_phase != TouchPhase::Started {
                    return;
                }
                let focused = state.read(cx).focus_handle(cx).is_focused(window);
                let began = if focused && hitbox.should_handle_scroll(window) {
                    WheelLatch::Field
                } else {
                    WheelLatch::Elsewhere
                };
                latch.update(cx, |latch, _| *latch = began);
            });
        },
    )
    // The field's border box: an absolute child is placed inside its parent's border.
    .absolute()
    .top(px(-1.))
    .left(px(-1.))
    .w(px(WIDTH))
    .h(px(HEIGHT))
}

/// The field over `state`, stepping by `spec`.
#[derive(IntoElement)]
pub struct NumberField {
    state: Entity<InputState>,
    spec: NumberSpec,
}

impl NumberField {
    pub fn new(state: &Entity<InputState>, spec: NumberSpec) -> Self {
        Self {
            state: state.clone(),
            spec,
        }
    }
}

impl RenderOnce for NumberField {
    /// `.settings-input`: bg-tertiary, a 1px border that turns accent on focus, 4px radius,
    /// 8px 12px padding, 13px text; the spin buttons sit inside the padding on the right.
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let latch = window.use_keyed_state(
            ("number-field-wheel", self.state.entity_id()),
            cx,
            |_, _| WheelLatch::default(),
        );
        let c = colors(cx);
        let focused = self.state.read(cx).focus_handle(cx).is_focused(window);
        let field: SharedString = format!("number-field-{}", self.state.entity_id()).into();
        let group = field.clone();
        let (state, spec) = (self.state.clone(), self.spec);
        let arrows = c.text_secondary;
        let hover = c.bg_hover;
        let spin_button = move |button: Button, icon: IconName| {
            button
                .w(px(15.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(2.))
                .hover(move |s| s.bg(hover))
                .when(!focused, |b| {
                    b.opacity(0.).group_hover(group.clone(), |s| s.opacity(1.))
                })
                .child(ui::icon(icon, 8., arrows))
        };
        let spin_up = spin_button.clone();
        let wheel = self.state.clone();
        let wheel_latched = latch.clone();
        div()
            .group(field)
            // Chromium's spin button takes the wheel only while the field is focused, and a
            // trackpad's only for a sequence that began over the field; the page around it
            // does not scroll then.
            .on_scroll(move |event, window, cx| {
                if !wheel.read(cx).focus_handle(cx).is_focused(window) {
                    return;
                }
                if let Some(action) = wheel_step(event, *wheel_latched.read(cx)) {
                    cx.stop_propagation();
                    step(&wheel, action, spec, window, cx);
                }
            })
            .w(px(WIDTH))
            .h(px(HEIGHT))
            .px(px(12.))
            .py(px(4.))
            .rounded(px(4.))
            .border_1()
            .border_color(if focused { c.accent } else { c.border })
            .bg(c.bg_tertiary)
            .child(wheel_latch(self.state.clone(), latch))
            .child(
                NumberInput::new(&self.state)
                    .controls_right()
                    .size_full()
                    .on_step(move |action, window, cx| step(&state, action, spec, window, cx))
                    .input(
                        Input::new(&self.state)
                            .appearance(false)
                            .h_full()
                            .px(px(0.))
                            .py(px(0.))
                            .text_size(px(ui::BODY)),
                    )
                    .increment_button(move |button| spin_up(button, IconName::ChevronUp))
                    .decrement_button(move |button| spin_button(button, IconName::ChevronDown)),
            )
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use gpui_kit::{
        Context, Modifiers, MouseMoveEvent, Pixels, Point, Render, TestAppContext,
        VisualTestContext, point,
    };

    use super::*;

    /// The prefix Timeout field: `min=500 max=5000 step=100`.
    const TIMEOUT: NumberSpec = NumberSpec {
        min: 500.,
        max: 5000.,
        step: 100.,
    };

    fn up(text: &str) -> Option<String> {
        spin(text, StepAction::Increment, TIMEOUT)
    }

    fn down(text: &str) -> Option<String> {
        spin(text, StepAction::Decrement, TIMEOUT)
    }

    #[test]
    fn valid_numbers_follow_html() {
        for (text, value) in [
            ("1500", 1500.),
            ("-5", -5.),
            ("1500.7", 1500.7),
            (".5", 0.5),
            ("1e3", 1000.),
            ("1E+2", 100.),
            ("2e-1", 0.2),
        ] {
            assert_eq!(number_value(text), Some(value), "{text}");
        }
        for text in [
            "", "-", "+5", "5.", "1e", "1e+", " 5", "5 ", "abc", "0x10", "1,000", "1e400",
            "Infinity", "--5",
        ] {
            assert_eq!(number_value(text), None, "{text:?}");
        }
    }

    #[test]
    fn steps_on_the_grid_from_min() {
        assert_eq!(up("1500"), Some("1600".to_owned()));
        assert_eq!(down("1500"), Some("1400".to_owned()));
        assert_eq!(
            up("1550"),
            Some("1600".to_owned()),
            "off the grid, up to the next"
        );
        assert_eq!(
            down("1550"),
            Some("1500".to_owned()),
            "off the grid, down to the next"
        );
        assert_eq!(up("4950"), Some("5000".to_owned()));
        assert_eq!(down("500.5"), Some("500".to_owned()));
    }

    #[test]
    fn an_empty_or_invalid_value_steps_into_range() {
        assert_eq!(up(""), Some("500".to_owned()));
        assert_eq!(down(""), Some("500".to_owned()));
        assert_eq!(up("1e"), Some("500".to_owned()));
        let any = NumberSpec {
            min: 0.,
            max: f64::MAX,
            step: 100.,
        };
        assert_eq!(spin("", StepAction::Increment, any), Some("100".to_owned()));
        assert_eq!(
            spin("", StepAction::Decrement, any),
            Some("0".to_owned()),
            "0 moves up a step to step down"
        );
    }

    #[test]
    fn out_of_range_values_step_back_in_or_stay() {
        assert_eq!(up("100"), Some("500".to_owned()));
        assert_eq!(down("100"), None);
        assert_eq!(down("500"), None);
        assert_eq!(down("6000"), Some("5000".to_owned()));
        assert_eq!(up("6000"), None);
        assert_eq!(up("5000"), None);
        let ragged = NumberSpec {
            min: 500.,
            max: 5050.,
            step: 100.,
        };
        assert_eq!(
            spin("5020", StepAction::Increment, ragged),
            None,
            "the last grid value is below it"
        );
        assert_eq!(
            spin("5000", StepAction::Increment, ragged),
            None,
            "already the last grid value"
        );
        assert_eq!(
            spin("4900", StepAction::Increment, ragged),
            Some("5000".to_owned())
        );
    }

    #[test]
    fn fractions_step_by_the_default_step() {
        let font = NumberSpec {
            min: 8.,
            max: 32.,
            step: 1.,
        };
        assert_eq!(
            spin("13.5", StepAction::Increment, font),
            Some("14".to_owned())
        );
        assert_eq!(
            spin("13.5", StepAction::Decrement, font),
            Some("13".to_owned())
        );
        assert_eq!(
            spin("14", StepAction::Increment, font),
            Some("15".to_owned())
        );
    }

    /// A Timeout field at the window's top left, in a box that counts the wheel events that
    /// reach it.
    struct Host {
        field: Entity<InputState>,
        scrolls: Rc<Cell<usize>>,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let scrolls = self.scrolls.clone();
            div()
                .size_full()
                .on_scroll(move |_, _, _| scrolls.set(scrolls.get() + 1))
                .child(NumberField::new(&self.field, TIMEOUT))
        }
    }

    fn host(
        cx: &mut TestAppContext,
    ) -> (Entity<InputState>, Rc<Cell<usize>>, &mut VisualTestContext) {
        cx.update(gpui_kit::init);
        cx.update(crate::theme::install);
        let scrolls = Rc::new(Cell::new(0));
        let (host, cx) = cx.add_window_view({
            let scrolls = scrolls.clone();
            move |window, cx| Host {
                field: cx.new(|cx| InputState::new(window, cx).default_value("1500")),
                scrolls,
            }
        });
        draw(cx);
        let field = cx.update(|_, cx| host.read(cx).field.clone());
        (field, scrolls, cx)
    }

    fn draw(cx: &mut VisualTestContext) {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }

    fn text(field: &Entity<InputState>, cx: &mut VisualTestContext) -> String {
        cx.update(|_, cx| field.read(cx).value().to_string())
    }

    fn wheel(
        cx: &mut VisualTestContext,
        position: Point<Pixels>,
        delta: ScrollDelta,
        phase: TouchPhase,
        source: ScrollSource,
    ) {
        cx.simulate_event(MouseMoveEvent {
            position,
            pressed_button: None,
            modifiers: Modifiers::default(),
        });
        cx.simulate_event(ScrollEvent {
            position,
            delta,
            modifiers: Modifiers::default(),
            touch_phase: phase,
            source,
        });
        draw(cx);
    }

    /// A mouse wheel's event: it comes without a sequence.
    fn mouse(cx: &mut VisualTestContext, position: Point<Pixels>, delta: ScrollDelta) {
        wheel(
            cx,
            position,
            delta,
            TouchPhase::Moved,
            ScrollSource::Pointer,
        );
    }

    /// A trackpad's event of `y` pixels (below 0 steps down), in its sequence's `phase`.
    fn trackpad(cx: &mut VisualTestContext, position: Point<Pixels>, y: f32, phase: TouchPhase) {
        let delta = ScrollDelta::Pixels(point(px(0.), px(y)));
        wheel(cx, position, delta, phase, ScrollSource::Pointer);
    }

    fn focus(field: &Entity<InputState>, cx: &mut VisualTestContext) {
        cx.update(|window, cx| field.update(cx, |input, cx| input.focus(window, cx)));
        draw(cx);
    }

    /// Over the field, 30px in from its left edge.
    const OVER: Point<Pixels> = point(px(30.), px(16.));
    /// Beside the field, in the box around it.
    const BESIDE: Point<Pixels> = point(px(300.), px(200.));
    const UP: ScrollDelta = ScrollDelta::Lines(point(0., 1.));
    const DOWN: ScrollDelta = ScrollDelta::Lines(point(0., -1.));

    /// A mouse wheel over the field steps it once per event while it is focused, and the box
    /// around it does not scroll; otherwise the box scrolls.
    #[gpui_kit::test]
    fn the_mouse_wheel_steps_the_focused_field(cx: &mut TestAppContext) {
        let (field, scrolls, cx) = host(cx);
        mouse(cx, OVER, UP);
        assert_eq!(text(&field, cx), "1500", "not focused");
        assert_eq!(scrolls.get(), 1);

        focus(&field, cx);
        mouse(cx, OVER, UP);
        assert_eq!(text(&field, cx), "1600");
        mouse(cx, OVER, DOWN);
        mouse(cx, OVER, DOWN);
        assert_eq!(text(&field, cx), "1400");
        assert_eq!(scrolls.get(), 1, "the box does not scroll");

        cx.update(|window, cx| field.update(cx, |input, cx| input.set_value("5000", window, cx)));
        mouse(cx, OVER, UP);
        assert_eq!(text(&field, cx), "5000", "at max");
        assert_eq!(scrolls.get(), 1, "still taken at max");

        mouse(cx, OVER, ScrollDelta::Lines(point(1., 0.)));
        wheel(cx, OVER, UP, TouchPhase::Moved, ScrollSource::Touch);
        mouse(cx, BESIDE, UP);
        assert_eq!(text(&field, cx), "5000");
        assert_eq!(
            scrolls.get(),
            4,
            "a sideways scroll, a touch pan and a wheel beside the field scroll the box"
        );
    }

    /// A trackpad sequence that begins over the focused field steps it once per event over it,
    /// however little each scrolls, momentum after the fingers lift included, and the box
    /// around it does not scroll.
    #[gpui_kit::test]
    fn a_trackpad_sequence_begun_over_the_focused_field_steps_it(cx: &mut TestAppContext) {
        let (field, scrolls, cx) = host(cx);
        focus(&field, cx);
        trackpad(cx, OVER, -2., TouchPhase::Started);
        trackpad(cx, OVER, -0.5, TouchPhase::Moved);
        trackpad(cx, OVER, -0.5, TouchPhase::Moved);
        assert_eq!(text(&field, cx), "1200");
        assert_eq!(scrolls.get(), 0, "the box does not scroll");

        trackpad(cx, OVER, 0., TouchPhase::Ended);
        trackpad(cx, OVER, 3., TouchPhase::Moved);
        assert_eq!(text(&field, cx), "1300", "momentum steps it too");
        let sideways = ScrollDelta::Pixels(point(px(5.), px(0.)));
        wheel(cx, OVER, sideways, TouchPhase::Moved, ScrollSource::Pointer);
        assert_eq!(text(&field, cx), "1300");
        assert_eq!(
            scrolls.get(),
            2,
            "the lift and a sideways scroll, which step nothing, reach the box"
        );
    }

    /// A trackpad sequence that begins beside the focused field, as when it scrolls the dialog
    /// and carries the field under the still pointer, scrolls the box to its end and leaves
    /// the field alone; a mouse wheel over it still steps it.
    #[gpui_kit::test]
    fn a_trackpad_sequence_begun_beside_the_field_scrolls_past_it(cx: &mut TestAppContext) {
        let (field, scrolls, cx) = host(cx);
        focus(&field, cx);
        trackpad(cx, BESIDE, -2., TouchPhase::Started);
        trackpad(cx, OVER, -2., TouchPhase::Moved);
        trackpad(cx, OVER, -2., TouchPhase::Moved);
        trackpad(cx, OVER, 0., TouchPhase::Ended);
        trackpad(cx, OVER, -5., TouchPhase::Moved);
        assert_eq!(text(&field, cx), "1500");
        assert_eq!(scrolls.get(), 5, "every event scrolls the box");

        mouse(cx, OVER, UP);
        assert_eq!(text(&field, cx), "1600");
        assert_eq!(scrolls.get(), 5);
    }

    /// A trackpad sequence that begins over the field before it is focused does not step it;
    /// the next one, begun while it is focused, does.
    #[gpui_kit::test]
    fn a_trackpad_sequence_begun_before_focus_scrolls_past_the_field(cx: &mut TestAppContext) {
        let (field, scrolls, cx) = host(cx);
        trackpad(cx, OVER, -2., TouchPhase::Started);
        focus(&field, cx);
        trackpad(cx, OVER, -2., TouchPhase::Moved);
        assert_eq!(text(&field, cx), "1500");
        assert_eq!(scrolls.get(), 2);

        trackpad(cx, OVER, -2., TouchPhase::Started);
        trackpad(cx, OVER, -2., TouchPhase::Moved);
        assert_eq!(text(&field, cx), "1300");
        assert_eq!(scrolls.get(), 2);
    }

    /// A sequence is over the field when it begins anywhere inside its border, and only there.
    #[gpui_kit::test]
    fn a_trackpad_sequence_is_over_the_field_up_to_its_border(cx: &mut TestAppContext) {
        let (field, _, cx) = host(cx);
        focus(&field, cx);
        for (corner, stepped) in [
            (point(px(0.5), px(0.5)), "1400"),
            (point(px(99.5), px(33.)), "1300"),
            (point(px(100.5), px(16.)), "1300"),
            (point(px(30.), px(34.)), "1300"),
        ] {
            trackpad(cx, corner, 0., TouchPhase::Started);
            trackpad(cx, OVER, -2., TouchPhase::Moved);
            assert_eq!(text(&field, cx), stepped, "begun at {corner:?}");
        }
    }

    /// The spin buttons sit inside the field's right padding: up over down.
    #[gpui_kit::test]
    fn the_spin_buttons_step_and_focus_the_field(cx: &mut TestAppContext) {
        let (field, _, cx) = host(cx);
        cx.simulate_click(point(px(79.), px(10.)), Modifiers::none());
        draw(cx);
        assert_eq!(text(&field, cx), "1600");
        assert!(cx.update(|window, cx| field.read(cx).focus_handle(cx).is_focused(window)));
        cx.simulate_click(point(px(79.), px(23.)), Modifiers::none());
        cx.simulate_click(point(px(79.), px(23.)), Modifiers::none());
        draw(cx);
        assert_eq!(text(&field, cx), "1400");
    }

    /// Only numbers can be typed: letters are dropped, digits, a sign and a point are not.
    #[gpui_kit::test]
    fn only_numbers_can_be_typed(cx: &mut TestAppContext) {
        let (field, _, cx) = host(cx);
        cx.update(|window, cx| {
            field.update(cx, |input, cx| {
                input.set_value("", window, cx);
                input.focus(window, cx);
            })
        });
        draw(cx);
        for typed in ["-", "1", "a", "2", "x", ".", "5"] {
            cx.simulate_input(typed);
        }
        assert_eq!(text(&field, cx), "-12.5");
    }
}
