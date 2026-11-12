//! Browser tabs: a local web page in a native webview (WKWebView / WebKitGTK via wry) under
//! the toolbar of `PageViews.tsx` (back, forward, reload / stop, home, the address bar, open
//! externally), plus the navigation guard of `src/main/webviewGuard.ts`.
//!
//! Without GPUI Fast composition the native view is drawn above GPUI content in its bounds,
//! so the owner hides it whenever its tab is not shown or a dialog is open
//! ([`BrowserView::set_shown`]).
//!
//! A page that is markdown source (`isMarkdownPage`) is rendered in its place, probed once
//! each load finishes; the webview stays hidden under it until a page that is not markdown
//! loads. Links in the rendered page load in the tab on its origin, elsewhere in the system
//! browser. WebKit cannot display `text/markdown` (Chromium shows it as text), so a page
//! the webview cannot display is reloaded through the tab's own URL scheme, which fetches
//! it and serves markdown as text ([`proxied_url`]); anything else opens in the system
//! browser.

use std::time::Duration;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    Animation, AnimationExt as _, AnyElement, Context, Entity, SharedString, Subscription, Task, WeakEntity, Window, div,
    px, relative, rgb,
};
use gpui_webview::WebView;
use treeterm_model::apps::{
    MarkdownLink, PAGE_PROBE_SCRIPT, PageContent, is_allowed_webview_url, is_markdown_page, markdown_link, page_content,
    page_path, proxied_url, resolve_address, unproxied_url,
};
use treeterm_model::workspace_store::WorkspaceStore;

use crate::theme::colors;

enum Page {
    /// Built on first render (it needs the window).
    NotBuilt,
    Live(Entity<WebView>),
    Failed(String),
}

/// What the webview reports (from its own callbacks, so through a channel).
enum PageEvent {
    Started(String),
    Finished(String),
    /// [`PAGE_PROBE_SCRIPT`]'s result.
    Probed(String),
    /// A page the webview cannot display (it wanted to download it).
    Undisplayable(String),
}

/// A back / forward the toolbar asked for: the next load moves along the history instead
/// of starting a new entry.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pending {
    None,
    Back,
    Forward,
}

/// The address bar while it is being edited (`Draft`): navigation must not clobber it.
enum Draft {
    Idle,
    Editing { error: Option<String> },
}

