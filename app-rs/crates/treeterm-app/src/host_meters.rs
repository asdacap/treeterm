//! A remote session header's CPU and memory: two 3px bars, CPU over memory, coloured as the
//! session pane's Monitor colours its bars (green, amber from 60 %, red from 85 %), from the
//! monitor script run on the host every second. While the script cannot run they are greyed
//! out. Hovering them shows the numbers. Its own view, so a sample re-renders only the bars.

use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, Context, Entity, Hsla, Subscription, Window, div, px, relative, rgb};
use treeterm_core::connection::ConnectionId;
use treeterm_core::system_monitor::{SystemMetrics, utilization_color};
use treeterm_core::traffic::{format_bytes, format_rate};
use treeterm_model::app_store::AppStore;
use treeterm_model::system_monitor::{Metrics, Polling, SystemMonitor};
use treeterm_model::traffic::{Reading, TrafficMeter};

use crate::theme::colors;

pub struct HostMeters {
    monitor: Entity<SystemMonitor>,
    /// The connection's download meter, for the details' Traffic row.
    traffic: Entity<TrafficMeter>,
    _observe: Subscription,
}

impl HostMeters {
    /// Starts sampling `connection`'s host; stops when dropped.
    pub fn new(
        app: Entity<AppStore>,
        connection: ConnectionId,
        traffic: Entity<TrafficMeter>,
        cx: &mut Context<Self>,
    ) -> Self {
        let monitor = cx.new(|cx| SystemMonitor::new(app, connection, Polling::Meters, cx));
        let observe = cx.observe(&monitor, |_, _, cx| cx.notify());
        Self { monitor, traffic, _observe: observe }
    }
}

/// A `height` px track filled to `percent`.
fn bar(percent: f64, height: f32, track: Hsla) -> gpui_kit::Div {
    div()
        .h(px(height))
        .rounded(px(height / 2.))
        .overflow_hidden()
        .bg(track)
        .child(div().h_full().w(relative((percent / 100.) as f32)).bg(rgb(utilization_color(percent))))
}

fn tabular() -> gpui_kit::FontFeatures {
    gpui_kit::FontFeatures(std::sync::Arc::new(vec![("tnum".into(), 1)]))
}

impl Render for HostMeters {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors(cx);
        let track = c.border;
        let metrics = self.monitor.read(cx).metrics();
        let ([cpu, memory], greyed) = match metrics {
            Metrics::Loaded(m) => ([m.cpu.usage_percent, m.memory.usage_percent], false),
            Metrics::Loading => ([0., 0.], false),
            Metrics::Failed(_) => ([0., 0.], true),
        };
        let (monitor, traffic) = (self.monitor.clone(), self.traffic.clone());
        v_flex()
            .id("host-meters")
            .w(px(28.))
            .flex_shrink_0()
            .ml(px(6.))
            .gap(px(2.))
            .when(greyed, |d| d.opacity(0.4))
            .child(bar(cpu, 3., track))
            .child(bar(memory, 3., track))
            .tooltip(move |window, cx| {
                let (monitor, traffic) = (monitor.clone(), traffic.clone());
                Tooltip::element(move |_, cx| details(&monitor, &traffic, cx)).build(window, cx)
            })
    }
}

/// The hover: the latest sample in numbers, else why there is none.
fn details(monitor: &Entity<SystemMonitor>, traffic: &Entity<TrafficMeter>, cx: &App) -> AnyElement {
    match monitor.read(cx).metrics() {
        Metrics::Loading => div().child("Collecting…").into_any_element(),
        Metrics::Failed(why) => div().max_w(px(400.)).child(format!("Unavailable: {why}")).into_any_element(),
        Metrics::Loaded(m) => loaded(m, traffic.read(cx).reading(), cx),
    }
}

fn loaded(m: &SystemMetrics, traffic: Reading, cx: &App) -> AnyElement {
    let c = colors(cx);
    let (primary, secondary, track) = (c.text_primary, c.text_secondary, c.border);
    let row = |label: &'static str, value: AnyElement| {
        h_flex()
            .gap(px(8.))
            .items_center()
            .child(div().w(px(56.)).flex_shrink_0().text_color(secondary).child(label))
            .child(div().flex_1().min_w_0().child(value))
    };
    let gauge = |percent: f64| {
        h_flex()
            .gap(px(8.))
            .items_center()
            .child(div().flex_1().child(bar(percent, 4., track)))
            .child(div().w(px(32.)).text_right().child(format!("{percent:.0}%")))
            .into_any_element()
    };
    let cores = match m.cpu.core_count {
        1 => "1 core".to_owned(),
        n => format!("{n} cores"),
    };
    let [one, five, fifteen] = m.load_average;
    let traffic = match traffic {
        Reading::Loading => "↓ …".to_owned(),
        Reading::Ready(rate) => {
            format!("↓ {} · {} total", format_rate(rate.bytes_per_sec), format_bytes(rate.total_bytes))
        }
    };
    v_flex()
        .w(px(240.))
        .gap(px(6.))
        .text_size(px(12.))
        .text_color(primary)
        .font_features(tabular())
        .child(
            h_flex()
                .justify_between()
                .text_size(px(11.))
                .text_color(secondary)
                .child(cores)
                .child(format!("up {}", m.uptime)),
        )
        .child(row("CPU", gauge(m.cpu.usage_percent)))
        .child(row("Load", div().child(format!("{one:.2}  {five:.2}  {fifteen:.2}")).into_any_element()))
        .child(row("Memory", gauge(m.memory.usage_percent)))
        .child(row(
            "",
            div()
                .text_color(secondary)
                .child(format!(
                    "{} of {} used",
                    format_bytes(m.memory.used_bytes),
                    format_bytes(m.memory.total_bytes)
                ))
                .into_any_element(),
        ))
        .child(row("Traffic", div().child(traffic).into_any_element()))
        .into_any_element()
}
