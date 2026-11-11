//! `TrafficBadge.tsx`: "↓ 1.2 KB/s", with the total received as its tooltip. Its own view, so
//! a rate change re-renders only the badge.

use gpui_kit::prelude::*;
use gpui_kit::{Context, Entity, SharedString, Window, div, px};
use treeterm_core::traffic::{format_bytes, format_rate};
use treeterm_model::traffic::TrafficMeter;

use crate::theme::colors;

pub struct TrafficBadge {
    meter: Entity<TrafficMeter>,
    id: SharedString,
}

impl TrafficBadge {
    pub fn new(meter: Entity<TrafficMeter>, id: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        cx.observe(&meter, |_, _, cx| cx.notify()).detach();
        Self { meter, id: id.into() }
    }
}

impl Render for TrafficBadge {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let meter = self.meter.read(cx);
        let rate = meter.rate();
        let total = format_bytes(rate.total_bytes);
        // `ConnectionTrafficBadge` reads "↓ …" until the first sample; a workspace starts at 0.
        let text = if self.id.as_ref() == "connection-traffic" && !meter.is_sampled() {
            "↓ …".to_owned()
        } else {
            format!("↓ {}", format_rate(rate.bytes_per_sec))
        };
        div()
            .id(self.id.clone())
            .flex_shrink_0()
            .text_size(px(crate::ui::SMALL))
            .font_weight(gpui_kit::FontWeight::NORMAL)
            // `font-variant-numeric: tabular-nums`: the rate does not jitter as it changes.
            .font_features(gpui_kit::FontFeatures(std::sync::Arc::new(vec![("tnum".into(), 1)])))
            .whitespace_nowrap()
            .text_color(colors(cx).text_secondary)
            .child(text)
            .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(total.clone()).build(window, cx))
    }
}
