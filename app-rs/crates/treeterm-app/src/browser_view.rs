//! Browser tabs: a web page in a native webview (WKWebView / WebKitGTK via wry) under the
//! toolbar of `PageViews.tsx` (back, forward, reload / stop, home, the address bar, open
//! externally). As in Electron, whose guard (`src/main/webviewGuard.ts`) checks only the
//! page the webview opens on and its popups, the start page must be on localhost but the
//! page may then go anywhere ([`tab_navigation`]); popups open in the system browser.
//!
//! The toolbar follows the webview's own navigation state ([`webview_native`], [`PageNav`]):
//! its history, a load from its request on, navigations within the page. A load that ends
//! without a page (server down) shows a message in the page's place, where Chromium shows its
//! error page and WKWebView nothing.
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
//! it and serves markdown as text ([`ProxiedPages`]); anything else opens in the system
//! browser.

use std::cell::RefCell;
use std::rc::Rc;
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
    MarkdownLink, PAGE_PROBE_SCRIPT, PageContent, ProxiedPages, TabNavigation, is_allowed_webview_url,
    is_markdown_page, markdown_link, page_content, resolve_address, tab_navigation,
};
use treeterm_model::page_nav::{NavInput, PageNav, PageState, Reload};
use treeterm_model::workspace_store::WorkspaceStore;

use crate::theme::colors;
use crate::webview_native;

enum Page {
    /// Built on first render (it needs the window).
    NotBuilt,
    Live(Entity<WebView>),
    Failed(String),
}

