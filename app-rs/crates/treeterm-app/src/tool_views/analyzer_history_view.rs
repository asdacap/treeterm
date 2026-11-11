//! The Activity History tab (`AnalyzerHistory.tsx`), drawn with its inline styles.

use std::collections::HashSet;

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, Context, Entity, FontWeight, Hsla, SharedString, Subscription, Window, div, px, rgb};
use treeterm_model::tool_tabs::analyzer_history::{
    AnalyzerHistory, HistorySource, buffer_preview, clock, kind_pill, response_preview, summarize, transition_extra,
    transition_kind,
};

use crate::ui::{MONO, normal_line, text_button};

/// What a click expands: a transition's snapshot, or a call's buffer, response or prompt.
/// Indices count from the oldest entry, so they stay put as new ones arrive.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Fold {
    Snapshot(usize),
    Buffer(usize),
    Response(usize),
    Prompt(usize),
}

const GREY: u32 = 0x888888;
const LIGHT: u32 = 0xcccccc;

pub struct AnalyzerHistoryView {
    model: Entity<AnalyzerHistory>,
    open: HashSet<Fold>,
    _observe: Subscription,
}

/// `pillStyle`: white 11px medium text on `background`, padded 1px 6px, rounded 3px.
fn pill(text: impl Into<SharedString>, background: u32, fg: u32, mono: bool) -> gpui_kit::Div {
    div()
        .flex_shrink_0()
        .px(px(6.))
        .py(px(1.))
        .rounded(px(3.))
        .bg(rgb(background))
        .text_color(rgb(fg))
        .font_weight(FontWeight::MEDIUM)
        .when(mono, |d| d.font_family(MONO))
        .child(text.into())
}

/// An 11px monospace span.
fn mono(text: impl Into<SharedString>, color: u32) -> gpui_kit::Div {
    div().flex_shrink_0().font_family(MONO).text_color(rgb(color)).child(text.into())
}

/// A section heading: the `h3` (18.72px bold) with its entry count.
fn heading(title: &'static str, count: usize) -> gpui_kit::Div {
    h_flex()
        .gap(px(12.))
        .child(div().text_size(px(18.72)).line_height(px(22.)).font_weight(FontWeight::BOLD).text_color(rgb(LIGHT)).child(title))
        .child(div().text_size(px(12.)).line_height(normal_line(12.)).text_color(rgb(GREY)).child(format!("{count} entries")))
}

/// The bordered row every entry sits in, its body and Debug button side by side.
fn entry(body: gpui_kit::Div, debug: AnyElement) -> gpui_kit::Div {
    h_flex()
        .items_start()
        .gap(px(12.))
        .px(px(12.))
        .py(px(8.))
        .rounded(px(4.))
        .border_1()
        .border_color(rgb(0x333333))
        .bg(rgb(0x1e1e1e))
        .child(body)
        .child(debug)
}

fn empty(text: &'static str) -> gpui_kit::Div {
    div().p(px(8.)).text_size(px(13.)).line_height(normal_line(13.)).text_color(rgb(GREY)).child(text)
}

impl AnalyzerHistoryView {
    pub fn new(model: Entity<AnalyzerHistory>, cx: &mut Context<Self>) -> Self {
        let observe = cx.observe(&model, |_, _, cx| cx.notify());
        Self { model, open: HashSet::new(), _observe: observe }
    }

    /// A `pre` (11px monospace, wrapping) that expands on click; `boxed` ones sit on #1a1a1a.
    fn fold(&self, fold: Fold, full: String, collapsed: String, color: u32, boxed: bool, cx: &mut Context<Self>) -> AnyElement {
        let id = SharedString::from(match fold {
            Fold::Snapshot(i) => format!("snapshot-{i}"),
            Fold::Buffer(i) => format!("buffer-{i}"),
            Fold::Response(i) => format!("response-{i}"),
            Fold::Prompt(i) => format!("prompt-{i}"),
        });
        let text = if self.open.contains(&fold) { full } else { collapsed };
        div()
            .id(id)
            .min_w_0()
            .font_family(MONO)
            .text_size(px(11.))
            .line_height(normal_line(11.))
            .text_color(rgb(color))
            .cursor_pointer()
            .when(boxed, |d| d.p(px(4.)).rounded(px(3.)).bg(rgb(0x1a1a1a)))
            .children(text.lines().map(|line| div().min_h(normal_line(11.)).child(line.to_owned())).collect::<Vec<_>>())
            .on_click(cx.listener(move |view, _, _, cx| {
                if !view.open.remove(&fold) {
                    view.open.insert(fold);
                }
                cx.notify();
            }))
            .into_any_element()
    }

    fn debug_button(&self, id: String, buffer: String) -> AnyElement {
        let model = self.model.clone();
        text_button(SharedString::from(id), "Debug", 11., (4., 8.), rgb(LIGHT), rgb(0x333333), rgb(0x555555), 4.)
            .on_click(move |_, _, cx| model.update(cx, |m, cx| m.debug(&buffer, cx)))
            .into_any_element()
    }

