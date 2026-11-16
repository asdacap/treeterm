//! Electron's "Application Error" screen (`AppErrorFallback`, styled by `.app-error-*`), shown
//! by the process the panic hook starts after a panic on the UI thread (`crash_report`). It
//! shows the panic's message and backtrace, as `AppErrorFallback` does for an `error`: the
//! panicked process is gone, so its log is the only other place they are. "Reload Application"
//! starts the app again; the daemon kept every PTY, so the session comes back.

use std::io;

use gpui_kit::prelude::*;
use gpui_kit::{
    AnyWindowHandle, App, Context, Entity, FontWeight, HighlightStyle, KeyBinding, Menu, MenuItem, StyledText,
    Window, WindowOptions, div, px,
};
use treeterm_core::crash_report::{self, CrashReport};

use crate::menus::Quit;
use crate::theme::colors;
use crate::ui;

/// `.app-error-message` and `.app-error-stack`: `'Monaco', 'Menlo', 'Ubuntu Mono', monospace`.
const MONO: &str = "Monaco";

/// Starts the app again ([`crash_report::relaunch`] with no arguments); a test's stand-in.
pub type Relaunch = Box<dyn Fn() -> io::Result<()>>;

/// Whether the stack trace is open (`showStack`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Stack {
    Hidden,
    Shown,
}

/// What became of "Reload Application".
enum Reload {
    NotPressed,
    /// The app could not be started again; this process stays to say why.
    Failed(String),
}

pub struct CrashView {
    report: CrashReport,
    stack: Stack,
    reload: Reload,
    relaunch: Relaunch,
}

/// The crash screen in a window opened with the app's `window` options, under a menu that
/// only quits. Closing the window quits.
pub fn open(report: CrashReport, window: WindowOptions, cx: &mut App) -> (AnyWindowHandle, Entity<CrashView>) {
    let quit = if cfg!(target_os = "macos") { "cmd-q" } else { "ctrl-q" };
    cx.bind_keys([KeyBinding::new(quit, Quit, None)]);
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.set_menus(vec![Menu::new("TreeTerm").items([MenuItem::action("Quit TreeTerm", Quit)])]);
    let relaunch: Relaunch = Box::new(|| crash_report::relaunch(&[]));
    let (handle, view) = gpui_kit::open_window(window, cx, |_, cx| cx.new(|_| CrashView::new(report, relaunch)))
        .expect("open the crash screen");
    cx.on_window_closed(|cx, _| {
        if cx.windows().is_empty() {
            cx.quit();
        }
    })
    .detach();
    (handle, view)
}

impl CrashView {
    pub fn new(report: CrashReport, relaunch: Relaunch) -> Self {
        Self { report, stack: Stack::Hidden, reload: Reload::NotPressed, relaunch }
    }

    fn toggle_stack(&mut self, cx: &mut Context<Self>) {
        self.stack = match self.stack {
            Stack::Hidden => Stack::Shown,
            Stack::Shown => Stack::Hidden,
        };
        cx.notify();
    }

    /// Electron reloaded the page (`location.reload()`), not reopening a directory given on
    /// the command line (`initialWorkspacePath` is read once): start the app with no
    /// arguments, then quit.
    fn reload(&mut self, cx: &mut Context<Self>) {
        match (self.relaunch)() {
            Ok(()) => cx.quit(),
            Err(error) => {
                tracing::error!(%error, "could not start the app again");
                self.reload = Reload::Failed(error.to_string());
                cx.notify();
            }
        }
    }

    #[cfg(feature = "automation")]
    pub fn automation_state(&self) -> serde_json::Value {
        let reload_error = match &self.reload {
            Reload::NotPressed => serde_json::Value::Null,
            Reload::Failed(error) => error.as_str().into(),
        };
        serde_json::json!({
            "screen": "crash",
            "message": self.report.message,
            "stackShown": self.stack == Stack::Shown,
            "reloadError": reload_error,
        })
    }

    /// `.app-error-details`: "Error: <message>", then the Show / Hide Stack Trace toggle
    /// over the stack while it is open.
    fn details(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let c = colors(cx);
        let (bg, bg_secondary, border, hover, primary, secondary) =
            (c.bg_primary, c.bg_secondary, c.border, c.bg_hover, c.text_primary, c.text_secondary);
        const LABEL: &str = "Error:";
        let bold = HighlightStyle { font_weight: Some(FontWeight::BOLD), ..Default::default() };
        let message = StyledText::new(format!("{LABEL} {}", self.report.message)).with_highlights([(0..LABEL.len(), bold)]);
        let toggle_label = match self.stack {
            Stack::Hidden => "Show Stack Trace",
            Stack::Shown => "Hide Stack Trace",
        };
        // `.app-error-stack-toggle`: transparent, 1px border, 6px 12px, 11px text.
        let toggle = ui::key_button("app-error-stack-toggle", 4., window, cx)
            .px(px(12.))
            .py(px(6.))
            .border_1()
            .border_color(border)
            .text_size(px(ui::SMALL))
            .line_height(ui::normal_line(ui::SMALL))
            .text_color(primary)
            .cursor_pointer()
            .hover(move |d| d.bg(hover))
            .debug_selector(|| "app-error-stack-toggle".into())
            .child(toggle_label)
            .on_click(cx.listener(|view, _, _, cx| view.toggle_stack(cx)));
        // `.app-error-stack`: a `pre` at most 300px tall that scrolls both ways.
        let stack = (self.stack == Stack::Shown).then(|| {
            div()
                .id("app-error-stack")
                .debug_selector(|| "app-error-stack".into())
                .mt(px(12.))
                .flex()
                // The text's own height, not the box's, so the box can scroll down to its end.
                .items_start()
                .max_h(px(300.))
                .overflow_scroll()
                .p(px(12.))
                .bg(bg)
                .border_1()
                .border_color(border)
                .rounded(px(4.))
                .font_family(MONO)
                .text_size(px(11.))
                .line_height(ui::normal_line(11.))
                .text_color(secondary)
                .child(div().flex_shrink_0().whitespace_nowrap().child(self.report.stack.clone()))
        });
        div()
            .flex()
            .flex_col()
            .mb(px(24.))
            .p(px(16.))
            .bg(bg_secondary)
            .border_1()
            .border_color(border)
            .rounded(px(4.))
            .text_left()
            .child(
                div()
                    .font_family(MONO)
                    .text_size(px(12.))
                    .line_height(ui::normal_line(12.))
                    .text_color(primary)
                    .child(message),
            )
            // `.app-error-stack-container`, 12px below the message.
            .child(div().mt(px(12.)).flex().flex_col().child(div().flex().child(toggle)).children(stack))
    }
}