pub struct BrowserView {
    /// The start page (Home).
    url: String,
    page: Page,
    shown: bool,
    current: String,
    loading: bool,
    back: Vec<String>,
    forward: Vec<String>,
    pending: Pending,
    address: Option<Entity<InputState>>,
    draft: Draft,
    /// Where the page last shown is remembered (the tab's `path`), when set.
    memory: Option<(WeakEntity<WorkspaceStore>, String)>,
    /// The page as last probed; the previous one stays until the next load is probed.
    content: PageContent,
    markdown_scroll: gpui_kit::ScrollHandle,
    /// Where probe results go (the webview's callback runs off the main thread).
    probes: Option<tokio::sync::mpsc::UnboundedSender<PageEvent>>,
    /// This tab's URL scheme for pages the webview cannot display ([`proxied_url`]).
    proxy: String,
    _events: Option<Task<()>>,
    _address: Vec<Subscription>,
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
            // As a URL reads back (`http://localhost:8765/`); an address it cannot read stays as given.
            current: resolve_address(&url, &url).unwrap_or_else(|_| url.clone()),
            url,
            page: Page::NotBuilt,
            shown: true,
            loading: false,
            back: Vec::new(),
            forward: Vec::new(),
            pending: Pending::None,
            address: None,
            draft: Draft::Idle,
            memory: None,
            content: PageContent::Web,
            markdown_scroll: gpui_kit::ScrollHandle::new(),
            probes: None,
            proxy: {
                // One scheme per tab: WebKitGTK registers schemes per web context.
                static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
                format!("treeterm-page{}", NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
            },
            _events: None,
            _address: Vec::new(),
        }
    }

    /// Remember the page shown in the tab's state (`path`), as Electron's page memory does,
    /// so the tab reopens there. `home` is the start page Home goes back to.
    pub fn remember_pages_in(mut self, workspace: WeakEntity<WorkspaceStore>, tab_id: String, home: String) -> Self {
        self.memory = Some((workspace, tab_id));
        self.url = home;
        self
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    /// Show or hide the native view (tab switched away, dialog open).
    pub fn set_shown(&mut self, shown: bool, cx: &mut Context<Self>) {
        if self.shown == shown {
            return;
        }
        self.shown = shown;
        self.sync_visibility(cx);
    }

    /// The native view shows while the tab does, unless rendered markdown covers it.
    fn sync_visibility(&mut self, cx: &mut Context<Self>) {
        let visible = self.shown && self.content == PageContent::Web;
        if let Page::Live(webview) = &self.page {
            webview.update(cx, |w, _| if visible { w.show() } else { w.hide() });
        }
    }

    /// Ask the page what it is ([`PAGE_PROBE_SCRIPT`]).
    fn probe(&mut self, cx: &mut Context<Self>) {
        let Some(probes) = self.probes.clone() else {
            return;
        };
        self.webview(cx, |w| {
            let sent = w.raw().evaluate_script_with_callback(PAGE_PROBE_SCRIPT, move |result| {
                if probes.send(PageEvent::Probed(result)).is_err() {
                    tracing::debug!("browser tab closed while its page was probed");
                }
            });
            if let Err(error) = sent {
                tracing::error!(%error, "browser page probe");
            }
        });
    }

    /// A link clicked in rendered markdown.
    fn follow_markdown_link(&mut self, href: &str, cx: &mut Context<Self>) {
        match markdown_link(href, &self.current) {
            Ok(MarkdownLink::InTab(url)) => self.load(url, cx),
            Ok(MarkdownLink::External(url)) => open_externally(&url),
            Err(error) => tracing::error!(%error, "markdown link"),
        }
    }

    fn build(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !is_allowed_webview_url(&self.current) {
            self.page = Page::Failed(format!("Only localhost pages can be shown: {}", self.current));
            return;
        }
        let (events, mut updates) = tokio::sync::mpsc::unbounded_channel();
        self.probes = Some(events.clone());
        let undisplayable = events.clone();
        let (proxy, guard_proxy) = (self.proxy.clone(), self.proxy.clone());
        let builder = wry::WebViewBuilder::new()
            .with_url(self.current.clone())
            // In-page navigation stays on localhost; anything else goes to the system browser.
            .with_navigation_handler(move |url| {
                let allowed = is_allowed_webview_url(&url) || url == "about:blank" || unproxied_url(&guard_proxy, &url).is_some();
                if !allowed {
                    open_externally(&url);
                }
                allowed
            })
            .with_download_started_handler(move |url, _| {
                if undisplayable.send(PageEvent::Undisplayable(url)).is_err() {
                    tracing::debug!("browser tab closed while a page downloaded");
                }
                false
            })
            .with_asynchronous_custom_protocol(self.proxy.clone(), move |_, request, responder| {
                let page = unproxied_url(&proxy, &request.uri().to_string());
                std::thread::spawn(move || responder.respond(proxy_response(page)));
            })
            .with_new_window_req_handler(|url, _| {
                open_externally(&url);
                wry::NewWindowResponse::Deny
            })
            .with_on_page_load_handler(move |event, url| {
                let event = match event {
                    wry::PageLoadEvent::Started => PageEvent::Started(url),
                    wry::PageLoadEvent::Finished => PageEvent::Finished(url),
                };
                if events.send(event).is_err() {
                    tracing::debug!("browser tab closed while its page loaded");
                }
            });
        self.page = match WebView::build(builder, window, cx) {
            Ok(webview) => {
                self._events = Some(cx.spawn(async move |view, cx| {
                    while let Some(event) = updates.recv().await {
                        if view.update(cx, |view, cx| view.on_page(event, cx)).is_err() {
                            return;
                        }
                    }
                }));
                Page::Live(cx.new(|_| webview))
            }
            Err(error) => Page::Failed(format!("Cannot show the page: {error}")),
        };
    }

    /// Follow the page: history, loading state, the address shown and the remembered page.
    fn on_page(&mut self, event: PageEvent, cx: &mut Context<Self>) {
        // A proxied page is shown (and remembered) as the page it stands for.
        let shown = |url: String| unproxied_url(&self.proxy, &url).unwrap_or(url);
        let event = match event {
            PageEvent::Started(url) => PageEvent::Started(shown(url)),
            PageEvent::Finished(url) => PageEvent::Finished(shown(url)),
            other @ (PageEvent::Probed(_) | PageEvent::Undisplayable(_)) => other,
        };
        match event {
            PageEvent::Started(url) => {
                self.loading = true;
                match std::mem::replace(&mut self.pending, Pending::None) {
                    Pending::Back => {
                        self.back.pop();
                        self.forward.push(std::mem::replace(&mut self.current, url));
                    }
                    Pending::Forward => {
                        self.forward.pop();
                        self.back.push(std::mem::replace(&mut self.current, url));
                    }
                    Pending::None if url != self.current => {
                        self.back.push(std::mem::replace(&mut self.current, url));
                        self.forward.clear();
                    }
                    Pending::None => {}
                }
            }
            PageEvent::Finished(url) => {
                self.loading = false;
                self.current = url;
                self.probe(cx);
            }
            PageEvent::Undisplayable(url) => {
                match proxied_url(&self.proxy, &url) {
                    Some(proxied) if is_allowed_webview_url(&url) => self.load(proxied, cx),
                    Some(_) | None => tracing::error!(%url, "the browser tab cannot show this page"),
                }
                return;
            }
            PageEvent::Probed(result) => {
                self.content = match page_content(&result, &self.proxy) {
                    Ok(content) => content,
                    Err(error) => {
                        tracing::error!(%error, "browser page probe");
                        PageContent::Web
                    }
                };
                self.sync_visibility(cx);
                cx.notify();
                return;
            }
        }
        if let Some((workspace, tab)) = &self.memory
            && let Some(path) = page_path(&self.url, &self.current)
            && let Some(workspace) = workspace.upgrade()
        {
            workspace.update(cx, |w, cx| {
                w.update_tab_state(tab, |state| state["path"] = serde_json::Value::String(path), cx);
            });
        }
        cx.notify();
    }

    fn webview(&self, cx: &mut Context<Self>, f: impl FnOnce(&mut WebView)) {
        if let Page::Live(webview) = &self.page {
            webview.update(cx, |w, _| f(w));
        }
    }

    fn load(&mut self, url: String, cx: &mut Context<Self>) {
        self.webview(cx, |w| w.load_url(&url));
    }

    fn go(&mut self, pending: Pending, cx: &mut Context<Self>) {
        self.pending = pending;
        self.webview(cx, |w| {
            let result = match pending {
                Pending::Back => w.back(),
                Pending::Forward => w.forward(),
                Pending::None => Ok(()),
            };
            if let Err(error) = result {
                tracing::error!(%error, "browser history");
            }
        });
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        self.webview(cx, |w| {
            if let Err(error) = w.raw().reload() {
                tracing::error!(%error, "browser reload");
            }
        });
    }

    fn stop(&mut self, cx: &mut Context<Self>) {
        self.webview(cx, |w| {
            if let Err(error) = w.raw().evaluate_script("window.stop()") {
                tracing::error!(%error, "browser stop");
            }
        });
        self.loading = false;
        cx.notify();
    }

    /// The address bar, made on first render (it needs the window).
    fn address(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<InputState> {
        if let Some(address) = &self.address {
            return address.clone();
        }
        let current = self.current.clone();
        let address = cx.new(|cx| InputState::new(window, cx).default_value(current));
        self._address = vec![cx.subscribe_in(&address, window, |view, input, event: &InputEvent, window, cx| match event {
            InputEvent::Focus => input.update(cx, |input, cx| input.select_all(window, cx)),
            InputEvent::Change => {
                let error = None;
                if let Draft::Idle = view.draft {
                    view.draft = Draft::Editing { error };
                } else {
                    view.draft = Draft::Editing { error: None };
                }
                cx.notify();
            }
            InputEvent::PressEnter { .. } => {
                let typed = input.read(cx).value().to_string();
                match resolve_address(&typed, &view.current) {
                    Ok(target) => {
                        view.draft = Draft::Idle;
                        window.blur(cx);
                        view.load(target, cx);
                    }
                    Err(error) => view.draft = Draft::Editing { error: Some(error) },
                }
                cx.notify();
            }
            InputEvent::Blur => {
                if let Draft::Editing { error: None } = view.draft {
                    view.draft = Draft::Idle;
                    cx.notify();
                }
            }
        })];
        self.address = Some(address.clone());
        address
    }
}

/// What the tab's proxy scheme serves for `page` (the URL it stands for): markdown as
/// text; for anything else a note, and the page opens in the system browser. Runs on its
/// own thread (the fetch blocks it).
fn proxy_response(page: Option<String>) -> wry::http::Response<std::borrow::Cow<'static, [u8]>> {
    let text = |status: u16, body: String| {
        wry::http::Response::builder()
            .status(status)
            .header("Content-Type", "text/plain; charset=utf-8")
            .body(std::borrow::Cow::Owned(body.into_bytes()))
            .unwrap_or_else(|_| wry::http::Response::new(std::borrow::Cow::Borrowed(&[][..])))
    };
    let Some(page) = page.filter(|page| is_allowed_webview_url(page)) else {
        return text(404, "Only localhost pages can be shown".to_owned());
    };
    let fetched = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())
        .and_then(|rt| rt.block_on(treeterm_core::http::fetch_page(&page)).map_err(|error| error.to_string()));
    match fetched {
        Ok(fetched) if is_markdown_page(&fetched.content_type, &page) => {
            text(fetched.status, String::from_utf8_lossy(&fetched.body).into_owned())
        }
        Ok(fetched) => {
            open_externally(&page);
            text(200, format!("`{page}` ({}) cannot be shown here; it was opened in the system browser.", fetched.content_type))
        }
        Err(error) => text(502, format!("Cannot load `{page}`: {error}")),
    }
}

