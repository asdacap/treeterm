//! Browser tabs: a web page in a native webview (WKWebView / WebKitGTK via wry) under the
//! toolbar of `PageViews.tsx` (back, forward, reload / stop, home, the address bar, open
//! externally). As in Electron, whose guard (`src/main/webviewGuard.ts`) checks only the
//! page the webview opens on and its popups, the start page must be on localhost but the
//! page may then go anywhere ([`tab_navigation`]); popups open in the system browser.
//!
//! The toolbar follows the webview's own navigation state ([`webview_native`], [`PageNav`]):
//! its history, a load from its request on, navigations within the page. A load that fails
//! before its page came (server down) shows a message in the page's place, where Electron's
//! webview shows a blank error page and WKWebView nothing.
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

use gpui_kit::component::input::{self, InputEvent, InputState};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    Animation, AnimationExt as _, AnyElement, App, Context, DispatchPhase, Div, Entity, FocusHandle, MouseDownEvent,
    SharedString, Subscription, Task, WeakEntity, Window, canvas, div, px, relative, rgb,
};
use gpui_webview::WebView;
use treeterm_model::apps::{
    MarkdownLink, PAGE_PROBE_SCRIPT, PageContent, ProxiedPages, TabNavigation, is_allowed_webview_url,
    is_markdown_page, markdown_link, page_content, resolve_address, tab_navigation,
};
use treeterm_model::page_nav::{NavInput, PageNav, PageState, Reload};
use treeterm_model::workspace_store::WorkspaceStore;

