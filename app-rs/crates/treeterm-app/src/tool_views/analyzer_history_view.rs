//! The Activity History tab (`AnalyzerHistory.tsx`).

use std::collections::HashSet;

use gpui_kit::component::button::Button;
use gpui_kit::component::{Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, Context, Entity, Hsla, SharedString, Subscription, Window, div, rgb};
use treeterm_model::tool_tabs::analyzer_history::{
    AnalyzerHistory, HistorySource, buffer_preview, clock, kind_pill, response_preview, summarize, transition_extra,
    transition_kind,
};

use super::{heading, pill};
use crate::theme::colors;

/// What a click expands: a transition's snapshot, or a call's buffer, response or prompt.
/// Indices count from the oldest entry, so they stay put as new ones arrive.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Fold {
    Snapshot(usize),
    Buffer(usize),
    Response(usize),
    Prompt(usize),
}

pub struct AnalyzerHistoryView {
    model: Entity<AnalyzerHistory>,
    open: HashSet<Fold>,
    _observe: Subscription,
}

impl AnalyzerHistoryView {
    pub fn new(model: Entity<AnalyzerHistory>, cx: &mut Context<Self>) -> Self {
        let observe = cx.observe(&model, |_, _, cx| cx.notify());
        Self { model, open: HashSet::new(), _observe: observe }
    }

    /// A monospace block that expands on click.
    fn fold(&self, fold: Fold, full: String, collapsed: String, color: Hsla, cx: &mut Context<Self>) -> AnyElement {
        let id = SharedString::from(match fold {
            Fold::Snapshot(i) => format!("snapshot-{i}"),
            Fold::Buffer(i) => format!("buffer-{i}"),
            Fold::Response(i) => format!("response-{i}"),
            Fold::Prompt(i) => format!("prompt-{i}"),
        });
        let text = if self.open.contains(&fold) { full } else { collapsed };
        div()
            .id(id)
            .font_family("Menlo")
            .text_xs()
            .text_color(color)
            .cursor_pointer()
            .children(text.lines().map(|line| div().child(line.to_owned())).collect::<Vec<_>>())
            .on_click(cx.listener(move |view, _, _, cx| {
                if !view.open.remove(&fold) {
                    view.open.insert(fold);
                }
                cx.notify();
            }))
            .into_any_element()
    }

    fn debug_button(&self, id: String, buffer: String) -> Button {
        let model = self.model.clone();
        Button::new(SharedString::from(id))
            .xsmall()
            .outline()
            .label("Debug")
            .on_click(move |_, _, cx| model.update(cx, |m, cx| m.debug(&buffer, cx)))
    }

    fn transitions(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let c = colors(cx);
        let (secondary, border, card) = (c.text_secondary, c.border, c.bg_primary);
        let transitions = self.model.read(cx).transitions(cx);
        let count = transitions.len();
        let mut list = v_flex().gap_1();
        if transitions.is_empty() {
            list = list.child(div().p_2().text_sm().text_color(secondary).child("No transitions yet."));
        }
        for (newest, transition) in transitions.into_iter().enumerate() {
            let index = count - 1 - newest;
            let snapshot = transition.detail.snapshot().to_owned();
            let extra = transition_extra(&transition.detail);
            let body = v_flex()
                .flex_1()
                .min_w_0()
                .gap_1()
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .text_xs()
                        .child(div().font_family("Menlo").text_color(secondary).child(clock(transition.timestamp)))
                        .child(div().font_family("Menlo").child(format!("{} → {}", transition.from.as_str(), transition.to.as_str())))
                        .child(pill(transition_kind(&transition.detail), rgb(0x555555).into()))
                        .when(!extra.is_empty(), |row| row.child(div().text_color(secondary).child(extra))),
                )
                .child(self.fold(Fold::Snapshot(index), snapshot.clone(), buffer_preview(&snapshot), rgb(0x666666).into(), cx));
            list = list.child(
                h_flex()
                    .p_2()
                    .gap_3()
                    .items_start()
                    .rounded_md()
                    .border_1()
                    .border_color(border)
                    .bg(card)
                    .child(body)
                    .child(self.debug_button(format!("debug-transition-{index}"), snapshot)),
            );
        }
        v_flex()
            .gap_3()
            .child(heading("Activity Transitions", Some(format!("{count} entries")), secondary))
            .child(list)
            .into_any_element()
    }

    fn calls(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let c = colors(cx);
        let (secondary, border, card, error) = (c.text_secondary, c.border, c.bg_primary, c.error);
        let calls = self.model.read(cx).calls(cx)?;
        let count = calls.len();
        let mut list = v_flex().gap_1();
        if calls.is_empty() {
            list = list.child(div().p_2().text_sm().text_color(secondary).child("No history entries yet."));
        }
        for (newest, entry) in calls.into_iter().enumerate() {
            let index = count - 1 - newest;
            let summary = summarize(&entry);
            let (kind, kind_color) = kind_pill(entry.kind);
            let mut body = v_flex().flex_1().min_w_0().gap_1().child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .text_xs()
                    .child(div().font_family("Menlo").text_color(secondary).child(clock(entry.timestamp)))
                    .child(pill(kind, rgb(kind_color).into()))
                    .child(div().font_family("Menlo").text_color(secondary).child(entry.model.clone()))
                    .when(summary.cached, |row| row.child(pill("cached", rgb(0x2e7d32).into())))
                    .when_some(summary.duration, |row, duration| {
                        row.child(pill(format!("{}ms", duration.as_millis()), rgb(0x555555).into()))
                    })
                    .when(summary.error.is_some(), |row| row.child(pill("error", rgb(0xf44747).into()))),
            );
            if let Some(message) = &summary.error {
                body = body.child(div().text_sm().text_color(error).child(message.clone()));
            }
            body = body.child(self.fold(
                Fold::Buffer(index),
                entry.buffer_text.clone(),
                buffer_preview(&entry.buffer_text),
                rgb(0x666666).into(),
                cx,
            ));
            if !summary.response.is_empty() {
                let collapsed = response_preview(&summary.response);
                body = body.child(div().p_1().rounded_sm().bg(rgb(0x1a1a1a)).child(self.fold(
                    Fold::Response(index),
                    summary.response.clone(),
                    collapsed,
                    rgb(0x23d18b).into(),
                    cx,
                )));
            }
            if let Some(prompt) = summary.system_prompt {
                body = body.child(div().p_1().rounded_sm().bg(rgb(0x1a1a1a)).child(self.fold(
                    Fold::Prompt(index),
                    prompt,
                    "system prompt (click to expand)".to_owned(),
                    rgb(0xb5cea8).into(),
                    cx,
                )));
            }
            list = list.child(
                h_flex()
                    .p_2()
                    .gap_3()
                    .items_start()
                    .rounded_md()
                    .border_1()
                    .border_color(border)
                    .bg(card)
                    .child(body)
                    .child(self.debug_button(format!("debug-call-{index}"), entry.buffer_text.clone())),
            );
        }
        Some(
            v_flex()
                .gap_3()
                .child(heading("Analyzer History", Some(format!("{count} entries")), secondary))
                .child(list)
                .into_any_element(),
        )
    }
}

impl Render for AnalyzerHistoryView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let error = colors(cx).error;
        let root = v_flex().id("analyzer-history").size_full().p_4().gap_6().overflow_y_scroll();
        if *self.model.read(cx).source() == HistorySource::Missing {
            return root.child(div().text_color(error).child("Invalid state: missing sourceTabId"));
        }
        let transitions = self.transitions(cx);
        let calls = self.calls(cx);
        root.child(transitions).children(calls)
    }
}