/// `.webapp-toolbar button`: a 14px glyph, 2px 8px padding, transparent until hovered;
/// 35% when disabled.
fn toolbar_button(id: &'static str, glyph: &'static str, title: &'static str, enabled: bool, cx: &gpui_kit::App) -> gpui_kit::Stateful<gpui_kit::Div> {
    let c = colors(cx);
    let hover = c.bg_hover;
    div()
        .id(id)
        .flex_shrink_0()
        .px(px(8.))
        .py(px(2.))
        .rounded(px(4.))
        .border_1()
        .border_color(gpui_kit::transparent_black())
        .text_size(px(14.))
        .line_height(px(18.))
        .text_color(c.text_primary)
        .child(glyph)
        .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(title).build(window, cx))
        .when(enabled, |d| d.cursor_pointer().hover(move |d| d.bg(hover)))
        .when(!enabled, |d| d.opacity(0.35))
}

impl Render for BrowserView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Page::NotBuilt = self.page {
            self.build(window, cx);
        }
        let address = self.address(window, cx);
        // While not editing, the bar shows the page.
        let editing = matches!(self.draft, Draft::Editing { .. });
        if !editing && address.read(cx).value() != self.current.as_str() {
            let current = self.current.clone();
            address.update(cx, |input, cx| input.set_value(current, window, cx));
        }
        let c = colors(cx);
        let (error, border, accent) = (c.error, c.border, c.accent);
        let focused = gpui_kit::Focusable::focus_handle(address.read(cx), cx).is_focused(window);
        let invalid = match &self.draft {
            Draft::Editing { error: Some(error) } => Some(error.clone()),
            Draft::Editing { error: None } | Draft::Idle => None,
        };
        let body: AnyElement = match &self.page {
            Page::NotBuilt => div().into_any_element(),
            Page::Failed(message) => div().p_4().text_color(error).child(message.clone()).into_any_element(),
            Page::Live(webview) => match &self.content {
                PageContent::Web => div().size_full().bg(gpui_kit::white()).child(webview.clone()).into_any_element(),
                // `.webapp-markdown`: the webview keeps its place (hidden) under the page.
                PageContent::Markdown(text) => {
                    let this = cx.weak_entity();
                    let preview = crate::file_browser_view::markdown_preview_linked(
                        SharedString::from(format!("page-markdown-{}", self.current)),
                        text.clone(),
                        &self.markdown_scroll,
                        move |href, _, cx| match this.update(cx, |view, cx| view.follow_markdown_link(href, cx)) {
                            Ok(()) | Err(_) => {} // Err: the tab closed
                        },
                        cx,
                    );
                    div().relative().size_full().child(webview.clone()).child(div().absolute().inset_0().child(preview)).into_any_element()
                }
            },
        };
        let (can_back, can_forward, loading) = (!self.back.is_empty(), !self.forward.is_empty(), self.loading);
        let tip: SharedString = invalid.clone().unwrap_or_else(|| address.read(cx).value().to_string()).into();
        let field = div()
            .id("webapp-address")
            .flex_1()
            .min_w_0()
            .mx(px(4.))
            .child(
                crate::ui::field(
                    &address,
                    crate::ui::FieldStyle {
                        bg: c.bg_primary,
                        border: if invalid.is_some() { rgb(0xf44336).into() } else { border },
                        radius: 14.,
                        pad: (4., 12.),
                        size: 12.,
                        mono: false,
                        height: 25.,
                    },
                    window,
                    cx,
                )
                .when(focused && invalid.is_none(), |d| d.border_color(accent)),
            )
            .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx));
        let mut toolbar = h_flex()
            .relative()
            .flex_shrink_0()
            .gap(px(4.))
            .px(px(8.))
            .py(px(4.))
            .border_b_1()
            .border_color(border)
            .bg(c.bg_secondary)
            .text_size(px(12.))
            .child(toolbar_button("page-back", "←", "Back", can_back, cx).when(can_back, |d| {
                d.on_click(cx.listener(|view, _, _, cx| view.go(Pending::Back, cx)))
            }))
            .child(toolbar_button("page-forward", "→", "Forward", can_forward, cx).when(can_forward, |d| {
                d.on_click(cx.listener(|view, _, _, cx| view.go(Pending::Forward, cx)))
            }));
        toolbar = if loading {
            toolbar.child(toolbar_button("page-stop", "✕", "Stop", true, cx).on_click(cx.listener(|view, _, _, cx| view.stop(cx))))
        } else {
            toolbar.child(toolbar_button("page-reload", "⟳", "Reload", true, cx).on_click(cx.listener(|view, _, _, cx| view.reload(cx))))
        };
        let home = self.url.clone();
        let external = self.current.clone();
        toolbar = toolbar
            .child(toolbar_button("page-home", "⌂", "Home", true, cx).on_click(cx.listener(move |view, _, _, cx| {
                view.load(home.clone(), cx);
            })))
            .child(field)
            .child(
                toolbar_button("page-external", "↗", "Open in external browser", true, cx)
                    .on_click(move |_, _, cx| cx.open_url(&external)),
            )
            // An indeterminate progress line under the toolbar while the page loads.
            .when(loading, |d| {
                d.child(
                    div().absolute().left_0().right_0().bottom(px(-1.)).h(px(2.)).overflow_hidden().child(
                        div().absolute().top_0().h_full().w(relative(0.3)).bg(accent).with_animation(
                            "page-progress",
                            Animation::new(Duration::from_secs(1)).repeat().with_easing(gpui_kit::ease_in_out),
                            |bar, delta| bar.left(relative(-0.3 + 1.3 * delta)),
                        ),
                    ),
                )
            });
        // Cmd/Ctrl+L focuses the address bar.
        let focus_address = address.clone();
        v_flex()
            .size_full()
            .on_key_down(move |event: &gpui_kit::KeyDownEvent, window, cx| {
                let keystroke = &event.keystroke;
                if (keystroke.modifiers.platform || keystroke.modifiers.control) && keystroke.key == "l" {
                    cx.stop_propagation();
                    focus_address.update(cx, |input, cx| input.focus(window, cx));
                }
            })
            .on_key_down(cx.listener(|view, event: &gpui_kit::KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" && matches!(view.draft, Draft::Editing { .. }) {
                    view.draft = Draft::Idle;
                    window.blur(cx);
                    cx.notify();
                }
            }))
            .child(toolbar)
            .child(div().flex_1().min_h_0().child(body))
    }
}


