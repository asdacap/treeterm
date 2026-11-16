//! A Browser tab's webview's own navigation state, which wry does not report: it sends
//! `Started` only once a response commits, and nothing for a load's request, a load that
//! fails, or a navigation within the page. WKWebView (key-value observing) and WebKitGTK
//! (property notifications) report it as it changes; WKWebView's navigation delegate also
//! reports a new page's load starting and failing (WebKitGTK shows its own error page). Also
//! the history, reload and stop calls wry does not wrap, and the Edit menu's commands on a
//! page.

#[cfg(target_os = "macos")]
pub use platform::{send_edit, web_page_has_focus};
pub use platform::{NavObserver, edit, go_back, go_forward, observe, reload_from_origin, stop_loading};

/// An Edit menu command, as a web page takes it (Electron's edit roles on a focused
/// `<webview>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditCommand {
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    /// macOS's Paste and Match Style.
    PasteAsPlainText,
    /// Delete the selection.
    Delete,
    SelectAll,
}

impl EditCommand {
    /// The AppKit action a menu item sends the first responder for it.
    #[cfg(target_os = "macos")]
    pub fn selector(self) -> &'static std::ffi::CStr {
        match self {
            EditCommand::Undo => c"undo:",
            EditCommand::Redo => c"redo:",
            EditCommand::Cut => c"cut:",
            EditCommand::Copy => c"copy:",
            EditCommand::Paste => c"paste:",
            EditCommand::PasteAsPlainText => c"pasteAsPlainText:",
            EditCommand::Delete => c"delete:",
            EditCommand::SelectAll => c"selectAll:",
        }
    }

    /// WebKitGTK's editing command for it (`webkit_web_view_execute_editing_command`).
    #[cfg(target_os = "linux")]
    pub fn webkit_command(self) -> &'static str {
        match self {
            EditCommand::Undo => "Undo",
            EditCommand::Redo => "Redo",
            EditCommand::Cut => "Cut",
            EditCommand::Copy => "Copy",
            EditCommand::Paste => "Paste",
            EditCommand::PasteAsPlainText => "PasteAsPlainText",
            EditCommand::Delete => "Delete",
            EditCommand::SelectAll => "SelectAll",
        }
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use std::ffi::c_void;
    use std::rc::Rc;

    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, NSObject, ProtocolObject, Sel};
    use objc2::{AllocAnyThread, DefinedClass, MainThreadOnly, define_class, msg_send};
    use objc2_foundation::{
        NSDictionary, NSError, NSKeyValueChangeKey, NSKeyValueObservingOptions,
        NSObjectNSKeyValueObserverRegistration, NSObjectProtocol, NSString, NSURL,
        NSURLErrorFailingURLErrorKey,
    };
    use objc2_web_kit::{WKNavigation, WKNavigationDelegate, WKWebView};
    use treeterm_model::page_nav::{LoadFailure, NativeNav, NavInput};

    /// WKWebView's key-value observable properties a [`NativeNav`] is read from.
    const KEYS: [&str; 4] = ["URL", "loading", "canGoBack", "canGoForward"];

    type Report = Rc<dyn Fn(NavInput)>;

    struct ObserverIvars {
        webview: Retained<WKWebView>,
        report: Report,
    }

    define_class!(
        #[unsafe(super(NSObject))]
        #[name = "TreeTermNavigationObserver"]
        #[ivars = ObserverIvars]
        struct Observer;

        /// NSKeyValueObserving.
        impl Observer {
            #[unsafe(method(observeValueForKeyPath:ofObject:change:context:))]
            fn observe_value(
                &self,
                _key_path: Option<&NSString>,
                _object: Option<&AnyObject>,
                _change: Option<&NSDictionary<NSKeyValueChangeKey, AnyObject>>,
                _context: *mut c_void,
            ) {
                // Read whole: one change can move several properties at once.
                let ivars = self.ivars();
                (ivars.report)(NavInput::Native(snapshot(&ivars.webview)));
            }
        }

        unsafe impl NSObjectProtocol for Observer {}
    );

    struct DelegateIvars {
        /// wry's delegate, which keeps handling everything else (navigation policy, commits,
        /// downloads): calls this one does not answer are forwarded to it.
        wry: Retained<ProtocolObject<dyn WKNavigationDelegate>>,
        report: Report,
    }

    define_class!(
        /// Stands in for wry's navigation delegate, which does not report a load starting or
        /// failing before its page came.
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "TreeTermNavigationDelegate"]
        #[ivars = DelegateIvars]
        struct Delegate;

        /// Forwarding to wry's delegate. WebKit asks which callbacks the delegate has when it
        /// is set, so the answer includes wry's.
        impl Delegate {
            #[unsafe(method(respondsToSelector:))]
            fn responds_to_selector(&self, selector: Sel) -> bool {
                // SAFETY: NSObject's own answer, for the methods this class defines.
                let own: bool = unsafe { msg_send![super(self), respondsToSelector: selector] };
                own || self.ivars().wry.respondsToSelector(selector)
            }

            #[unsafe(method(forwardingTargetForSelector:))]
            fn forwarding_target(&self, _selector: Sel) -> *mut AnyObject {
                Retained::as_ptr(&self.ivars().wry).cast_mut().cast()
            }
        }

        unsafe impl NSObjectProtocol for Delegate {}

        unsafe impl WKNavigationDelegate for Delegate {
            #[unsafe(method(webView:didStartProvisionalNavigation:))]
            fn did_start_provisional_navigation(
                &self,
                webview: &WKWebView,
                _navigation: Option<&WKNavigation>,
            ) {
                (self.ivars().report)(NavInput::Started(url_of(webview)));
            }

            #[unsafe(method(webView:didFailProvisionalNavigation:withError:))]
            fn did_fail_provisional_navigation(
                &self,
                _webview: &WKWebView,
                _navigation: Option<&WKNavigation>,
                error: &NSError,
            ) {
                (self.ivars().report)(NavInput::Failed(failure(error)));
            }
        }
    );

    /// Reports until dropped.
    pub struct NavObserver {
        observer: Retained<Observer>,
        /// Set as the webview's navigation delegate in wry's place, which it is again once
        /// dropped. None when the webview had no delegate to stand in for (wry always sets
        /// one): its loads' failures then go unreported.
        delegate: Option<Retained<Delegate>>,
    }

    impl Drop for NavObserver {
        fn drop(&mut self) {
            let webview = &self.observer.ivars().webview;
            for key in KEYS {
                // SAFETY: registered for this key on this webview in `observe`.
                unsafe {
                    webview.removeObserver_forKeyPath(&self.observer, &NSString::from_str(key))
                };
            }
            if let Some(delegate) = &self.delegate {
                // SAFETY: wry's delegate, which wry keeps alive with its webview.
                unsafe { webview.setNavigationDelegate(Some(&*delegate.ivars().wry)) };
            }
        }
    }

    /// Report the webview's navigation state now and whenever it changes, and a new page's
    /// load starting or failing, until the observer is dropped: on the main thread as WebKit
    /// reports them, so in order with wry's callbacks.
    pub fn observe(webview: &wry::WebView, report: impl Fn(NavInput) + 'static) -> NavObserver {
        let webview = wk(webview);
        let report: Report = Rc::new(report);
        let observer = Observer::alloc().set_ivars(ObserverIvars {
            webview: webview.clone(),
            report: report.clone(),
        });
        // SAFETY: NSObject's designated initialiser.
        let observer: Retained<Observer> = unsafe { msg_send![super(observer), init] };
        for key in KEYS {
            // SAFETY: the observer outlives the registration (`NavObserver::drop` removes it)
            // and keeps the webview alive until then.
            unsafe {
                webview.addObserver_forKeyPath_options_context(
                    &observer,
                    &NSString::from_str(key),
                    NSKeyValueObservingOptions::New,
                    std::ptr::null_mut(),
                );
            }
        }
        // SAFETY: a property read on the main thread.
        let delegate = match unsafe { webview.navigationDelegate() } {
            Some(wry) => {
                let delegate = Delegate::alloc(webview.mtm()).set_ivars(DelegateIvars {
                    wry,
                    report: report.clone(),
                });
                // SAFETY: NSObject's designated initialiser.
                let delegate: Retained<Delegate> = unsafe { msg_send![super(delegate), init] };
                // SAFETY: the delegate outlives the registration (the webview holds it weakly;
                // `NavObserver::drop` puts wry's back).
                unsafe {
                    webview.setNavigationDelegate(Some(ProtocolObject::from_ref(&*delegate)))
                };
                Some(delegate)
            }
            None => {
                tracing::error!(
                    "a browser tab's webview has no navigation delegate: its failed loads are not shown"
                );
                None
            }
        };
        report(NavInput::Native(snapshot(&webview)));
        NavObserver { observer, delegate }
    }

    fn url_of(webview: &WKWebView) -> String {
        // SAFETY: a property read, on the main thread (GPUI's and WebKit's).
        unsafe { webview.URL() }
            .and_then(|url| url.absoluteString())
            .map(|url| url.to_string())
            .unwrap_or_default()
    }

    fn snapshot(webview: &WKWebView) -> NativeNav {
        // SAFETY: plain property reads, on the main thread (GPUI's and WebKit's).
        unsafe {
            NativeNav {
                url: url_of(webview),
                loading: webview.isLoading(),
                can_go_back: webview.canGoBack(),
                can_go_forward: webview.canGoForward(),
            }
        }
    }

    fn failure(error: &NSError) -> LoadFailure {
        // SAFETY: a static NSString WebKit's errors are keyed by.
        let key = unsafe { NSURLErrorFailingURLErrorKey };
        let url = error
            .userInfo()
            .objectForKey(key)
            .and_then(|url| url.downcast::<NSURL>().ok())
            .and_then(|url| url.absoluteString())
            .map(|url| url.to_string())
            .unwrap_or_default();
        LoadFailure {
            url,
            domain: error.domain().to_string(),
            code: error.code() as i64,
        }
    }

    fn wk(webview: &wry::WebView) -> Retained<WKWebView> {
        Retained::into_super(wry::WebViewExtMacOS::webview(webview))
    }

    // SAFETY (below): WKWebView's navigation calls, on the main thread. Their `WKNavigation`
    // is only a handle for delegate callbacks.

    pub fn go_back(webview: &wry::WebView) {
        drop(unsafe { wk(webview).goBack() });
    }

    pub fn go_forward(webview: &wry::WebView) {
        drop(unsafe { wk(webview).goForward() });
    }

    /// Reload, bypassing the cache (`reloadIgnoringCache`).
    pub fn reload_from_origin(webview: &wry::WebView) {
        drop(unsafe { wk(webview).reloadFromOrigin() });
    }

    pub fn stop_loading(webview: &wry::WebView) {
        unsafe { wk(webview).stopLoading() };
    }

    /// The shared application. GPUI calls the app back on the main thread, as AppKit requires.
    fn app() -> Retained<objc2_app_kit::NSApplication> {
        let main_thread = objc2::MainThreadMarker::new().expect("AppKit is called from the main thread");
        objc2_app_kit::NSApplication::sharedApplication(main_thread)
    }

    /// Whether the key window's first responder is a web page (a WKWebView, or a view in
    /// one): the page took the keyboard, which GPUI does not see.
    pub fn web_page_has_focus() -> bool {
        let Some(responder) = app().keyWindow().and_then(|window| window.firstResponder()) else {
            return false;
        };
        let mut view = responder.downcast::<objc2_app_kit::NSView>().ok();
        while let Some(current) = view {
            if current.isKindOfClass(<WKWebView as objc2::ClassType>::class()) {
                return true;
            }
            // SAFETY: a view property read, on the main thread.
            view = unsafe { current.superview() };
        }
        false
    }

    /// Send `command` to the key window's first responder, as an AppKit Edit menu item does.
    pub fn send_edit(command: super::EditCommand) {
        let action = objc2::runtime::Sel::register(command.selector());
        // SAFETY: an action message, sent down the responder chain on the main thread.
        if !unsafe { app().sendAction_to_from(action, None, None) } {
            tracing::warn!(?command, "nothing in the responder chain took the Edit command");
        }
    }

    /// Give `webview` the keyboard and send it `command`.
    pub fn edit(webview: &wry::WebView, command: super::EditCommand) {
        let page = wk(webview);
        match page.window() {
            Some(window) => {
                window.makeFirstResponder(Some(&page));
                send_edit(command);
            }
            None => tracing::warn!(?command, "the page is in no window"),
        }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use std::rc::Rc;

    use treeterm_model::page_nav::{NativeNav, NavInput};
    use webkit2gtk::WebViewExt as _;
    use webkit2gtk::glib::SignalHandlerId;
    use webkit2gtk::glib::prelude::ObjectExt as _;

    /// Reports until dropped.
    pub struct NavObserver {
        webview: webkit2gtk::WebView,
        handlers: Vec<SignalHandlerId>,
        history: Option<(webkit2gtk::BackForwardList, SignalHandlerId)>,
    }

    impl Drop for NavObserver {
        fn drop(&mut self) {
            for handler in self.handlers.drain(..) {
                self.webview.disconnect(handler);
            }
            if let Some((list, handler)) = self.history.take() {
                list.disconnect(handler);
            }
        }
    }

    /// Report the webview's navigation state now and whenever it changes, until the observer
    /// is dropped: on the main thread (GTK runs there) as WebKit changes it, so in order with
    /// wry's callbacks. A failed load is not reported: WebKitGTK shows its own error page.
    pub fn observe(webview: &wry::WebView, report: impl Fn(NavInput) + 'static) -> NavObserver {
        let webview = wry::WebViewExtUnix::webview(webview);
        let report = Rc::new(report);
        let handlers = vec![
            {
                let report = report.clone();
                webview.connect_is_loading_notify(move |webview| report(snapshot(webview)))
            },
            {
                let report = report.clone();
                webview.connect_uri_notify(move |webview| report(snapshot(webview)))
            },
        ];
        // Back / Forward change with the history (WebKitGTK has no property for them).
        let history = webview.back_forward_list().map(|list| {
            let (report, weak) = (report.clone(), webview.downgrade());
            let handler = list.connect_local("changed", false, move |_| {
                if let Some(webview) = weak.upgrade() {
                    report(snapshot(&webview));
                }
                None
            });
            (list, handler)
        });
        report(snapshot(&webview));
        NavObserver {
            webview,
            handlers,
            history,
        }
    }

    fn snapshot(webview: &webkit2gtk::WebView) -> NavInput {
        NavInput::Native(NativeNav {
            url: webview.uri().map(String::from).unwrap_or_default(),
            loading: webview.is_loading(),
            can_go_back: webview.can_go_back(),
            can_go_forward: webview.can_go_forward(),
        })
    }

    pub fn go_back(webview: &wry::WebView) {
        wry::WebViewExtUnix::webview(webview).go_back();
    }

    pub fn go_forward(webview: &wry::WebView) {
        wry::WebViewExtUnix::webview(webview).go_forward();
    }

    /// Reload, bypassing the cache (`reloadIgnoringCache`).
    pub fn reload_from_origin(webview: &wry::WebView) {
        wry::WebViewExtUnix::webview(webview).reload_bypass_cache();
    }

    pub fn stop_loading(webview: &wry::WebView) {
        wry::WebViewExtUnix::webview(webview).stop_loading();
    }

    /// Run `command` on the page and give it the keyboard back, which the menu bar drawn in
    /// the GPUI window took.
    pub fn edit(webview: &wry::WebView, command: super::EditCommand) {
        wry::WebViewExtUnix::webview(webview).execute_editing_command(command.webkit_command());
        if let Err(error) = webview.focus() {
            tracing::warn!(%error, ?command, "cannot give the page the keyboard back");
        }
    }
}
