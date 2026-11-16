//! `<input type="number" class="settings-input" min max step>` as Chromium draws it: a text
//! field that takes numbers, with spin buttons (shown while it is hovered or focused) and
//! ArrowUp/ArrowDown that step its value as Chromium's do. Typed values are never clamped:
//! `min` and `max` only bound the stepping.

use gpui_kit::assets::IconName;
use gpui_kit::base::{Button, NumberInput, StepAction};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::prelude::*;
use gpui_kit::{App, Entity, Focusable as _, SharedString, Window, div, px};

use crate::theme::colors;
use crate::ui;

/// `input[type=number].settings-input`: 100px wide.
pub const WIDTH: f32 = 100.;

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
        div()
            .group(field)
            .w(px(WIDTH))
            .h(px(33.5))
            .px(px(12.))
            .py(px(4.))
            .rounded(px(4.))
            .border_1()
            .border_color(if focused { c.accent } else { c.border })
            .bg(c.bg_tertiary)
            .child(
                NumberInput::new(&self.state)
                    .controls_right()
                    .size_full()
                    .on_step(move |action, window, cx| {
                        state.update(cx, |input, cx| {
                            input.focus(window, cx);
                            if let Some(text) = spin(&input.value(), action, spec) {
                                input.set_value(text, window, cx);
                            }
                        });
                    })
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
}
