//! A Browser tab's navigation, followed from its webview's own state as `PageViews.tsx`
//! follows `did-navigate`, `did-navigate-in-page`, `did-start-loading` and
//! `did-stop-loading`: the page shown (also after `pushState` and fragment changes), whether
//! a load is under way (from the request, not the response), Back / Forward from the
//! webview's history, and a load that ended without a page (Chromium shows its error page
//! for it; WKWebView shows nothing).

use crate::apps::unproxied_url;

/// What the native webview says of its navigation, read whenever it changes (WKWebView's
/// key-value observable `URL`, `loading`, `canGoBack` and `canGoForward`; WebKitGTK's `uri`,
/// `is-loading` and back-forward list).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NativeNav {
    /// The active URL: while a load is under way the one it is loading, otherwise the
    /// page's, with its in-page changes. Empty before any page.
    pub url: String,
    pub loading: bool,
    pub can_go_back: bool,
    pub can_go_forward: bool,
}

/// What a Browser tab's [`PageNav`] follows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NavInput {
    /// The webview's navigation state changed.
    Native(NativeNav),
    /// The tab loads a URL itself (address bar, Home, a markdown link, Reload of a page that
    /// could not be loaded): a new load, even while another is under way.
    Requested(String),
    /// Reload (from the origin) of the page shown.
    Reloading,
    /// The page being loaded replaced the one shown, at this URL (wry's `Started`, sent from
    /// `didCommitNavigation`).
    Committed(String),
    /// The load turned into a download (a page the webview cannot display): it ends without
    /// a page, which is no failure.
    Downloaded,
    /// Stop was pressed: the load ends without a page, which is no failure. With nothing
    /// loading in the webview (WebKit ignores a reload without a page), it ends at once, as
    /// no end will come.
    Stopped,
}

/// What the tab does after a [`NavInput`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NavEffects {
    /// The tab went to another page or moved within it (`did-navigate`,
    /// `did-navigate-in-page`): it is remembered.
    pub navigated: bool,
    /// A load ended, however it ended (`did-stop-loading`): the page is probed.
    pub load_ended: bool,
}

/// What the page area shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageState {
    /// The webview's page (or its rendered markdown).
    Shown,
    /// The last load ended before any page came (server down, connection refused): shown
    /// in Chromium's error page's place until another load starts.
    Failed { url: String },
}

/// What Reload does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reload {
    /// Reload the page shown, bypassing the cache (`reloadIgnoringCache`).
    FromOrigin,
    /// Load the page that could not be loaded again (Chromium reloads its error page's URL),
    /// or the page shown when the webview has none to reload.
    Retry(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Load {
    Idle,
    /// Under way for `url` (redirects move it), with no page yet.
    Provisional {
        url: String,
        kind: Attempt,
    },
    /// Its page replaced the one shown; it loads the rest.
    Committed,
}

/// What a load that ends without a page means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Attempt {
    /// It failed, unless the webview got to its URL without a new page: a navigation within
    /// the page shown (a fragment, a `pushState` entry), which WebKit also reports as a load
    /// when the tab asked for it.
    Load,
    /// The page shown loaded again: it failed, although the URL stays the same.
    Reload,
    /// It became a download or was stopped.
    Diverted,
}

pub struct PageNav {
    /// The tab's URL scheme for pages it proxies: they show as the page they stand for.
    proxy: String,
    current: String,
    load: Load,
    /// What the webview said last. Its URL is empty while it has no page (before its first,
    /// or once its first load failed or was stopped): WebKit then reloads nothing.
    native: NativeNav,
    page: PageState,
}

impl PageNav {
    /// A tab opening `current`, which proxies pages through its `proxy` scheme.
    pub fn new(current: String, proxy: String) -> Self {
        Self {
            proxy,
            current,
            load: Load::Idle,
            native: NativeNav::default(),
            page: PageState::Shown,
        }
    }

    /// The page shown (the address bar's text, Open in external browser).
    pub fn current(&self) -> &str {
        &self.current
    }

    /// Stop and the progress line instead of Reload.
    pub fn loading(&self) -> bool {
        !matches!(self.load, Load::Idle)
    }

    pub fn can_go_back(&self) -> bool {
        self.native.can_go_back
    }

