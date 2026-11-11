//! The SSH Monitor sub-tab (`SystemMonitor.tsx`): CPU and memory graphs, disk gauges, load /
//! uptime / cores tiles and the busiest processes with Stop and Kill.

use gpui_kit::assets::IconName;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, Bounds, Context, Entity, FontWeight, Hsla, PathBuilder, Pixels, SharedString, Subscription, Window,
    canvas, div, point, px, relative, rgb, rgba,
};
use treeterm_core::exec::ExecClient;
use treeterm_core::system_monitor::utilization_color;
use treeterm_core::traffic::format_bytes;
use treeterm_model::system_monitor::{Metrics, SystemMonitor};

use crate::theme::colors;
use crate::ui::MONO;

pub struct MonitorView {
    monitor: Entity<SystemMonitor>,
    _observe: Subscription,
}

impl MonitorView {
    pub fn new(exec: ExecClient, cx: &mut Context<Self>) -> Self {
        let monitor = cx.new(|cx| SystemMonitor::new(exec, cx));
        let observe = cx.observe(&monitor, |_, _, cx| cx.notify());
        Self { monitor, _observe: observe }
    }

    fn refresh(&self, cx: &mut Context<Self>) {
        self.monitor.update(cx, |m, cx| m.refresh(cx));
    }
}

/// `.system-monitor-section-title`.
fn section(title: &'static str, secondary: Hsla) -> gpui_kit::Div {
    v_flex().gap(px(6.)).child(
        div()
            .mb(px(2.))
            .text_size(px(11.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(secondary)
            .child(title.to_uppercase()),
    )
}

/// `.system-monitor-gauge-label`: 36px wide, 11px secondary.
fn gauge_label(text: impl Into<SharedString>, secondary: Hsla) -> gpui_kit::Div {
    // Long mount labels overflow the 36px column, as in Electron, rather than wrap.
    div().w(px(36.)).flex_shrink_0().whitespace_nowrap().text_size(px(11.)).text_color(secondary).child(text.into())
}

/// The graph inside its 80px box: dashed 0/50/100% rules, the area at 15% and the line.
fn graph(samples: Vec<f64>, color: Hsla, rule: Hsla) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds: Bounds<Pixels>, (), window, _| {
            // The SVG's 300×80 viewBox, padded 2, stretched over the box.
            let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
            let at = |x: f32, y: f32| point(bounds.origin.x + px(x / 300. * w), bounds.origin.y + px(y / 80. * h));
            let y_of = |percent: f64| 2. + 76. - (percent as f32 / 100.) * 76.;
            for percent in [0., 50., 100.] {
                let mut line = PathBuilder::stroke(px(0.5)).dash_array(&[px(4.), px(3.)]);
                line.move_to(at(2., y_of(percent)));
                line.line_to(at(298., y_of(percent)));
                if let Ok(path) = line.build() {
                    window.paint_path(path, rule);
                }
            }
            let points: Vec<_> = match samples.len() {
                0 => return,
                1 => vec![at(2., y_of(samples[0])), at(298., y_of(samples[0]))],
                n => samples
                    .iter()
                    .enumerate()
                    .map(|(i, &value)| at(2. + i as f32 / (n - 1) as f32 * 296., y_of(value)))
                    .collect(),
            };
            let mut area = PathBuilder::fill();
            area.move_to(at(2., 78.));
            for &p in &points {
                area.line_to(p);
            }
            area.line_to(at(298., 78.));
            area.close();
            if let Ok(path) = area.build() {
                window.paint_path(path, color.opacity(0.15));
            }
            let mut line = PathBuilder::stroke(px(1.5));
            line.move_to(points[0]);
            for &p in &points[1..] {
                line.line_to(p);
            }
            if let Ok(path) = line.build() {
                window.paint_path(path, color);
            }
        },
    )
    .size_full()
}

