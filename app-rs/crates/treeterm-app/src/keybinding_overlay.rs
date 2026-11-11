//! The prefix-mode overlay: which keys do what, and how long is left. Port of
//! `KeybindingOverlay.tsx`.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, Entity, Window, div, px, relative, rgba};
use treeterm_core::keybinding::{Os, PrefixMode};
use treeterm_model::keybinding_store::KeybindingStore;

use crate::theme::colors;

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
    let key = |label: String| {
        div()
            .px(px(8.))
            .py(px(3.))
            .min_w(px(28.))
            .rounded(px(4.))
            .border_1()
            .border_color(c.border)
            .bg(c.bg_tertiary)
            .text_color(c.accent)
            .font_weight(gpui_kit::FontWeight::BOLD)
            .text_center()
            .child(label)
    };
    let (title, hint, body) = match mode {
        PrefixMode::WorkspaceFocus => (
            "Workspace Focus".to_owned(),
            "Navigate: ↑/↓ • Select: Enter • Cancel: Esc",
            h_flex()
                .gap(px(12.))
                .child(key("↑ ↓".into()))
                .child(div().child("Move between workspaces"))
                .child(key("Enter".into()))
                .child(div().child("Open workspace"))
                .into_any_element(),
        ),
        PrefixMode::Active | PrefixMode::Idle => {
            let mut grid = div().flex().flex_wrap().gap(px(6.));
            for item in machine.settings().actions.overlay_items(os) {
                grid = grid.child(
                    h_flex()
                        .w(relative(0.48))
                        .gap(px(12.))
                        .child(key(item.key_label.to_uppercase()))
                        .child(div().text_color(c.text_primary).child(item.action.label())),
                );
            }
            (
                machine.settings().prefix_key.label(os),
                "Press a key…",
                grid.into_any_element(),
            )
        }
    };
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
                    .text_size(px(11.))
                    .child(
                        h_flex()
                            .gap(px(8.))
                            .pb(px(6.))
                            .mb(px(8.))
                            .border_b_1()
                            .border_color(c.border)
                            .child(
                                div()
                                    .px(px(8.))
                                    .py(px(3.))
                                    .rounded(px(4.))
                                    .bg(c.accent)
                                    .text_color(gpui_kit::white())
                                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                    .child(title),
                            )
                            .child(div().flex_1())
                            .child(div().text_size(px(10.)).italic().text_color(c.text_secondary).child(hint)),
                    )
                    .child(body)
                    .child(
                        div()
                            .mt(px(8.))
                            .h(px(2.))
                            .w_full()
                            .rounded(px(1.))
                            .bg(c.bg_tertiary)
                            .child(div().h_full().w(relative(remaining)).rounded(px(1.)).bg(c.accent)),
                    ),
            )
            .into_any_element(),
    )
}