    pub fn can_go_forward(&self) -> bool {
        self.native.can_go_forward
    }

    pub fn page(&self) -> &PageState {
        &self.page
    }

    pub fn reload(&self) -> Reload {
        match &self.page {
            PageState::Failed { url } => Reload::Retry(url.clone()),
            // A webview whose first load was stopped has nothing to reload: load the page.
            PageState::Shown if self.native.url.is_empty() => Reload::Retry(self.current.clone()),
            PageState::Shown => Reload::FromOrigin,
        }
    }

    pub fn apply(&mut self, input: NavInput) -> NavEffects {
        let mut effects = NavEffects::default();
        match input {
            NavInput::Native(native) => {
                let url = self.shown(native.url.clone());
                let loading = native.loading;
                let started = loading && !self.native.loading;
                let ended = !loading && self.native.loading;
                self.native = native;
                // A load the page or the history started. A failure goes with it: it may
                // be the load this one cancelled.
                if started && self.load == Load::Idle {
                    self.load = Load::Provisional {
                        url: url.clone(),
                        kind: Attempt::Load,
                    };
                    self.page = PageState::Shown;
                }
                match &mut self.load {
                    // The page shown stays until the load commits.
                    Load::Provisional { url: target, .. } => {
                        if loading {
                            target.clone_from(&url);
                        }
                    }
                    // `pushState`, `replaceState`, a fragment, Back / Forward within the page.
                    Load::Idle | Load::Committed => effects.navigated = self.navigate(url.clone()),
                }
                if ended {
                    if let Load::Provisional { url: target, kind } =
                        std::mem::replace(&mut self.load, Load::Idle)
                    {
                        // A load that failed leaves the webview on the page it showed.
                        match kind {
                            Attempt::Load if url == target => {
                                effects.navigated = self.navigate(url)
                            }
                            Attempt::Load | Attempt::Reload => {
                                self.page = PageState::Failed { url: target }
                            }
                            Attempt::Diverted => {}
                        }
                    }
                    effects.load_ended = true;
                }
            }
            NavInput::Requested(url) => {
                let url = self.shown(url);
                // WebKit loads the page shown again, unless only its fragment is asked for.
                let kind = if url == self.current && !url.contains('#') {
                    Attempt::Reload
                } else {
                    Attempt::Load
                };
                self.load = Load::Provisional { url, kind };
            }
            NavInput::Reloading => {
                self.load = Load::Provisional {
                    url: self.current.clone(),
                    kind: Attempt::Reload,
                };
            }
            NavInput::Committed(url) => {
                let url = self.shown(url);
                if self.load != Load::Idle {
                    self.load = Load::Committed;
                }
                self.page = PageState::Shown;
                if !url.is_empty() {
                    self.current = url;
                }
                effects.navigated = true;
            }
            NavInput::Downloaded => match &mut self.load {
                Load::Provisional { kind, .. } => *kind = Attempt::Diverted,
                // WebKit can end the load before it hands over the download.
                Load::Idle | Load::Committed => self.page = PageState::Shown,
            },
            NavInput::Stopped => {
                if !self.native.loading {
                    // The webview's reports all came before Stop: it loads nothing, so it will
                    // report no end (a reload WebKit ignored, a load it never started).
                    self.load = Load::Idle;
                } else if let Load::Provisional { kind, .. } = &mut self.load {
                    *kind = Attempt::Diverted;
                }
            }
        }
        effects
    }

    /// A proxied page shows (and is remembered) as the page it stands for.
    fn shown(&self, url: String) -> String {
        unproxied_url(&self.proxy, &url).unwrap_or(url)
    }