/// `UsageGraph`: label, current value and detail over the graph with its axis.
fn usage_graph(label: &'static str, samples: Vec<f64>, detail: String, cx: &gpui_kit::App) -> impl IntoElement + use<> {
    let c = colors(cx);
    let current = samples.last().copied().unwrap_or(0.);
    let color: Hsla = rgb(utilization_color(current)).into();
    v_flex()
        .font_family(MONO)
        .child(
            h_flex()
                .gap(px(8.))
                .mb(px(4.))
                .child(gauge_label(label, c.text_secondary))
                .child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).text_color(color).child(format!("{current:.1}%")))
                .child(div().flex_shrink_0().text_size(px(10.)).text_color(c.text_secondary).child(detail)),
        )
        .child(
            h_flex()
                .gap(px(4.))
                .mb(px(6.))
                .items_stretch()
                .child(
                    v_flex()
                        .min_w(px(28.))
                        .py(px(2.))
                        .justify_between()
                        .items_end()
                        .text_size(px(9.))
                        .line_height(px(11.))
                        .children(["100%", "50%", "0%"]),
                )
                .child(
                    div()
                        .flex_1()
                        .h(px(80.))
                        .p(px(1.))
                        .border_1()
                        .border_color(c.border)
                        .rounded(px(4.))
                        .bg(c.bg_tertiary)
                        .child(graph(samples, color, c.border)),
                ),
        )
}

/// `Gauge`: a disk's mount point and usage bar.
fn gauge(label: String, percent: f64, c: &crate::theme::Colors) -> impl IntoElement {
    let fill = (percent.clamp(0., 100.) / 100.) as f32;
    h_flex()
        .gap(px(8.))
        .font_family(MONO)
        .child(gauge_label(label, c.text_secondary))
        .child(
            div()
                .relative()
                .flex_1()
                .min_w(px(60.))
                .h(px(14.))
                .rounded(px(3.))
                .bg(c.bg_tertiary)
                .overflow_hidden()
                .child(div().h_full().w(relative(fill)).rounded(px(3.)).bg(rgb(utilization_color(percent))))
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .right(px(4.))
                        .text_size(px(10.))
                        .line_height(px(14.))
                        .text_color(c.text_primary)
                        .child(format!("{percent:.0}%")),
                ),
        )
}

