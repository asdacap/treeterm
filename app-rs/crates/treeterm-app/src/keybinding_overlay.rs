//! The prefix-mode overlay: which keys do what, and how long is left. Port of
//! `KeybindingOverlay.tsx`.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    Animation, AnimationExt as _, AnyElement, App, BoxShadow, Entity, FontWeight, Window, div, linear_color_stop,
    linear_gradient, point, px, relative,
    rgb, rgba,
};
use treeterm_core::keybinding::{Os, PrefixMode};
use treeterm_model::keybinding_store::KeybindingStore;

use crate::theme::colors;
use crate::ui::MONO;

/// Height of a `.keybinding-overlay-key` (11px text, 3px padding, 1px border): a grid row.
const ROW: f32 = 20.667;

pub fn render(store: &Entity<KeybindingStore>, window: &mut Window, cx: &App) -> Option<AnyElement> {
    let machine = store.read(cx).machine();
    let mode = machine.mode();
    if mode == PrefixMode::Idle {
        return None;
    }
    // The progress bar drains continuously while the overlay is up.
    window.request_animation_frame();
    let c = colors(cx);
    let os = Os::host();
    let remaining = machine.remaining_fraction(std::time::Instant::now());
    let (header, items): (Vec<AnyElement>, Vec<(String, String)>) = match mode {
        PrefixMode::WorkspaceFocus => (
            vec![title("Workspace Focus", cx), hint("Navigate: ↑/↓ • Select: Enter • Cancel: Esc", cx)],
            vec![
                ("↑ / ↓".to_owned(), "Navigate Workspaces".to_owned()),
                ("Enter".to_owned(), "Select Workspace".to_owned()),
                ("Esc".to_owned(), "Cancel".to_owned()),
            ],
        ),
        PrefixMode::Active | PrefixMode::Idle => {
            let mut items: Vec<(String, String)> = machine
                .settings()
                .actions
                .overlay_items(os)
                .into_iter()
                .map(|item| (item.key_label.to_uppercase(), item.action.label().to_owned()))
                .collect();
            // Electron sorts the upper-cased keys.
            items.sort_by(|a, b| a.0.cmp(&b.0));
            let prefix = div()
                .px(px(8.))
                .py(px(3.))
                .rounded(px(4.))
                .bg(c.accent)
                .font_family(MONO)
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(gpui_kit::white())
                .child(machine.settings().prefix_key.label(os))
                .into_any_element();
            (vec![prefix, title("Prefix Mode", cx), hint("Press a key or Esc to cancel", cx)], items)
        }
    };
    // `grid-template-columns: repeat(2, auto 1fr); gap: 6px 12px`: items fill rows left to
    // right; each key column is as wide as its widest key, which every key stretches to.
    let column = |offset: usize| {
        let mut keys = v_flex().flex_shrink_0().gap(px(6.));
        let mut actions = v_flex().flex_1().min_w_0().gap(px(6.));
        for (key, action) in items.iter().skip(offset).step_by(2) {
            keys = keys.child(
                div()
                    .h(px(ROW))
                    .min_w(px(28.))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(4.))
                    .border_1()
                    .border_color(c.border)
                    .bg(c.bg_tertiary)
                    .shadow(vec![BoxShadow {
                        color: rgba(0x00000033).into(),
                        offset: point(px(0.), px(1.)),
                        blur_radius: px(2.),
                        spread_radius: px(0.),
                        inset: false,
                    }])
                    .font_family(MONO)
                    .font_weight(FontWeight::BOLD)
                    .text_color(c.accent)
                    .child(key.clone()),
            );
            actions = actions.child(div().h(px(ROW)).flex().items_center().text_color(c.text_primary).child(action.clone()));
        }
        (keys, actions)
    };
    let (keys_a, actions_a) = column(0);
    let (keys_b, actions_b) = column(1);
    Some(
        div()
            .absolute()
            .bottom(px(12.))
            .left_0()
            .right_0()
            .flex()
            .justify_center()
            .child(
                v_flex()
                    .min_w(px(500.))
                    .max_w(px(650.))
                    .px(px(14.))
                    .py(px(10.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(c.border)
                    .bg(rgba(0x1e1e1ef2))
                    .shadow(vec![BoxShadow {
                        color: rgba(0x00000066).into(),
                        offset: point(px(0.), px(4.)),
                        blur_radius: px(16.),
                        spread_radius: px(0.),
                        inset: false,
                    }])
                    .text_size(px(11.))
                    .line_height(px(13.))
                    .child(
                        h_flex()
                            .gap(px(8.))
                            .pb(px(6.))
                            .mb(px(8.))
                            .border_b_1()
                            .border_color(c.border)
                            .children(header),
                    )
                    .child(h_flex().items_start().gap(px(12.)).child(keys_a).child(actions_a).child(keys_b).child(actions_b))
                    .child(
                        div()
                            .mt(px(8.))
                            .h(px(2.))
                            .w_full()
                            .rounded(px(1.))
                            .overflow_hidden()
                            .bg(c.bg_tertiary)
                            .child(div().h_full().w(relative(remaining)).rounded(px(2.)).bg(linear_gradient(
                                90.,
                                linear_color_stop(c.accent, 0.),
                                linear_color_stop(rgb(0x00a6ff), 1.),
                            ))),
                    )
                    // `overlay-appear`: fades in and rises 20px over 0.2s.
                    .relative()
                    .with_animation(
                        "keybinding-overlay-appear",
                        Animation::new(std::time::Duration::from_millis(200)).with_easing(|t: f32| 1. - (1. - t) * (1. - t)),
                        |overlay, delta| overlay.opacity(delta).top(px(20. * (1. - delta))),
                    ),
            )
            .into_any_element(),
    )
}

/// `.keybinding-overlay-title`: semibold, taking the free space.
fn title(text: &'static str, cx: &App) -> AnyElement {
    div().flex_1().font_weight(FontWeight::SEMIBOLD).text_color(colors(cx).text_primary).child(text).into_any_element()
}

/// `.keybinding-overlay-hint`: 10px italic secondary.
fn hint(text: &'static str, cx: &App) -> AnyElement {
    div().text_size(px(10.)).italic().text_color(colors(cx).text_secondary).child(text).into_any_element()
}