    fn transitions(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let transitions = self.model.read(cx).transitions(cx);
        let count = transitions.len();
        let mut list = v_flex().gap(px(4.));
        if transitions.is_empty() {
            list = list.child(empty("No transitions yet."));
        }
        for (newest, transition) in transitions.into_iter().enumerate() {
            let index = count - 1 - newest;
            let snapshot = transition.detail.snapshot().to_owned();
            let extra = transition_extra(&transition.detail);
            let body = v_flex()
                .flex_1()
                .min_w_0()
                .gap(px(4.))
                .child(
                    h_flex()
                        .gap(px(8.))
                        .text_size(px(11.))
                        .line_height(normal_line(11.))
                        .child(mono(clock(transition.timestamp), GREY))
                        .child(mono(format!("{} → {}", transition.from.as_str(), transition.to.as_str()), LIGHT))
                        .child(pill(transition_kind(&transition.detail), 0x555555, 0xffffff, false))
                        .when(!extra.is_empty(), |row| row.child(div().text_color(rgb(GREY)).child(extra))),
                )
                .child(self.fold(Fold::Snapshot(index), snapshot.clone(), buffer_preview(&snapshot), 0x666666, false, cx));
            list = list.child(entry(body, self.debug_button(format!("debug-transition-{index}"), snapshot)));
        }
        v_flex().gap(px(12.)).child(heading("Activity Transitions", count)).child(list).into_any_element()
    }

    fn calls(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let calls = self.model.read(cx).calls(cx)?;
        let count = calls.len();
        let mut list = v_flex().gap(px(4.));
        if calls.is_empty() {
            list = list.child(empty("No history entries yet."));
        }
        for (newest, call) in calls.into_iter().enumerate() {
            let index = count - 1 - newest;
            let summary = summarize(&call);
            let (kind, kind_color) = kind_pill(call.kind);
            let mut body = v_flex().flex_1().min_w_0().gap(px(4.)).child(
                h_flex()
                    .gap(px(8.))
                    .text_size(px(11.))
                    .line_height(normal_line(11.))
                    .child(mono(clock(call.timestamp), GREY))
                    .child(pill(kind, kind_color, 0xffffff, false))
                    .child(mono(call.model.clone(), GREY))
                    .when(summary.cached, |row| row.child(pill("cached", 0x2e7d32, 0xffffff, false)))
                    .when_some(summary.duration, |row, duration| {
                        row.child(pill(format!("{}ms", duration.as_millis()), 0x555555, 0xdddddd, true))
                    })
                    .when(summary.error.is_some(), |row| row.child(pill("error", 0xf44747, 0xffffff, false))),
            );
            if let Some(message) = &summary.error {
                body = body.child(
                    div().text_size(px(12.)).line_height(normal_line(12.)).text_color(rgb(0xf44747)).child(message.clone()),
                );
            }
            body = body.child(self.fold(
                Fold::Buffer(index),
                call.buffer_text.clone(),
                buffer_preview(&call.buffer_text),
                0x666666,
                false,
                cx,
            ));
            if !summary.response.is_empty() {
                let collapsed = response_preview(&summary.response);
                body = body.child(self.fold(Fold::Response(index), summary.response.clone(), collapsed, 0x23d18b, true, cx));
            }
            if let Some(prompt) = summary.system_prompt {
                body = body.child(self.fold(
                    Fold::Prompt(index),
                    prompt,
                    "system prompt (click to expand)".to_owned(),
                    0xb5cea8,
                    true,
                    cx,
                ));
            }
            list = list.child(entry(body, self.debug_button(format!("debug-call-{index}"), call.buffer_text.clone())));
        }
        Some(
            v_flex()
                .gap(px(12.))
                .child(
                    heading("Analyzer History", count).child(
                        text_button("analyzer-refresh", "Refresh", 12., (4., 12.), rgb(LIGHT), rgb(0x333333), rgb(0x555555), 4.)
                            // The list follows the analyzer live; Refresh just redraws it.
                            .on_click(cx.listener(|_, _, _, cx| cx.notify())),
                    ),
                )
                .child(list)
                .into_any_element(),
        )
    }
}

impl Render for AnalyzerHistoryView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if *self.model.read(cx).source() == HistorySource::Missing {
            let error: Hsla = rgb(0xf14c4c).into();
            return v_flex().id("analyzer-history").p(px(16.)).text_color(error).child("Invalid state: missing sourceTabId");
        }
        let transitions = self.transitions(cx);
        let calls = self.calls(cx);
        v_flex()
            .id("analyzer-history")
            .size_full()
            .p(px(16.))
            .gap(px(24.))
            .overflow_y_scroll()
            .child(transitions)
            .children(calls)
    }
}