impl Render for MonitorView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors(cx);
        let (secondary, primary, border) = (c.text_secondary, c.text_primary, c.border);
        let root = v_flex()
            .id("system-monitor")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p(px(12.))
            .gap(px(16.))
            .text_size(px(12.));
        let (metrics, cpu, memory, updated) = {
            let m = self.monitor.read(cx);
            (
                m.metrics().clone(),
                m.cpu_history().samples().collect::<Vec<f64>>(),
                m.memory_history().samples().collect::<Vec<f64>>(),
                m.updated_at(),
            )
        };
        let m = match metrics {
            Metrics::Loading => {
                return root.child(
                    h_flex()
                        .justify_center()
                        .gap(px(8.))
                        .p(px(32.))
                        .text_color(secondary)
                        .child(Spinner::new().icon(gpui_kit::component::Icon::new(IconName::LoaderCircle)).with_size(px(16.)).color(secondary))
                        .child("Collecting system metrics..."),
                );
            }
            Metrics::Failed(message) => {
                return root.child(
                    v_flex()
                        .p(px(12.))
                        .items_start()
                        .child(div().font_family(MONO).text_size(px(12.)).text_color(rgb(0xf44336)).child(message))
                        .child(
                            crate::ui::text_button("monitor-retry", "Retry", 11., (4., 12.), secondary, gpui_kit::transparent_black(), border, 4.)
                                .mt(px(8.))
                                .hover(move |d| d.text_color(primary).border_color(secondary))
                                .on_click(cx.listener(|view, _, _, cx| view.refresh(cx))),
                        ),
                );
            }
            Metrics::Loaded(m) => m,
        };
        let mut resources = section("Resources", secondary)
            .child(usage_graph("CPU", cpu, format!("{} cores", m.cpu.core_count), cx))
            .child(usage_graph(
                "MEM",
                memory,
                format!("{} / {}", format_bytes(m.memory.used_bytes), format_bytes(m.memory.total_bytes)),
                cx,
            ));
        for disk in &m.disks {
            let chars: Vec<char> = disk.mount_point.chars().collect();
            let label = if chars.len() > 4 {
                format!("...{}", chars[chars.len() - 4..].iter().collect::<String>())
            } else {
                disk.mount_point.clone()
            };
            resources = resources.child(gauge(label, disk.usage_percent, c));
        }
        let stat = |label: &'static str, value: String| {
            v_flex()
                .flex_1()
                .min_w_0()
                .px(px(10.))
                .py(px(8.))
                .rounded(px(4.))
                .border_1()
                .border_color(border)
                .bg(c.bg_secondary)
                .child(div().mb(px(3.)).text_size(px(10.)).text_color(secondary).child(label))
                .child(div().font_family(MONO).font_weight(FontWeight::SEMIBOLD).text_color(primary).child(value))
        };
        let load = m.load_average.iter().map(|v| format!("{v:.2}")).collect::<Vec<_>>().join("  ");
        let system = section("System", secondary).child(
            h_flex()
                .gap(px(8.))
                .items_stretch()
                .child(stat("Load (1/5/15m)", load))
                .child(stat("Uptime", m.uptime.clone()))
                .child(stat("Cores", m.cpu.core_count.to_string())),
        );
        // `grid-template-columns: 56px 72px 52px 52px 1fr auto; gap: 4px`.
        let cells = |row: gpui_kit::Stateful<gpui_kit::Div>, pid: String, user: String, cpu: String, mem: String, cmd: String| {
            row.flex()
                .items_center()
                .gap(px(4.))
                .child(div().w(px(56.)).flex_shrink_0().child(pid))
                .child(div().w(px(72.)).flex_shrink_0().truncate().child(user))
                .child(div().w(px(52.)).flex_shrink_0().child(cpu))
                .child(div().w(px(52.)).flex_shrink_0().child(mem))
                .child(div().flex_1().min_w_0().truncate().child(cmd))
        };
        let processes = (!m.processes.is_empty()).then(|| {
            let mut table = v_flex().font_family(MONO).text_size(px(11.)).child(
                cells(
                    div().id("process-header").py(px(4.)).border_b_1().border_color(border),
                    "PID".into(),
                    "USER".into(),
                    "%CPU".into(),
                    "%MEM".into(),
                    "COMMAND".into(),
                )
                .text_size(px(10.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(secondary)
                .child(div().w(px(38.)).flex_shrink_0()),
            );
            for (i, process) in m.processes.iter().enumerate() {
                let group = SharedString::from(format!("process-{i}"));
                // `:nth-child(even)`: the header is the first child, so the first row is tinted.
                table = table.child(
                    cells(
                        div().id(group.clone()).group(group.clone()).py(px(3.)).text_color(primary).when(i % 2 == 0, |d| d.bg(rgba(0xffffff05))),
                        process.pid.to_string(),
                        process.user.clone(),
                        format!("{:.1}", process.cpu_percent),
                        format!("{:.1}", process.mem_percent),
                        process.command.clone(),
                    )
                    .child(self.process_actions(i, process.pid, &group, secondary)),
                );
            }
            section("Top Processes", secondary).child(table)
        });
        let footer = h_flex()
            .justify_between()
            .child(
                div()
                    .pt(px(4.))
                    .text_size(px(10.))
                    .text_color(secondary)
                    .child(format!("Updated {}", updated.map(treeterm_core::clock::time_of_day).unwrap_or_default())),
            )
            .child(
                div()
                    .id("monitor-refresh")
                    .flex()
                    .items_center()
                    .px(px(6.))
                    .py(px(3.))
                    .rounded(px(4.))
                    .border_1()
                    .border_color(border)
                    .text_color(secondary)
                    .cursor_pointer()
                    .hover(move |d| d.text_color(primary).border_color(secondary))
                    .child(gpui_kit::component::Icon::new(IconName::RefreshCw).size(px(12.)))
                    .on_click(cx.listener(|view, _, _, cx| view.refresh(cx))),
            );
        root.child(resources).child(system).children(processes).child(footer)
    }
}

impl MonitorView {
    /// Stop (SIGTERM) and Kill (SIGKILL), shown while the row is hovered.
    fn process_actions(&self, i: usize, pid: u32, group: &SharedString, secondary: Hsla) -> AnyElement {
        let button = |name: &'static str, glyph: IconName, hover: u32, signal: &'static str| {
            let monitor = self.monitor.clone();
            div()
                .id(SharedString::from(format!("process-{name}-{i}")))
                .flex()
                .items_center()
                .justify_center()
                .px(px(4.))
                .py(px(2.))
                .rounded(px(3.))
                .border_1()
                .border_color(gpui_kit::transparent_black())
                .text_color(secondary)
                .cursor_pointer()
                .hover(move |d| d.text_color(rgb(hover)).border_color(rgb(hover)))
                .child(gpui_kit::component::Icon::new(glyph).size(px(10.)))
                .on_click(move |_, _, cx| monitor.update(cx, |m, cx| m.signal(pid, signal, cx)))
        };
        h_flex()
            .gap(px(2.))
            .opacity(0.)
            .group_hover(group.clone(), |d| d.opacity(1.))
            .child(button("stop", IconName::Square, 0xff9800, "TERM"))
            .child(button("kill", IconName::X, 0xf44336, "9"))
            .into_any_element()
    }
}
