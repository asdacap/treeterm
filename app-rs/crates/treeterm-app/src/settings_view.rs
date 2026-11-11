//! The Settings dialog. Port of the Terminal and Keyboard parts of `SettingsDialog.tsx`;
//! the other sections (AI harness, runners, web apps, SSH, LLM, analyzer, ...) follow.
//!
//! Edits are local until "Save", which validates them and writes `settings.json` through
//! the settings store (other windows and the Electron app pick the change up from there).

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{App, Context, Entity, SharedString, Window, div, px};
use treeterm_core::keybinding::{Key, Os, Shortcut};
use treeterm_core::settings::CursorStyle;
use treeterm_model::settings_store::SettingsStore;

use crate::theme::colors;

pub struct SettingsView {
    store: Entity<SettingsStore>,
    font_family: Entity<InputState>,
    font_size: Entity<InputState>,
    max_cols: Entity<InputState>,
    cursor_style: CursorStyle,
    cursor_blink: bool,
    allow_osc52: bool,
    prefix_key: Entity<InputState>,
    prefix_timeout: Entity<InputState>,
    /// newTab, closeTab, nextTab, prevTab, openSettings, workspaceFocus.
    action_keys: [Entity<InputState>; 6],
    error: Option<SharedString>,
}

const ACTION_LABELS: [&str; 6] = [
    "New tab",
    "Close tab",
    "Next tab",
    "Previous tab",
    "Open settings",
    "Workspace focus",
];

impl SettingsView {
    pub fn new(store: Entity<SettingsStore>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let settings = store.read(cx).settings().clone();
        let t = &settings.terminal;
        let k = &settings.keybindings;
        let mut field = |value: String| cx.new(|cx| InputState::new(window, cx).default_value(value));
        let action_keys = [
            field(k.new_tab.clone()),
            field(k.close_tab.clone()),
            field(k.next_tab.clone()),
            field(k.prev_tab.clone()),
            field(k.open_settings.clone()),
            field(k.workspace_focus.clone()),
        ];
        Self {
            font_family: field(t.font_family.clone()),
            font_size: field(t.font_size.to_string()),
            max_cols: field(t.max_cols.to_string()),
            prefix_key: field(settings.prefix_mode.prefix_key.clone()),
            prefix_timeout: field(settings.prefix_mode.timeout.as_millis().to_string()),
            action_keys,
            cursor_style: t.cursor_style,
            cursor_blink: t.cursor_blink,
            allow_osc52: t.allow_osc52_clipboard,
            error: None,
            store,
        }
    }

    /// Validate and save. Returns whether the dialog may close.
    pub fn save(&mut self, cx: &mut Context<Self>) -> bool {
        let text = |state: &Entity<InputState>, cx: &App| state.read(cx).value().trim().to_owned();
        let font_family = text(&self.font_family, cx);
        let font_size = text(&self.font_size, cx).parse::<f32>().ok().filter(|s| s.is_finite() && *s > 0.);
        let max_cols = text(&self.max_cols, cx).parse::<u16>().ok().filter(|c| *c >= 2);
        let prefix_key = text(&self.prefix_key, cx);
        let timeout = text(&self.prefix_timeout, cx).parse::<u64>().ok().filter(|ms| *ms > 0);
        let keys: Vec<String> = self.action_keys.iter().map(|k| text(k, cx)).collect();
        let fail = |this: &mut Self, message: String, cx: &mut Context<Self>| {
            this.error = Some(message.into());
            cx.notify();
            false
        };
        let Some(font_size) = font_size else {
            return fail(self, "Font size must be a positive number".into(), cx);
        };
        let Some(max_cols) = max_cols else {
            return fail(self, "Column limit must be a whole number of at least 2".into(), cx);
        };
        let Some(timeout) = timeout else {
            return fail(self, "Prefix timeout must be a positive number of milliseconds".into(), cx);
        };
        if let Err(error) = Shortcut::parse(&prefix_key, Os::host()) {
            return fail(self, format!("Prefix key “{prefix_key}”: {error}"), cx);
        }
        for (label, key) in ACTION_LABELS.iter().zip(&keys) {
            if let Err(error) = Key::parse(key) {
                return fail(self, format!("{label} key “{key}”: {error}"), cx);
            }
        }
        let (cursor_style, cursor_blink, allow_osc52) = (self.cursor_style, self.cursor_blink, self.allow_osc52);
        self.store.update(cx, |store, cx| {
            store.update(
                |s| {
                    s.terminal.font_family = font_family;
                    s.terminal.font_size = font_size;
                    s.terminal.max_cols = max_cols;
                    s.terminal.cursor_style = cursor_style;
                    s.terminal.cursor_blink = cursor_blink;
                    s.terminal.allow_osc52_clipboard = allow_osc52;
                    s.prefix_mode.prefix_key = prefix_key;
                    s.prefix_mode.timeout = std::time::Duration::from_millis(timeout);
                    let [new_tab, close_tab, next_tab, prev_tab, open_settings, workspace_focus] =
                        <[String; 6]>::try_from(keys).expect("six action keys");
                    s.keybindings.new_tab = new_tab;
                    s.keybindings.close_tab = close_tab;
                    s.keybindings.next_tab = next_tab;
                    s.keybindings.prev_tab = prev_tab;
                    s.keybindings.open_settings = open_settings;
                    s.keybindings.workspace_focus = workspace_focus;
                },
                cx,
            )
        });
        let saved_error = self.store.read(cx).error().map(str::to_owned);
        match saved_error {
            Some(error) => fail(self, format!("Could not save settings: {error}"), cx),
            None => true,
        }
    }
}