    /// Move within the page shown (or to it, before any commit was seen).
    fn navigate(&mut self, url: String) -> bool {
        if url.is_empty() || url == self.current {
            return false;
        }
        self.current = url;
        self.page = PageState::Shown;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROXY: &str = "treeterm-page1";
    const HOME: &str = "http://localhost:3000/";

    fn native(url: &str, loading: bool) -> NavInput {
        NavInput::Native(NativeNav {
            url: url.to_owned(),
            loading,
            can_go_back: false,
            can_go_forward: false,
        })
    }

    fn page(path: &str) -> String {
        format!("http://localhost:3000{path}")
    }

    fn tab() -> PageNav {
        PageNav::new(HOME.to_owned(), PROXY.to_owned())
    }

    fn failed(path: &str) -> PageState {
        PageState::Failed { url: page(path) }
    }

    const NOTHING: NavEffects = NavEffects {
        navigated: false,
        load_ended: false,
    };
    const NAVIGATED: NavEffects = NavEffects {
        navigated: true,
        load_ended: false,
    };
    const ENDED: NavEffects = NavEffects {
        navigated: false,
        load_ended: true,
    };

    #[test]
    fn a_load_shows_as_loading_from_its_request_and_moves_the_page_when_it_commits() {
        let mut nav = tab();
        assert_eq!(nav.apply(native(&page("/a"), true)), NOTHING);
        assert!(nav.loading());
        // Until the response comes, the bar keeps the page shown.
        assert_eq!(nav.current(), HOME);
        assert_eq!(nav.apply(native(&page("/b"), true)), NOTHING, "redirected");
        assert_eq!(nav.current(), HOME);
        assert_eq!(nav.apply(NavInput::Committed(page("/b"))), NAVIGATED);
        assert_eq!(nav.current(), page("/b"));
        assert!(nav.loading());
        assert_eq!(nav.apply(native(&page("/b"), false)), ENDED);
        assert!(!nav.loading());
        assert_eq!(nav.page(), &PageState::Shown);
        assert_eq!(nav.reload(), Reload::FromOrigin);
    }

    #[test]
    fn a_commit_is_a_navigation_even_to_the_same_page() {
        let mut nav = tab();
        nav.apply(native(HOME, true));
        assert_eq!(nav.apply(NavInput::Committed(HOME.to_owned())), NAVIGATED);
    }

    #[test]
    fn a_load_that_ends_without_a_page_failed() {
        let mut nav = tab();
        nav.apply(native(&page("/a"), true));
        // WKWebView's URL goes back to the page shown as the load fails.
        assert_eq!(nav.apply(native(HOME, false)), ENDED);
        assert_eq!(nav.page(), &failed("/a"));
        assert_eq!(
            nav.current(),
            HOME,
            "the bar keeps the page, as no did-navigate came"
        );
        assert_eq!(nav.reload(), Reload::Retry(page("/a")));
    }

    #[test]
    fn a_retry_shows_the_failure_until_a_page_comes_or_it_fails_again() {
        let mut nav = tab();
        nav.apply(native(&page("/a"), true));
        nav.apply(native(HOME, false));
        nav.apply(NavInput::Requested(page("/a")));
        nav.apply(native(&page("/a"), true));
        assert!(nav.loading());
        assert_eq!(nav.page(), &failed("/a"));
        nav.apply(native(HOME, false));
        assert_eq!(nav.page(), &failed("/a"));
        nav.apply(NavInput::Requested(page("/a")));
        nav.apply(native(&page("/a"), true));
        nav.apply(NavInput::Committed(page("/a")));
        assert_eq!(nav.page(), &PageState::Shown);
    }

    #[test]
    fn a_load_the_page_starts_clears_a_failure() {
        let mut nav = tab();
        nav.apply(native(&page("/a"), true));
        nav.apply(native(HOME, false));
        // A link clicked while /a was still on its way cancels it.
        nav.apply(native(&page("/b"), true));
        assert_eq!(nav.page(), &PageState::Shown);
    }

    #[test]
    fn the_first_page_failing_keeps_its_address() {
        let mut nav = tab();
        nav.apply(native(HOME, true));
        // Nothing committed yet: WKWebView has no URL left.
        assert_eq!(nav.apply(native("", false)), ENDED);
        assert_eq!(nav.page(), &failed("/"));
        assert_eq!(nav.current(), HOME);
    }

    #[test]
    fn a_reload_that_ends_without_a_page_failed() {
        let mut nav = tab();
        nav.apply(NavInput::Reloading);
        assert!(nav.loading());
        nav.apply(native(HOME, true));
        assert_eq!(nav.apply(native(HOME, false)), ENDED);
        assert_eq!(nav.page(), &failed("/"));
        // Loading the page shown again from the address bar is a reload too.
        let mut nav = tab();
        nav.apply(NavInput::Requested(HOME.to_owned()));
        nav.apply(native(HOME, true));
        nav.apply(native(HOME, false));
        assert_eq!(nav.page(), &failed("/"));
    }

    #[test]
    fn a_requested_fragment_is_a_navigation_within_the_page() {
        let mut nav = tab();
        nav.apply(NavInput::Requested(page("/#install")));
        nav.apply(native(&page("/#install"), true));
        assert_eq!(
            nav.apply(native(&page("/#install"), false)),
            NavEffects {
                navigated: true,
                load_ended: true
            }
        );
        assert_eq!(nav.current(), page("/#install"));
        assert_eq!(nav.page(), &PageState::Shown);
        // The same fragment again: nothing moves, and nothing failed.
        nav.apply(NavInput::Requested(page("/#install")));
        nav.apply(native(&page("/#install"), true));
        assert_eq!(nav.apply(native(&page("/#install"), false)), ENDED);
        assert_eq!(nav.page(), &PageState::Shown);
    }

    #[test]
    fn back_to_an_entry_of_the_page_shown_is_a_navigation_within_it() {
        let mut nav = PageNav::new(page("/app/b"), PROXY.to_owned());
        // WKWebView reports a history navigation it was asked for as a load.
        nav.apply(native(&page("/app/a"), true));
        assert_eq!(
            nav.apply(native(&page("/app/a"), false)),
            NavEffects {
                navigated: true,
                load_ended: true
            }
        );
        assert_eq!(nav.current(), page("/app/a"));
        assert_eq!(nav.page(), &PageState::Shown);
    }

    #[test]
    fn a_stopped_load_or_one_that_became_a_download_did_not_fail() {
        for input in [NavInput::Stopped, NavInput::Downloaded] {
            let mut nav = tab();
            nav.apply(native(&page("/a.md"), true));
            assert_eq!(nav.apply(input.clone()), NOTHING);
            assert_eq!(nav.apply(native(HOME, false)), ENDED, "{input:?}");
            assert_eq!(nav.page(), &PageState::Shown, "{input:?}");
        }
    }

    #[test]
    fn a_download_handed_over_after_its_load_ended_clears_the_failure() {
        let mut nav = tab();
        nav.apply(native(&page("/a.md"), true));
        nav.apply(native(HOME, false));
        assert_eq!(nav.apply(NavInput::Downloaded), NOTHING);
        assert_eq!(nav.page(), &PageState::Shown);
    }

    #[test]
    fn stop_once_the_page_committed_or_with_nothing_loading_changes_nothing() {
        let mut nav = tab();
        assert_eq!(nav.apply(NavInput::Stopped), NOTHING);
        assert!(!nav.loading());
        nav.apply(native(&page("/a"), true));
        nav.apply(NavInput::Committed(page("/a")));
        nav.apply(NavInput::Stopped);
        nav.apply(NavInput::Downloaded);
        assert_eq!(nav.apply(native(&page("/a"), false)), ENDED);
        assert_eq!(nav.page(), &PageState::Shown);
    }

    #[test]
    fn reload_after_the_first_load_was_stopped_loads_the_page() {
        let mut nav = tab();
        nav.apply(native(HOME, true));
        nav.apply(NavInput::Stopped);
        // WebKit has no page left, so its reload would do nothing.
        assert_eq!(nav.apply(native("", false)), ENDED);
        assert!(!nav.loading());
        assert_eq!(nav.page(), &PageState::Shown);
        assert_eq!(nav.reload(), Reload::Retry(HOME.to_owned()));
        nav.apply(NavInput::Requested(HOME.to_owned()));
        nav.apply(native(HOME, true));
        nav.apply(NavInput::Committed(HOME.to_owned()));
        assert_eq!(nav.apply(native(HOME, false)), ENDED);
        assert_eq!(nav.page(), &PageState::Shown);
        assert_eq!(nav.reload(), Reload::FromOrigin);
    }

    #[test]
    fn stop_ends_a_load_the_webview_never_started() {
        for input in [NavInput::Requested(page("/a")), NavInput::Reloading] {
            let mut nav = tab();
            nav.apply(native(HOME, false));
            nav.apply(input.clone());
            assert!(nav.loading(), "{input:?}");
            // The webview reports no end of a load it never started.
            assert_eq!(nav.apply(NavInput::Stopped), NOTHING, "{input:?}");
            assert!(!nav.loading(), "{input:?}");
            assert_eq!(nav.page(), &PageState::Shown, "{input:?}");
            assert_eq!(nav.current(), HOME, "{input:?}");
        }
    }

    #[test]
    fn in_page_navigations_move_the_page_while_idle_or_after_the_commit() {
        let mut nav = tab();
        assert_eq!(
            nav.apply(native(&page("/#top"), false)),
            NAVIGATED,
            "fragment"
        );
        assert_eq!(nav.current(), page("/#top"));
        assert_eq!(
            nav.apply(native(&page("/#top"), false)),
            NOTHING,
            "unchanged"
        );
        assert_eq!(nav.apply(native("", false)), NOTHING, "no URL");
        nav.apply(native(&page("/app"), true));
        nav.apply(NavInput::Committed(page("/app")));
        // A router replacing its state while the page still loads.
        assert_eq!(nav.apply(native(&page("/app/home"), true)), NAVIGATED);
        assert_eq!(nav.current(), page("/app/home"));
        assert_eq!(
            nav.apply(native(&page("/app/settings"), false)),
            NavEffects {
                navigated: true,
                load_ended: true
            },
            "pushState as the load ends"
        );
        assert_eq!(nav.current(), page("/app/settings"));
    }

    #[test]
    fn an_in_page_navigation_clears_a_failure() {
        let mut nav = tab();
        nav.apply(native(&page("/a"), true));
        nav.apply(native(HOME, false));
        // The page under the failure moving (its own script).
        assert_eq!(nav.apply(native(&page("/#x"), false)), NAVIGATED);
        assert_eq!(nav.page(), &PageState::Shown);
    }

    #[test]
    fn back_and_forward_follow_the_webview() {
        let mut nav = tab();
        assert!(!nav.can_go_back() && !nav.can_go_forward());
        let both = NativeNav {
            url: HOME.to_owned(),
            loading: false,
            can_go_back: true,
            can_go_forward: true,
        };
        nav.apply(NavInput::Native(both));
        assert!(nav.can_go_back() && nav.can_go_forward());
        let back = NativeNav {
            url: HOME.to_owned(),
            loading: false,
            can_go_back: true,
            can_go_forward: false,
        };
        nav.apply(NavInput::Native(back));
        assert!(nav.can_go_back() && !nav.can_go_forward());
    }

    #[test]
    fn a_requested_load_starts_anew_while_another_is_under_way() {
        let mut nav = tab();
        nav.apply(native(&page("/slow"), true));
        nav.apply(NavInput::Committed(page("/slow")));
        // Home pressed while the page still loads: WKWebView stays loading throughout.
        nav.apply(NavInput::Requested(HOME.to_owned()));
        assert_eq!(
            nav.apply(native(HOME, true)),
            NOTHING,
            "not moved before it commits"
        );
        assert_eq!(nav.current(), page("/slow"));
        nav.apply(native(&page("/slow"), false));
        assert_eq!(nav.page(), &failed("/"));
    }

    #[test]
    fn a_requested_load_shows_as_loading_at_once() {
        let mut nav = tab();
        nav.apply(NavInput::Requested(page("/a")));
        assert!(nav.loading());
        // The webview's own report follows.
        assert_eq!(nav.apply(native(&page("/a"), true)), NOTHING);
        nav.apply(NavInput::Committed(page("/a")));
        assert_eq!(nav.apply(native(&page("/a"), false)), ENDED);
        assert_eq!(nav.page(), &PageState::Shown);
    }

    #[test]
    fn proxied_pages_show_as_the_page_they_stand_for() {
        let mut nav = tab();
        let proxied = |path: &str| format!("{PROXY}://http.localhost:3000{path}");
        nav.apply(NavInput::Requested(proxied("/a.md")));
        nav.apply(native(&proxied("/a.md"), true));
        nav.apply(NavInput::Committed(proxied("/a.md")));
        assert_eq!(nav.current(), page("/a.md"));
        assert_eq!(nav.apply(native(&proxied("/a.md"), false)), ENDED);
        nav.apply(native(&proxied("/b.md"), true));
        nav.apply(native(&proxied("/a.md"), false));
        assert_eq!(nav.page(), &failed("/b.md"));
    }
}