/// Files a page is built from: served to its webview from memory.
#[derive(Clone)]
enum Asset {
    Static(&'static [u8]),
    Shared(std::sync::Arc<Vec<u8>>),
}

/// The 3D model viewer page (`assets/model-viewer`; `viewer.js` is generated by its
/// `build.sh` from three.js and `viewer.src.js`).
const MODEL_VIEWER_HTML: &[u8] = include_bytes!("../assets/model-viewer/index.html");
const MODEL_VIEWER_JS: &[u8] = include_bytes!("../assets/model-viewer/viewer.js");

/// A page served from memory to a webview: a PDF (WKWebView renders PDFs itself) or the 3D
/// model viewer with the model's bytes. Each page gets its own URL scheme, so pages never
/// share a registration (webkitgtk registers schemes per web context).
pub struct AssetPageView {
    files: std::sync::Arc<std::collections::HashMap<String, (&'static str, Asset)>>,
    /// The path and query the page opens at.
    entry: String,
    /// What the page is, for the error shown when it cannot be built.
    what: &'static str,
    page: Page,
    shown: bool,
}

impl AssetPageView {
    pub fn pdf(bytes: std::sync::Arc<Vec<u8>>) -> Self {
        let files = [("document.pdf".to_owned(), ("application/pdf", Asset::Shared(bytes)))];
        Self::new(files.into_iter().collect(), "document.pdf".to_owned(), "the PDF")
    }