fn row(label: &'static str, control: impl IntoElement) -> impl IntoElement {
    h_flex()
        .gap_3()
        .items_center()
        .child(div().w(px(150.)).text_sm().child(label))
        .child(div().flex_1().child(control))
}

fn section(title: &'static str, cx: &App) -> impl IntoElement {
    div()
        .pt_2()
        .pb_1()
        .text_xs()
        .text_color(colors(cx).text_secondary)
        .child(title.to_uppercase())
}

impl Render for SettingsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let error = colors(cx).error;
        let cursor = |style: CursorStyle, label: &'static str, cx: &mut Context<Self>| {
            let selected = self.cursor_style == style;
            let button = Button::new(label).label(label).small();
            let button = if selected { button.primary() } else { button.ghost() };
            button.on_click(cx.listener(move |view, _, _, cx| {
                view.cursor_style = style;
                cx.notify();
            }))
        };
        let mut keys = v_flex().gap_2();
        for (label, state) in ACTION_LABELS.iter().zip(&self.action_keys) {
            keys = keys.child(row(label, Input::new(state).small()));
        }
        v_flex()
            .gap_2()
            .child(section("Terminal", cx))
            .child(row("Font family", Input::new(&self.font_family).small()))
            .child(row("Font size", Input::new(&self.font_size).small()))
            .child(row(
                "Cursor",
                h_flex()
                    .gap_1()
                    .child(cursor(CursorStyle::Block, "Block", cx))
                    .child(cursor(CursorStyle::Underline, "Underline", cx))
                    .child(cursor(CursorStyle::Bar, "Bar", cx)),
            ))
            .child(row(
                "Cursor blink",
                Switch::new("cursor-blink").checked(self.cursor_blink).small().on_click(cx.listener(
                    |view, checked: &bool, _, cx| {
                        view.cursor_blink = *checked;
                        cx.notify();
                    },
                )),
            ))
            .child(row(
                "OSC 52 clipboard",
                Switch::new("osc52").checked(self.allow_osc52).small().on_click(cx.listener(
                    |view, checked: &bool, _, cx| {
                        view.allow_osc52 = *checked;
                        cx.notify();
                    },
                )),
            ))
            .child(row("Column limit", Input::new(&self.max_cols).small()))
            .child(section("Prefix mode", cx))
            .child(row("Prefix key", Input::new(&self.prefix_key).small()))
            .child(row("Timeout (ms)", Input::new(&self.prefix_timeout).small()))
            .child(section("Keys after the prefix", cx))
            .child(keys)
            .when_some(self.error.clone(), |d, message| {
                d.child(div().pt_2().text_sm().text_color(error).child(message))
            })
    }
}

/// Open the dialog in `window`.
pub fn open(store: Entity<SettingsStore>, window: &mut Window, cx: &mut App) {
    let view = cx.new(|cx| SettingsView::new(store, window, cx));
    window.open_dialog(cx, move |dialog, _window, _cx| {
        let save = view.clone();
        dialog
            .title("Settings")
            .w(px(560.))
            .child(view.clone())
            .footer(
                h_flex().justify_end().gap_2().child(Button::new("save-settings").primary().label("Save").on_click(
                    move |_, window, cx| {
                        if save.update(cx, |view, cx| view.save(cx)) {
                            window.close_dialog(cx);
                        }
                    },
                )),
            )
    });
}
