//! Browser tabs: a local web page in a native webview (WKWebView / WebKitGTK via wry).
//! Port of `PageViews.tsx` + the navigation guard of `src/main/webviewGuard.ts`.
//!
//! Without GPUI Fast composition the native view is drawn above GPUI content in its bounds,
//! so the owner hides it whenever its tab is not shown or a dialog is open
//! ([`BrowserView::set_shown`]).

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, Context, Entity, SharedString, Window, div};
use gpui_webview::WebView;
use treeterm_model::apps::is_allowed_webview_url;

use crate::theme::colors;

enum Page {
    /// Built on first render (it needs the window).
    NotBuilt,
    Live(Entity<WebView>),
    Failed(String),
}

pub struct BrowserView {
    url: String,
    page: Page,
    shown: bool,
}

/// Open a URL in the system browser. Runs on a webview thread, so no GPUI context.
fn open_externally(url: &str) {
    let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    if let Err(error) = std::process::Command::new(opener).arg(url).spawn() {
        tracing::error!(%error, %url, "cannot open URL in the system browser");
    }
}

impl BrowserView {
    pub fn new(url: String) -> Self {
        Self {
            url,
            page: Page::NotBuilt,
            shown: true,
        }
    }

    /// Show or hide the native view (tab switched away, dialog open).
    pub fn set_shown(&mut self, shown: bool, cx: &mut Context<Self>) {
        if self.shown == shown {
            return;
        }
        self.shown = shown;
        if let Page::Live(webview) = &self.page {
            webview.update(cx, |w, _| if shown { w.show() } else { w.hide() });
        }
    }

    fn build(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !is_allowed_webview_url(&self.url) {
            self.page = Page::Failed(format!("Only localhost pages can be shown: {}", self.url));
            return;
        }
        let builder = wry::WebViewBuilder::new()
            .with_url(self.url.clone())
            // In-page navigation stays on localhost; anything else goes to the system browser.
            .with_navigation_handler(|url| {
                let allowed = is_allowed_webview_url(&url) || url == "about:blank";
                if !allowed {
                    open_externally(&url);
                }
                allowed
            })
            .with_new_window_req_handler(|url, _| {
                open_externally(&url);
                wry::NewWindowResponse::Deny
            });
        self.page = match WebView::build(builder, window, cx) {
            Ok(webview) => Page::Live(cx.new(|_| webview)),
            Err(error) => Page::Failed(format!("Cannot show the page: {error}")),
        };
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        let url = self.url.clone();
        if let Page::Live(webview) = &self.page {
            webview.update(cx, |w, _| w.load_url(&url));
        }
    }
}

impl Render for BrowserView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Page::NotBuilt = self.page {
            self.build(window, cx);
        }
        let c = colors(cx);
        let (secondary, error, border) = (c.text_secondary, c.error, c.border);
        let url = self.url.clone();
        let body: AnyElement = match &self.page {
            Page::NotBuilt => div().into_any_element(),
            Page::Failed(message) => div().p_4().text_color(error).child(message.clone()).into_any_element(),
            Page::Live(webview) => div().size_full().child(webview.clone()).into_any_element(),
        };
        v_flex()
            .size_full()
            .child(
                h_flex()
                    .px_2()
                    .py_1()
                    .gap_2()
                    .items_center()
                    .border_b_1()
                    .border_color(border)
                    .child(div().flex_1().truncate().text_xs().text_color(secondary).child(SharedString::from(url.clone())))
                    .child(Button::new("page-reload").xsmall().ghost().label("Reload").on_click(cx.listener(
                        |view, _, _, cx| view.reload(cx),
                    )))
                    .child(Button::new("page-external").xsmall().ghost().label("Open in browser").on_click(
                        move |_, _, cx| cx.open_url(&url),
                    )),
            )
            .child(div().flex_1().min_h_0().child(body))
    }
}