    /// `format` is the model's extension (`stl`, `glb`, …), as the viewer's parsers are keyed.
    pub fn model(bytes: std::sync::Arc<Vec<u8>>, format: &str) -> Self {
        let files = [
            ("index.html".to_owned(), ("text/html; charset=utf-8", Asset::Static(MODEL_VIEWER_HTML))),
            ("viewer.js".to_owned(), ("text/javascript; charset=utf-8", Asset::Static(MODEL_VIEWER_JS))),
            ("model".to_owned(), ("application/octet-stream", Asset::Shared(bytes))),
        ];
        Self::new(files.into_iter().collect(), format!("index.html?format={format}"), "the 3D model")
    }

    fn new(files: std::collections::HashMap<String, (&'static str, Asset)>, entry: String, what: &'static str) -> Self {
        Self {
            files: std::sync::Arc::new(files),
            entry,
            what,
            page: Page::NotBuilt,
            shown: true,
        }
    }

    /// Show or hide the native view (tab switched away, another file, dialog open).
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
        static NEXT_SCHEME: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let scheme = format!("treeterm-asset{}", NEXT_SCHEME.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
        let files = self.files.clone();
        let prefix = format!("{scheme}://");
        let builder = wry::WebViewBuilder::new()
            .with_custom_protocol(scheme.clone(), move |_, request| {
                let path = request.uri().path().trim_start_matches('/');
                let response = match files.get(path) {
                    Some((mime, asset)) => {
                        let body = match asset {
                            Asset::Static(bytes) => std::borrow::Cow::Borrowed(*bytes),
                            Asset::Shared(bytes) => std::borrow::Cow::Owned((**bytes).clone()),
                        };
                        wry::http::Response::builder().header("Content-Type", *mime).body(body)
                    }
                    None => wry::http::Response::builder().status(404).body(std::borrow::Cow::Borrowed(&[][..])),
                };
                response.unwrap_or_else(|_| wry::http::Response::new(std::borrow::Cow::Borrowed(&[][..])))
            })
            .with_url(format!("{scheme}://localhost/{}", self.entry))
            .with_on_page_load_handler({
                let what = self.what;
                move |event, _url| match event {
                    wry::PageLoadEvent::Finished => tracing::info!(page = what, "asset page loaded"),
                    wry::PageLoadEvent::Started => {}
                }
            })
            // The model viewer reports `ready …` / `error …`.
            .with_ipc_handler({
                let what = self.what;
                move |request| tracing::info!(page = what, message = %request.body(), "asset page")
            })
            .with_navigation_handler(move |url| url.starts_with(&prefix))
            .with_new_window_req_handler(|url, _| {
                open_externally(&url);
                wry::NewWindowResponse::Deny
            });
        self.page = match WebView::build(builder, window, cx) {
            Ok(webview) => Page::Live(cx.new(|_| webview)),
            Err(error) => Page::Failed(format!("Cannot show {}: {error}", self.what)),
        };
    }
}

impl Render for AssetPageView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Page::NotBuilt = self.page {
            self.build(window, cx);
        }
        match &self.page {
            Page::NotBuilt => div().into_any_element(),
            Page::Failed(message) => div().p_4().text_color(colors(cx).error).child(message.clone()).into_any_element(),
            Page::Live(webview) => div().size_full().child(webview.clone()).into_any_element(),
        }
    }
}