/// What the webview reports (from its own callbacks, so through a channel), and what the tab
/// asks of it, in the order it happened.
enum PageEvent {
    Nav(NavInput),
    /// [`PAGE_PROBE_SCRIPT`]'s result.
    Probed(String),
    /// A page the webview cannot display (it wanted to download it).
    Undisplayable(String),
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
    nav: PageNav,
    address: Option<Entity<InputState>>,
    draft: Draft,
    /// The tab whose state remembers the page shown (`path`): its workspace and id.
    memory: (WeakEntity<WorkspaceStore>, String),
    /// The page as last probed; the previous one stays until the next load is probed.
    content: PageContent,
    markdown_scroll: gpui_kit::ScrollHandle,
    /// Where the webview's reports go (its callbacks cannot reach the view).
    events: Option<tokio::sync::mpsc::UnboundedSender<PageEvent>>,
    /// This tab's URL scheme for pages the webview cannot display, and the pages sent there.
    proxy: String,
    proxied: Rc<RefCell<ProxiedPages>>,
    _observer: Option<webview_native::NavObserver>,
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
    /// A page opening `url`, under Home going to `home`. Each page it shows is remembered in
    /// the state of the tab `tab_id` (Electron's page memory), so the tab reopens there.
    pub fn new(url: String, home: String, workspace: WeakEntity<WorkspaceStore>, tab_id: String) -> Self {
        // One scheme per tab: WebKitGTK registers schemes per web context.
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let proxy = format!("treeterm-page{}", NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
        Self {
            // As a URL reads back (`http://localhost:8765/`); an address it cannot read stays as given.
            nav: PageNav::new(resolve_address(&url, &url).unwrap_or(url), proxy.clone()),
            url: home,
            page: Page::NotBuilt,
            shown: true,
            address: None,
            draft: Draft::Idle,
            memory: (workspace, tab_id),
            content: PageContent::Web,
            markdown_scroll: gpui_kit::ScrollHandle::new(),
            events: None,
            proxy,
            proxied: Rc::new(RefCell::new(ProxiedPages::default())),
            _observer: None,
            _events: None,
            _address: Vec::new(),
        }
    }

    /// Show or hide the native view (tab switched away, dialog open).
    pub fn set_shown(&mut self, shown: bool, cx: &mut Context<Self>) {
        if self.shown == shown {
            return;
        }
        self.shown = shown;
        self.sync_visibility(cx);
    }

    /// The native view shows while the tab does, unless rendered markdown or a failed load
    /// covers it.
    fn sync_visibility(&mut self, cx: &mut Context<Self>) {
        let visible = self.shown && self.content == PageContent::Web && *self.nav.page() == PageState::Shown;
        if let Page::Live(webview) = &self.page {
            webview.update(cx, |w, _| if visible { w.show() } else { w.hide() });
        }
    }

    /// Ask the page what it is ([`PAGE_PROBE_SCRIPT`]).
    fn probe(&mut self, cx: &mut Context<Self>) {
        let Some(events) = self.events.clone() else {
            return;
        };
        self.webview(cx, |w| {
            let sent = w.raw().evaluate_script_with_callback(PAGE_PROBE_SCRIPT, move |result| {
                if events.send(PageEvent::Probed(result)).is_err() {
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
        match markdown_link(href, self.nav.current()) {
            Ok(MarkdownLink::InTab(url)) => self.load(url, cx),
            Ok(MarkdownLink::External(url)) => open_externally(&url),
            Err(error) => tracing::error!(%error, "markdown link"),
        }
    }

    fn build(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !is_allowed_webview_url(self.nav.current()) {
            self.page = Page::Failed(format!("Only localhost pages can be shown: {}", self.nav.current()));
            return;
        }
        let (events, mut updates) = tokio::sync::mpsc::unbounded_channel();
        self.events = Some(events.clone());
        let (undisplayable, observed) = (events.clone(), events.clone());
        let (proxy, guard_proxy, proxied) = (self.proxy.clone(), self.proxy.clone(), self.proxied.clone());
        let builder = wry::WebViewBuilder::new()
            .with_url(self.nav.current())
            .with_navigation_handler(move |url| match tab_navigation(&url, &guard_proxy) {
                TabNavigation::InTab => true,
                TabNavigation::External => {
                    open_externally(&url);
                    false
                }
                TabNavigation::Refused => {
                    tracing::warn!(%url, "a browser tab refused to go to a local file or an unreadable URL");
                    false
                }
            })
            .with_download_started_handler(move |url, _| {
                if undisplayable.send(PageEvent::Undisplayable(url)).is_err() {
                    tracing::debug!("browser tab closed while a page downloaded");
                }
                false
            })
            .with_asynchronous_custom_protocol(self.proxy.clone(), move |_, request, responder| {
                let page = proxied.borrow().page(&proxy, &request.uri().to_string());
                std::thread::spawn(move || responder.respond(proxy_response(page)));
            })
            .with_new_window_req_handler(|url, _| {
                open_externally(&url);
                wry::NewWindowResponse::Deny
            })
            // A load's end is the webview's own loading state ([`webview_native`]).
            .with_on_page_load_handler(move |event, url| match event {
                wry::PageLoadEvent::Started => {
                    if events.send(PageEvent::Nav(NavInput::Committed(url))).is_err() {
                        tracing::debug!("browser tab closed while its page loaded");
                    }
                }
                wry::PageLoadEvent::Finished => {}
            });
        self.page = match WebView::build(builder, window, cx) {
            Ok(webview) => {
                self._observer = Some(webview_native::observe(webview.raw(), move |native| {
                    if observed.send(PageEvent::Nav(NavInput::Native(native))).is_err() {
                        tracing::debug!("browser tab closed while its page navigated");
                    }
                }));
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

    /// Follow the page: history, loading state, the address shown, the remembered page and
    /// what the page is.
    fn on_page(&mut self, event: PageEvent, cx: &mut Context<Self>) {
        match event {
            PageEvent::Nav(input) => {
                tracing::debug!(?input, "browser tab navigation");
                let effects = self.nav.apply(input);
                // No workspace: the tab went with it, so there is nothing to remember the page in.
                if effects.navigated
                    && let Some(workspace) = self.memory.0.upgrade()
                {
                    workspace.update(cx, |w, cx| w.remember_page(&self.memory.1, &self.url, self.nav.current(), cx));
                }
                if effects.load_ended {
                    self.probe(cx);
                }
            }
            PageEvent::Undisplayable(url) => {
                self.nav.apply(NavInput::Downloaded);
                let proxied = if is_allowed_webview_url(&url) { self.proxied.borrow_mut().proxy(&self.proxy, &url) } else { None };
                match proxied {
                    Some(proxied) => self.load(proxied, cx),
                    None => tracing::error!(%url, "the browser tab cannot show this page"),
                }
            }
            // A probe that fails keeps the page's content, as Electron only logs it.
            PageEvent::Probed(result) => match page_content(&result, &self.proxy) {
                Ok(content) => self.content = content,
                Err(error) => tracing::error!(%error, "browser page probe"),
            },
        }
        self.sync_visibility(cx);
        cx.notify();
    }

    fn webview(&self, cx: &mut Context<Self>, f: impl FnOnce(&mut WebView)) {
        if let Page::Live(webview) = &self.page {
            webview.update(cx, |w, _| f(w));
        }
    }

    /// Tell the page's navigation what the tab asks of the webview, before asking: the
    /// webview reports what it does from within the call, and those reports come after.
    fn send(&self, input: NavInput) {
        if let Some(events) = &self.events
            && events.send(PageEvent::Nav(input)).is_err()
        {
            tracing::debug!("browser tab closed while it navigated");
        }
    }

    fn load(&mut self, url: String, cx: &mut Context<Self>) {
        self.send(NavInput::Requested(url.clone()));
        self.webview(cx, |w| {
            if let Err(error) = w.raw().load_url(&url) {
                tracing::error!(%error, %url, "browser load");
            }
        });
    }

    fn back(&mut self, cx: &mut Context<Self>) {
        self.webview(cx, |w| webview_native::go_back(w.raw()));
    }

    fn forward(&mut self, cx: &mut Context<Self>) {
        self.webview(cx, |w| webview_native::go_forward(w.raw()));
    }

    /// `reloadIgnoringCache`; a page that could not be loaded is loaded again.
    fn reload(&mut self, cx: &mut Context<Self>) {
        match self.nav.reload() {
            Reload::FromOrigin => {
                self.send(NavInput::Reloading);
                self.webview(cx, |w| webview_native::reload_from_origin(w.raw()));
            }
            Reload::Retry(url) => self.load(url, cx),
        }
    }

    fn stop(&mut self, cx: &mut Context<Self>) {
        self.send(NavInput::Stopped);
        self.webview(cx, |w| webview_native::stop_loading(w.raw()));
    }

    /// The address bar, made on first render (it needs the window).
    fn address(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<InputState> {
        if let Some(address) = &self.address {
            return address.clone();
        }
        let current = self.nav.current().to_owned();
        let address = cx.new(|cx| InputState::new(window, cx).default_value(current));
        self._address = vec![cx.subscribe_in(&address, window, |view, input, event: &InputEvent, window, cx| match event {
            InputEvent::Focus => input.update(cx, |input, cx| input.select_all(window, cx)),
            InputEvent::Change => {
                view.draft = Draft::Editing { error: None };
                cx.notify();
            }
            InputEvent::PressEnter { .. } => {
                let typed = input.read(cx).value().to_string();
                match resolve_address(&typed, view.nav.current()) {
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

/// A load that ended before any page came, in Chromium's error page's place (WKWebView
/// shows nothing; the reason is not known).
fn load_failure(url: &str, cx: &gpui_kit::App) -> impl IntoElement {
    let c = colors(cx);
    div().size_full().bg(c.bg_primary).child(
        v_flex()
            .max_w(px(600.))
            .mx_auto()
            .mt(px(80.))
            .px(px(24.))
            .gap(px(12.))
            .child(div().text_size(px(24.)).text_color(c.text_primary).child("This site can’t be reached"))
            .child(div().text_size(px(14.)).text_color(c.text_secondary).child(format!("Could not load {url}"))),
    )
}

impl Render for BrowserView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Page::NotBuilt = self.page {
            self.build(window, cx);
        }
        let address = self.address(window, cx);
        // While not editing, the bar shows the page.
        let editing = matches!(self.draft, Draft::Editing { .. });
        if !editing && address.read(cx).value() != self.nav.current() {
            let current = self.nav.current().to_owned();
            address.update(cx, |input, cx| input.set_value(current, window, cx));
        }
        let c = colors(cx);
        let (error, border, accent, bg_primary, bg_secondary) = (c.error, c.border, c.accent, c.bg_primary, c.bg_secondary);
        let focused = gpui_kit::Focusable::focus_handle(address.read(cx), cx).is_focused(window);
        let invalid = match &self.draft {
            Draft::Editing { error: Some(error) } => Some(error.clone()),
            Draft::Editing { error: None } | Draft::Idle => None,
        };
        let body: AnyElement = match &self.page {
            Page::NotBuilt => div().into_any_element(),
            Page::Failed(message) => div().p_4().text_color(error).child(message.clone()).into_any_element(),
            Page::Live(webview) => match (self.nav.page(), &self.content) {
                // The webview keeps its place (hidden) under the message.
                (PageState::Failed { url }, _) => div()
                    .relative()
                    .size_full()
                    .child(webview.clone())
                    .child(div().absolute().inset_0().child(load_failure(url, cx)))
                    .into_any_element(),
                (PageState::Shown, PageContent::Web) => {
                    div().size_full().bg(gpui_kit::white()).child(webview.clone()).into_any_element()
                }
                // `.webapp-markdown`: the webview keeps its place (hidden) under the page.
                (PageState::Shown, PageContent::Markdown(text)) => {
                    let this = cx.weak_entity();
                    let preview = crate::file_browser_view::markdown_preview_linked(
                        SharedString::from(format!("page-markdown-{}", self.nav.current())),
                        text.clone(),
                        &self.markdown_scroll,
                        move |href, _, cx| match this.update(cx, |view, cx| view.follow_markdown_link(href, cx)) {
                            Ok(()) | Err(_) => {} // Err: the tab closed
                        },
                        window,
                        cx,
                    );
                    div().relative().size_full().child(webview.clone()).child(div().absolute().inset_0().child(preview)).into_any_element()
                }
            },
        };
        let (can_back, can_forward, loading) = (self.nav.can_go_back(), self.nav.can_go_forward(), self.nav.loading());
        let tip: SharedString = invalid.clone().unwrap_or_else(|| address.read(cx).value().to_string()).into();
        let field = div()
            .id("webapp-address")
            .flex_1()
            .min_w_0()
            .mx(px(4.))
            // Escape drops what was typed and leaves the bar, also when nothing was.
            .on_key_down(cx.listener(|view, event: &gpui_kit::KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    view.draft = Draft::Idle;
                    window.blur(cx);
                    cx.notify();
                }
            }))
            .child(
                crate::ui::field(
                    &address,
                    crate::ui::FieldStyle {
                        bg: bg_primary,
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
            .bg(bg_secondary)
            .text_size(px(12.))
            .child(toolbar_button("page-back", "←", "Back", can_back, cx).when(can_back, |d| {
                d.on_click(cx.listener(|view, _, _, cx| view.back(cx)))
            }))
            .child(toolbar_button("page-forward", "→", "Forward", can_forward, cx).when(can_forward, |d| {
                d.on_click(cx.listener(|view, _, _, cx| view.forward(cx)))
            }));
        toolbar = if loading {
            toolbar.child(toolbar_button("page-stop", "✕", "Stop", true, cx).on_click(cx.listener(|view, _, _, cx| view.stop(cx))))
        } else {
            toolbar.child(toolbar_button("page-reload", "⟳", "Reload", true, cx).on_click(cx.listener(|view, _, _, cx| view.reload(cx))))
        };
        let home = self.url.clone();
        let external = self.nav.current().to_owned();
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

/// The page [`AssetPageView::image`] serves: `.file-viewer-image-container` around
/// `<img alt>`. It reports `ready <width>×<height>` or `error …` for the log, as the model
/// viewer does.
fn image_page_html(alt: &str) -> String {
    let alt: String = alt
        .chars()
        .map(|c| match c {
            '&' => "&amp;".to_owned(),
            '<' => "&lt;".to_owned(),
            '>' => "&gt;".to_owned(),
            '"' => "&quot;".to_owned(),
            '\'' => "&#39;".to_owned(),
            c => c.to_string(),
        })
        .collect();
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><style>\
         html, body {{ margin: 0; height: 100%; background: #1e1e1e; color: #cccccc; \
         font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; }} \
         body {{ display: flex; align-items: center; justify-content: center; padding: 16px; \
         box-sizing: border-box; overflow: auto; }} \
         img {{ max-width: 100%; max-height: 100%; object-fit: contain; }}\
         </style><script>function report(message) {{ if (window.ipc) window.ipc.postMessage(message) }}</script>\
         </head><body><img src=\"image\" alt=\"{alt}\" \
         onload=\"report('ready ' + this.naturalWidth + '\u{d7}' + this.naturalHeight)\" \
         onerror=\"report('error The image could not be decoded')\"></body></html>"
    )
}

/// A page served from memory to a webview: a PDF (WKWebView renders PDFs itself), the 3D
/// model viewer with the model's bytes, or an image GPUI cannot decode. Each page gets its
/// own URL scheme, so pages never share a registration (webkitgtk registers schemes per web
/// context).
pub struct AssetPageView {
    files: std::sync::Arc<std::collections::HashMap<String, (&'static str, Asset)>>,
    /// The path and query the page opens at.
    entry: String,
    /// What the page is, for the error shown when it cannot be built.
    what: &'static str,
    page: Page,
    shown: bool,
    /// The page zoom ([`AssetPageView::set_zoom`]); `1.0` as the page opens.
    zoom: f64,
    command: PageCommand,
}

/// How the last zoom or print asked of a page went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageCommand {
    Done,
    Failed(String),
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

    /// An image GPUI cannot decode (AVIF), drawn by WebKit as `FileViewer` draws images:
    /// centred in 16px of padding and scaled down to fit. One WebKit cannot decode shows its
    /// broken-image icon and `alt`, as Chromium's `<img alt>` does.
    pub fn image(bytes: std::sync::Arc<Vec<u8>>, mime: &'static str, alt: &str) -> Self {
        let page = image_page_html(alt).into_bytes();
        let files = [
            ("index.html".to_owned(), ("text/html; charset=utf-8", Asset::Shared(std::sync::Arc::new(page)))),
            ("image".to_owned(), (mime, Asset::Shared(bytes))),
        ];
        Self::new(files.into_iter().collect(), "index.html".to_owned(), "the image")
    }

    fn new(files: std::collections::HashMap<String, (&'static str, Asset)>, entry: String, what: &'static str) -> Self {
        Self {
            files: std::sync::Arc::new(files),
            entry,
            what,
            page: Page::NotBuilt,
            shown: true,
            zoom: 1.,
            command: PageCommand::Done,
        }
    }

    pub fn zoom(&self) -> f64 {
        self.zoom
    }

    pub fn command(&self) -> &PageCommand {
        &self.command
    }

    /// Zoom the page, where `1.0` is the size it opened at (WebKit scales a PDF's pages).
    pub fn set_zoom(&mut self, zoom: f64, cx: &mut Context<Self>) {
        let result = match &self.page {
            Page::Live(webview) => webview.read(cx).set_zoom(zoom).map_err(|error| error.to_string()),
            Page::NotBuilt | Page::Failed(_) => Err(format!("{} is not shown", self.what)),
        };
        if result.is_ok() {
            self.zoom = zoom;
        }
        self.finish("Cannot zoom", result, cx);
    }

    /// Open the system print panel for the page.
    pub fn print(&mut self, cx: &mut Context<Self>) {
        let result = match &self.page {
            Page::Live(webview) => webview.read(cx).raw().print().map_err(|error| error.to_string()),
            Page::NotBuilt | Page::Failed(_) => Err(format!("{} is not shown", self.what)),
        };
        self.finish("Cannot print", result, cx);
    }

    fn finish(&mut self, action: &str, result: Result<(), String>, cx: &mut Context<Self>) {
        self.command = match result {
            Ok(()) => PageCommand::Done,
            Err(error) => {
                tracing::error!(page = self.what, %error, "{action}");
                PageCommand::Failed(format!("{action}: {error}"))
            }
        };
        cx.notify();
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
            // The model viewer and the image page report `ready …` / `error …`.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_image_page_names_its_file_escaped() {
        let page = image_page_html("a \"b\" <c> & d's.avif");
        assert!(
            page.contains("<img src=\"image\" alt=\"a &quot;b&quot; &lt;c&gt; &amp; d&#39;s.avif\" "),
            "{page}"
        );
        assert!(page.contains("padding: 16px"), "{page}");
    }

    #[test]
    fn the_model_page_has_every_element_its_script_shows() {
        let html = std::str::from_utf8(MODEL_VIEWER_HTML).unwrap();
        let script = include_str!("../assets/model-viewer/viewer.src.js");
        let ids: Vec<&str> = ["element('", "show('", "showError('"]
            .iter()
            .flat_map(|call| script.split(call).skip(1).filter_map(|rest| rest.split('\'').next()))
            .collect();
        for id in ["loading", "parsing", "parse-error", "stage-error", "stats", "hint", "warning", "toolbar"] {
            assert!(ids.contains(&id), "{id} not used: {ids:?}");
        }
        for id in ids {
            assert!(html.contains(&format!("id=\"{id}\"")), "no #{id} in index.html");
        }
        // Until the script runs, the page says what FileViewer's Suspense fallback says.
        assert!(html.contains("<div id=\"loading\" class=\"placeholder\">Loading 3D viewer…</div>"));
    }
}