use crate::menus::{DeleteSelection, PasteAndMatchStyle};
use crate::theme::colors;
use crate::webview_native::{self, EditCommand};

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
                self._observer = Some(webview_native::observe(webview.raw(), move |input| {
                    if observed.send(PageEvent::Nav(input)).is_err() {
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
            // Its load ends as aborted ([`NavInput::Failed`]), so it did not fail.
            PageEvent::Undisplayable(url) => {
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

/// A load that failed before any page came, in the error page's place (Electron's webview
/// shows a blank page, WKWebView nothing).
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
                    let focus = gpui_kit::Focusable::focus_handle(webview.read(cx), cx);
                    let page = webview.clone();
                    let edit = move |command, cx: &mut App| webview_native::edit(page.read(cx).raw(), command);
                    page_edits(div().size_full().bg(gpui_kit::white()).child(webview.clone()), focus, edit)
                        .into_any_element()
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


/// A shown page's part in the Edit menu, as Electron's edit roles act on a focused `<webview>`.
/// A press on the page gives its webview's `focus` GPUI's focus: WebKitGTK passes presses on to
/// GPUI (AppKit does not, see `menus::FocusedPage`). While it has the focus, the Edit menu's
/// commands go to the page (`edit`), not to the field GPUI's focus was on before.
fn page_edits(page: Div, focus: FocusHandle, edit: impl Fn(EditCommand, &mut App) + 'static) -> Div {
    let edit = Rc::new(edit);
    let to_page = |command: EditCommand| {
        let edit = edit.clone();
        move |cx: &mut App| edit(command, cx)
    };
    let (undo, redo, cut) = (to_page(EditCommand::Undo), to_page(EditCommand::Redo), to_page(EditCommand::Cut));
    let (copy, paste, plain) =
        (to_page(EditCommand::Copy), to_page(EditCommand::Paste), to_page(EditCommand::PasteAsPlainText));
    let (delete, select_all) = (to_page(EditCommand::Delete), to_page(EditCommand::SelectAll));
    let pressed = canvas(
        |bounds, _, _| bounds,
        move |_, bounds, window, _| {
            window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                if phase == DispatchPhase::Bubble && bounds.contains(&event.position) {
                    window.focus(&focus, cx);
                }
            });
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full();
    page.relative()
        .child(pressed)
        .on_action(move |_: &input::Undo, _, cx| undo(cx))
        .on_action(move |_: &input::Redo, _, cx| redo(cx))
        .on_action(move |_: &input::Cut, _, cx| cut(cx))
        .on_action(move |_: &input::Copy, _, cx| copy(cx))
        .on_action(move |_: &input::Paste, _, cx| paste(cx))
        .on_action(move |_: &PasteAndMatchStyle, _, cx| plain(cx))
        .on_action(move |_: &DeleteSelection, _, cx| delete(cx))
        .on_action(move |_: &input::SelectAll, _, cx| select_all(cx))
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

/// The PDF viewer page (`assets/pdf-viewer`: Chromium's PDF viewer on pdf.js; `viewer.js`,
/// `viewer.css` and `pdf.worker.mjs` are generated by its `build.sh`).
const PDF_VIEWER_HTML: &[u8] = include_bytes!("../assets/pdf-viewer/index.html");
const PDF_VIEWER_JS: &[u8] = include_bytes!("../assets/pdf-viewer/viewer.js");
const PDF_VIEWER_CSS: &[u8] = include_bytes!("../assets/pdf-viewer/viewer.css");
const PDF_WORKER_JS: &[u8] = include_bytes!("../assets/pdf-viewer/pdf.worker.mjs");
// pdf.js's CMaps and standard fonts, which `build.sh` copies into `cmaps/` and
// `standard_fonts/`: `PDF_VIEWER_DATA`, each file's path in the page and its bytes, listed by
// the crate's `build.rs`.
include!(concat!(env!("OUT_DIR"), "/pdf_viewer_data.rs"));

/// A page served from memory to a webview: a PDF in the PDF viewer, the 3D model viewer with
/// the model's bytes, or an image GPUI cannot decode. Each page gets its own URL scheme, so
/// pages never share a registration (webkitgtk registers schemes per web context).
pub struct AssetPageView {
    files: std::sync::Arc<std::collections::HashMap<String, (&'static str, Asset)>>,
    /// The path and query the page opens at.
    entry: String,
    /// What the page is, for the error shown when it cannot be built.
    what: &'static str,
    page: Page,
    shown: bool,
    command: PageCommand,
    /// What the page asks of the app (`window.ipc`), handled in order.
    _messages: Option<Task<()>>,
}

/// How the last print asked of a page went: a failure shows above the page.
#[derive(Debug, Clone, PartialEq, Eq)]
enum PageCommand {
    Done,
    Failed(String),
}

/// What a page posts to the app (`window.ipc.postMessage`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PageMessage<'a> {
    /// The PDF viewer's Print, its pages laid out for print: open the print panel.
    Print,
    /// The PDF viewer's Print failed in the page (pdf.js could not draw a page for print):
    /// `print-failed <reason>`.
    PrintFailed(&'a str),
    /// The PDF viewer's Download.
    Download,
    /// `ready …` and `error …` from every page, for the log.
    Report(&'a str),
}

impl<'a> PageMessage<'a> {
    fn parse(message: &'a str) -> Self {
        match message {
            "print" => PageMessage::Print,
            "download" => PageMessage::Download,
            _ => match message.strip_prefix("print-failed ") {
                Some(reason) => PageMessage::PrintFailed(reason),
                None => PageMessage::Report(message),
            },
        }
    }
}

/// What a page asks of its owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetPageEvent {
    /// The PDF viewer's Download: save a copy of the file.
    Download,
}

impl gpui_kit::EventEmitter<AssetPageEvent> for AssetPageView {}

impl AssetPageView {
    /// `name` is the file's, the viewer's title unless the PDF has one of its own. pdf.js reads
    /// the CMaps and standard fonts a PDF names but does not embed from the page's `cmaps/`
    /// and `standard_fonts/`.
    pub fn pdf(bytes: std::sync::Arc<Vec<u8>>, name: &str) -> Self {
        let files = [
            ("index.html".to_owned(), ("text/html; charset=utf-8", Asset::Static(PDF_VIEWER_HTML))),
            ("viewer.js".to_owned(), ("text/javascript; charset=utf-8", Asset::Static(PDF_VIEWER_JS))),
            ("viewer.css".to_owned(), ("text/css; charset=utf-8", Asset::Static(PDF_VIEWER_CSS))),
            ("pdf.worker.mjs".to_owned(), ("text/javascript; charset=utf-8", Asset::Static(PDF_WORKER_JS))),
            ("document.pdf".to_owned(), ("application/pdf", Asset::Shared(bytes))),
            (
                "name".to_owned(),
                ("text/plain; charset=utf-8", Asset::Shared(std::sync::Arc::new(name.as_bytes().to_vec()))),
            ),
        ];
        let data = PDF_VIEWER_DATA
            .iter()
            .map(|(path, bytes)| ((*path).to_owned(), ("application/octet-stream", Asset::Static(bytes))));
        Self::new(files.into_iter().chain(data).collect(), "index.html".to_owned(), "the PDF")
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
            command: PageCommand::Done,
            _messages: None,
        }
    }

    /// Open the system print panel for the page (the PDF viewer lays its pages out for print
    /// first).
    pub fn print(&mut self, cx: &mut Context<Self>) {
        let result = match &self.page {
            Page::Live(webview) => webview.read(cx).raw().print().map_err(|error| error.to_string()),
            Page::NotBuilt | Page::Failed(_) => Err(format!("{} is not shown", self.what)),
        };
        match result {
            Ok(()) => {
                self.command = PageCommand::Done;
                cx.notify();
            }
            Err(error) => self.print_failed(&error, cx),
        }
    }

    /// A print that failed, in the page or opening the print panel: shown above the page.
    fn print_failed(&mut self, error: &str, cx: &mut Context<Self>) {
        tracing::error!(page = self.what, %error, "Cannot print");
        self.command = PageCommand::Failed(format!("Cannot print: {error}"));
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

    /// What the page asks ([`PageMessage`]).
    fn on_message(&mut self, message: &str, cx: &mut Context<Self>) {
        match PageMessage::parse(message) {
            PageMessage::Print => self.print(cx),
            PageMessage::PrintFailed(reason) => self.print_failed(reason, cx),
            PageMessage::Download => cx.emit(AssetPageEvent::Download),
            PageMessage::Report(message) => tracing::info!(page = self.what, %message, "asset page"),
        }
    }

    fn build(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        static NEXT_SCHEME: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let scheme = format!("treeterm-asset{}", NEXT_SCHEME.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
        let files = self.files.clone();
        let prefix = format!("{scheme}://");
        let (messages, mut received) = tokio::sync::mpsc::unbounded_channel::<String>();
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
            .with_ipc_handler(move |request| {
                if messages.send(request.body().clone()).is_err() {
                    tracing::debug!("asset page closed while it sent a message");
                }
            })
            .with_navigation_handler(move |url| url.starts_with(&prefix))
            .with_new_window_req_handler(|url, _| {
                open_externally(&url);
                wry::NewWindowResponse::Deny
            });
        self.page = match WebView::build(builder, window, cx) {
            Ok(webview) => {
                self._messages = Some(cx.spawn(async move |view, cx| {
                    while let Some(message) = received.recv().await {
                        if view.update(cx, |view, cx| view.on_message(&message, cx)).is_err() {
                            return;
                        }
                    }
                }));
                Page::Live(cx.new(|_| webview))
            }
            Err(error) => Page::Failed(format!("Cannot show {}: {error}", self.what)),
        };
    }
}

impl Render for AssetPageView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Page::NotBuilt = self.page {
            self.build(window, cx);
        }
        let failure = match &self.command {
            PageCommand::Done => None,
            PageCommand::Failed(message) => {
                Some(div().flex_shrink_0().px_4().py_1().text_color(colors(cx).error).child(message.clone()))
            }
        };
        let page = match &self.page {
            Page::NotBuilt => div().into_any_element(),
            Page::Failed(message) => div().p_4().text_color(colors(cx).error).child(message.clone()).into_any_element(),
            Page::Live(webview) => div().flex_1().min_h_0().child(webview.clone()).into_any_element(),
        };
        v_flex().size_full().children(failure).child(page)
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

    /// A text field above a page standing in for the webview, whose Edit commands are recorded.
    struct PageUnderField {
        field: Entity<InputState>,
        page: FocusHandle,
        taken: Rc<RefCell<Vec<EditCommand>>>,
    }

    impl Render for PageUnderField {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let taken = self.taken.clone();
            // As `gpui_webview`'s: the native view covers the element tracking the focus.
            let native = canvas(
                |bounds, window, _| window.insert_hitbox(bounds, gpui_kit::HitboxBehavior::BlockMouse),
                |_, _, _, _| {},
            )
            .size_full();
            let page = div().size_full().child(div().size_full().track_focus(&self.page).child(native));
            v_flex()
                .size_full()
                .child(gpui_kit::component::input::Input::new(&self.field))
                .child(page_edits(page, self.page.clone(), move |command, _| taken.borrow_mut().push(command)))
        }
    }

    /// A press on the page (which WebKitGTK passes on to GPUI) takes GPUI's focus from the
    /// field, and the Edit menu's commands then go to the page, not the field.
    #[gpui_kit::test]
    fn a_pressed_page_takes_the_edit_menu(cx: &mut gpui_kit::TestAppContext) {
        let taken = Rc::new(RefCell::new(Vec::new()));
        let recorded = taken.clone();
        let (window, view) = cx.update(|cx| {
            gpui_kit::init(cx);
            gpui_kit::open_window(gpui_kit::WindowOptions::default(), cx, |window, cx| {
                let field = cx.new(|cx| InputState::new(window, cx).default_value("hello"));
                cx.new(|cx| PageUnderField { field, page: cx.focus_handle(), taken: recorded })
            })
            .expect("open a window")
        });
        let cx = gpui_kit::VisualTestContext::from_window(window, cx).into_mut();
        let (field, page) = view.read_with(cx, |view, _| (view.field.clone(), view.page.clone()));
        cx.update(|window, cx| field.update(cx, |field, cx| field.focus(window, cx)));
        cx.run_until_parked();

        let size = cx.update(|window, _| window.viewport_size());
        cx.simulate_click(gpui_kit::point(size.width / 2., size.height - px(10.)), gpui_kit::Modifiers::default());
        assert!(cx.update(|window, _| page.is_focused(window)), "the page has the focus");
        let menu: [(Box<dyn gpui_kit::Action>, EditCommand); 8] = [
            (Box::new(input::Undo), EditCommand::Undo),
            (Box::new(input::Redo), EditCommand::Redo),
            (Box::new(input::Cut), EditCommand::Cut),
            (Box::new(input::Copy), EditCommand::Copy),
            (Box::new(input::Paste), EditCommand::Paste),
            (Box::new(PasteAndMatchStyle), EditCommand::PasteAsPlainText),
            (Box::new(DeleteSelection), EditCommand::Delete),
            (Box::new(input::SelectAll), EditCommand::SelectAll),
        ];
        let expected: Vec<EditCommand> = menu.iter().map(|(_, command)| *command).collect();
        for (action, _) in menu {
            cx.update(|window, cx| window.dispatch_action(action, cx));
        }
        cx.run_until_parked();
        assert_eq!(*taken.borrow(), expected);
        assert_eq!(cx.update(|_, cx| field.read(cx).value().to_string()), "hello");
    }

    #[test]
    fn the_pdf_page_has_every_element_its_script_uses_and_asks_what_the_app_does() {
        let html = std::str::from_utf8(PDF_VIEWER_HTML).unwrap();
        let script = include_str!("../assets/pdf-viewer/viewer.src.js");
        let ids: Vec<&str> = ["element('", "show('"]
            .iter()
            .flat_map(|call| script.split(call).skip(1).filter_map(|rest| rest.split('\'').next()))
            .collect();
        // Chromium's toolbar: the page box, zoom, fit, rotate, download, print; the sidebar.
        for id in [
            "page-input",
            "page-count",
            "zoom-out",
            "zoom-input",
            "zoom-in",
            "fit",
            "rotate",
            "download",
            "print",
            "sidebar-toggle",
            "thumbnails",
            "outline",
            "printContainer",
        ] {
            assert!(ids.contains(&id), "{id} not used: {ids:?}");
        }
        for id in ids {
            assert!(html.contains(&format!("id=\"{id}\"")), "no #{id} in index.html");
        }
        for title in ["Zoom out", "Zoom in", "Fit to page", "Rotate counterclockwise", "Download", "Print"] {
            assert!(html.contains(&format!("title=\"{title}\"")), "no {title} button");
        }
        // The page's files, and the messages `on_message` answers.
        for file in ["viewer.js", "viewer.css"] {
            assert!(html.contains(&format!("\"{file}\"")), "{file} not loaded");
        }
        for asked in [
            "fetch('name')",
            "fetch('document.pdf')",
            "workerSrc = 'pdf.worker.mjs'",
            "report('print')",
            "report(`print-failed ${",
            "report('download')",
            "cMapUrl: 'cmaps/'",
            "cMapPacked: true",
            "standardFontDataUrl: 'standard_fonts/'",
        ] {
            assert!(script.contains(asked), "{asked} not in the script");
        }
    }

    /// A PDF with a password asks for it in Chromium's password dialog, again after a wrong one,
    /// with Chromium's words.
    #[test]
    fn the_pdf_page_asks_for_a_password_as_chromium_does() {
        let html = std::str::from_utf8(PDF_VIEWER_HTML).unwrap();
        let script = include_str!("../assets/pdf-viewer/viewer.src.js");
        for asked in [
            "loadingTask.onPassword = password.ask",
            "reason === pdfjsLib.PasswordResponses.INCORRECT_PASSWORD",
            "element('password-error').hidden = !incorrect",
            "show('password-dialog')",
            "updatePassword(input.value)",
        ] {
            assert!(script.contains(asked), "{asked} not in the script");
        }
        for id in ["password-form", "password-input", "password-submit"] {
            assert!(script.contains(&format!("element('{id}')")), "#{id} not used");
        }
        assert!(html.contains("<div id=\"password-dialog\" hidden>"), "the dialog shows only when asked");
        assert!(html.contains("<div id=\"password-error\" hidden>Incorrect password</div>"));
        assert!(html.contains("<input id=\"password-input\" type=\"password\""));
        for text in [
            ">Password required<",
            ">This document is password protected.  Please enter a password.<",
            "type=\"submit\">Submit<",
        ] {
            assert!(html.contains(text), "{text} not in index.html");
        }
    }

    /// Text in a predefined (CJK) CMap or a standard font the PDF does not embed: pdf.js asks
    /// the page for the CMap's or the font's file, and every one its worker can ask for is
    /// served, and nothing else of pdfjs-dist's data.
    #[test]
    fn the_pdf_page_serves_every_cmap_and_standard_font_pdfjs_asks_for() {
        let page = AssetPageView::pdf(std::sync::Arc::new(b"%PDF-1.4".to_vec()), "a.pdf");
        let served = |path: &str| match page.files.get(path) {
            Some(("application/octet-stream", Asset::Static(bytes))) => !bytes.is_empty(),
            _ => false,
        };
        let worker = std::str::from_utf8(PDF_WORKER_JS).unwrap();
        // The worker's built-in CMaps (`BUILT_IN_CMAPS`): the list that starts with Adobe-GB1-UCS2.
        let start = worker.find("[\"Adobe-GB1-UCS2\"").expect("the worker lists its built-in CMaps");
        let end = start + worker[start..].find(']').unwrap();
        let cmaps: Vec<&str> = worker[start + 1..end].split(',').map(|name| name.trim_matches('"')).collect();
        for name in ["UniJIS-UCS2-H", "Adobe-Japan1-UCS2", "UniGB-UCS2-H", "UniCNS-UCS2-H", "UniKS-UCS2-H"] {
            assert!(cmaps.contains(&name), "{name} not in {cmaps:?}");
        }
        for name in &cmaps {
            assert!(served(&format!("cmaps/{name}.bcmap")), "cmaps/{name}.bcmap not served");
        }
        // The standard fonts' files (the worker's `getFontNameToFileMap`).
        let fonts: std::collections::BTreeSet<&str> = worker
            .split('"')
            .filter(|text| text.ends_with(".pfb") || text.ends_with(".ttf"))
            .collect();
        for name in ["FoxitSymbol.pfb", "FoxitDingbats.pfb", "LiberationSans-Regular.ttf"] {
            assert!(fonts.contains(name), "{name} not in {fonts:?}");
        }
        for name in &fonts {
            assert!(served(&format!("standard_fonts/{name}")), "standard_fonts/{name} not served");
        }
        assert_eq!(PDF_VIEWER_DATA.len(), cmaps.len() + fonts.len());
        assert!(!served("cmaps/LICENSE") && !served("standard_fonts/LICENSE_FOXIT"));
    }

    #[test]
    fn the_pdf_page_serves_the_viewer_the_document_and_its_name() {
        let page = AssetPageView::pdf(std::sync::Arc::new(b"%PDF-1.4".to_vec()), "a b.pdf");
        assert_eq!(page.entry, "index.html");
        let file = |path: &str| match page.files.get(path) {
            Some((mime, Asset::Static(bytes))) => (*mime, bytes.to_vec()),
            Some((mime, Asset::Shared(bytes))) => (*mime, (**bytes).clone()),
            None => panic!("{path} not served"),
        };
        assert_eq!(file("document.pdf"), ("application/pdf", b"%PDF-1.4".to_vec()));
        assert_eq!(file("name"), ("text/plain; charset=utf-8", b"a b.pdf".to_vec()));
        for path in ["index.html", "viewer.js", "viewer.css", "pdf.worker.mjs"] {
            assert!(!file(path).1.is_empty(), "{path} is empty");
        }
    }

    #[test]
    fn a_pages_messages_are_told_apart() {
        assert_eq!(PageMessage::parse("print"), PageMessage::Print);
        assert_eq!(PageMessage::parse("download"), PageMessage::Download);
        assert_eq!(
            PageMessage::parse("print-failed Bad page: 2 of 3"),
            PageMessage::PrintFailed("Bad page: 2 of 3")
        );
        assert_eq!(PageMessage::parse("ready 3 pages"), PageMessage::Report("ready 3 pages"));
        assert_eq!(PageMessage::parse("error Invalid PDF"), PageMessage::Report("error Invalid PDF"));
        assert_eq!(PageMessage::parse("printed"), PageMessage::Report("printed"));
    }

    /// A print the PDF viewer could not lay out shows above the page, as one whose print panel
    /// cannot open does.
    #[gpui_kit::test]
    fn a_print_that_fails_in_the_page_shows_above_it(cx: &mut gpui_kit::TestAppContext) {
        use gpui_kit::AppContext as _;
        let view = cx.new(|_| AssetPageView::pdf(std::sync::Arc::new(b"%PDF-1.4".to_vec()), "a.pdf"));
        let notified = std::rc::Rc::new(std::cell::Cell::new(0));
        cx.update(|cx| {
            let notified = notified.clone();
            cx.observe(&view, move |_, _| notified.set(notified.get() + 1)).detach();
        });
        let command = |cx: &mut gpui_kit::TestAppContext| view.read_with(cx, |page, _| page.command.clone());
        view.update(cx, |page, cx| page.on_message("print-failed Bad page: 2 of 3", cx));
        cx.run_until_parked();
        assert_eq!(command(cx), PageCommand::Failed("Cannot print: Bad page: 2 of 3".into()));
        assert_eq!(notified.get(), 1);
        // Not built (no window in tests), the page cannot open the print panel either; what
        // the page only reports leaves the failure shown.
        view.update(cx, |page, cx| page.on_message("print", cx));
        assert_eq!(command(cx), PageCommand::Failed("Cannot print: the PDF is not shown".into()));
        view.update(cx, |page, cx| page.on_message("ready 1 pages", cx));
        assert_eq!(command(cx), PageCommand::Failed("Cannot print: the PDF is not shown".into()));
    }
}