impl Render for CrashView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let details = self.details(window, cx);
        let c = colors(cx);
        let (bg, primary, secondary, accent, red) = (c.bg_primary, c.text_primary, c.text_secondary, c.accent, c.error);
        // `.app-error-reload-btn`: accent, white 14px text, 10px 24px, 90% opaque on hover.
        let reload = ui::key_button("app-error-reload", 4., window, cx)
            .px(px(24.))
            .py(px(10.))
            .bg(accent)
            .text_size(px(14.))
            .line_height(ui::normal_line(14.))
            .text_color(gpui_kit::white())
            .cursor_pointer()
            .hover(|d| d.opacity(0.9))
            .debug_selector(|| "app-error-reload".into())
            .child("Reload Application")
            .on_click(cx.listener(|view, _, _, cx| view.reload(cx)));
        let failed = match &self.reload {
            Reload::NotPressed => None,
            Reload::Failed(error) => Some(
                div()
                    .mt(px(12.))
                    .text_size(px(ui::BODY))
                    .text_color(red)
                    .debug_selector(|| "app-error-reload-failed".into())
                    .child(format!("Could not reload: {error}")),
            ),
        };
        // `.app-error-fallback`: the whole window, its content centred (`.app-error-content`:
        // at most 600px wide, centred text).
        div().size_full().flex().items_center().justify_center().p(px(20.)).bg(bg).text_color(primary).child(
            div()
                .flex()
                .flex_col()
                .max_w(px(600.))
                .text_center()
                .child(
                    div()
                        .mb(px(16.))
                        .text_size(px(24.))
                        .line_height(ui::normal_line(24.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(red)
                        .child("Application Error"),
                )
                .child(
                    div()
                        .mb(px(24.))
                        .text_size(px(16.))
                        .line_height(px(24.))
                        .text_color(secondary)
                        .child("The application encountered an unexpected error and needs to reload."),
                )
                .child(details)
                .child(div().flex().justify_center().child(reload))
                .children(failed),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use gpui_kit::{Modifiers, TestAppContext, VisualTestContext};

    use super::*;

    fn report() -> CrashReport {
        CrashReport { message: "boom".into(), stack: "panicked at src/main.rs:1:1:\n   0: treeterm::main".into() }
    }

    /// The crash screen, starting the app again with `relaunch`.
    fn show(relaunch: Relaunch, cx: &mut TestAppContext) -> (Entity<CrashView>, &mut VisualTestContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::theme::install(cx);
        });
        let (view, cx) = cx.add_window_view(|_, _| CrashView::new(report(), relaunch));
        draw(cx);
        (view, cx)
    }

    fn draw(cx: &mut VisualTestContext) {
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }

    fn click(selector: &'static str, cx: &mut VisualTestContext) {
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::none());
        draw(cx);
    }

    /// The stack shows only once Show Stack Trace is pressed, and hides again (`showStack`).
    #[gpui_kit::test]
    fn the_toggle_shows_and_hides_the_stack(cx: &mut TestAppContext) {
        let (view, cx) = show(Box::new(|| Ok(())), cx);
        assert!(cx.debug_bounds("app-error-stack").is_none());
        click("app-error-stack-toggle", cx);
        assert!(cx.debug_bounds("app-error-stack").is_some());
        assert!(cx.update(|_, cx| view.read(cx).stack == Stack::Shown));
        click("app-error-stack-toggle", cx);
        assert!(cx.debug_bounds("app-error-stack").is_none());
    }

    /// Reload Application starts the app again (and quits).
    #[gpui_kit::test]
    fn reload_starts_the_app_again(cx: &mut TestAppContext) {
        let started = Rc::new(Cell::new(0));
        let (_, cx) = show(
            Box::new({
                let started = started.clone();
                move || {
                    started.set(started.get() + 1);
                    Ok(())
                }
            }),
            cx,
        );
        click("app-error-reload", cx);
        assert_eq!(started.get(), 1);
        assert!(cx.debug_bounds("app-error-reload-failed").is_none());
    }

    /// When the app cannot be started again, the screen stays and says why.
    #[gpui_kit::test]
    fn a_failed_reload_says_why(cx: &mut TestAppContext) {
        let (view, cx) = show(Box::new(|| Err(io::Error::other("no such program"))), cx);
        click("app-error-reload", cx);
        assert!(cx.debug_bounds("app-error-reload-failed").is_some());
        let failed = cx.update(|_, cx| match &view.read(cx).reload {
            Reload::Failed(error) => error.clone(),
            Reload::NotPressed => panic!("the reload failed"),
        });
        assert_eq!(failed, "no such program");
    }
}
