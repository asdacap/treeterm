//! A Browser tab's navigation, followed from its webview's own state as `PageViews.tsx`
//! follows `did-navigate`, `did-navigate-in-page`, `did-start-loading` and
//! `did-stop-loading`: the page shown (also after `pushState` and fragment changes), whether
//! a load is under way (from the request, not the response), Back / Forward from the
//! webview's history, and a load that failed before any page came (Electron's webview shows
//! a blank error page then; WKWebView shows nothing).

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

/// A load that ended before any page came, as the webview reports it (WKWebView's
/// `didFailProvisionalNavigation`, its `NSError`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadFailure {
    /// The URL that could not be loaded (`NSURLErrorFailingURLErrorKey`); empty when
    /// the error does not say.
    pub url: String,
    /// The error's domain and code: `NSURLErrorDomain` -1004 for a refused connection.
    pub domain: String,
    pub code: i64,
}

/// `NSURLErrorCancelled`: the load was stopped, or another replaced it.
const URL_ERROR_CANCELLED: (&str, i64) = ("NSURLErrorDomain", -999);
/// `WebKitErrorFrameLoadInterruptedByPolicyChange`: the page became a download, or the tab
/// refused to go there.
const FRAME_LOAD_INTERRUPTED: (&str, i64) = ("WebKitErrorDomain", 102);

impl LoadFailure {
    /// A load stopped, replaced or turned into a download: Chromium's `ERR_ABORTED`, which
    /// Electron reports as no failure (`did-fail-load`) and shows no error page for.
    pub fn aborted(&self) -> bool {
        [URL_ERROR_CANCELLED, FRAME_LOAD_INTERRUPTED].contains(&(self.domain.as_str(), self.code))
    }
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
    /// A load of a new page started, whoever started it (the tab, the page, the history),
    /// at this URL (`didStartProvisionalNavigation`). WebKit reports its URL and loading state
    /// ([`NavInput::Native`]) just before.
    Started(String),
    /// The page being loaded replaced the one shown, at this URL (wry's `Started`, sent from
    /// `didCommitNavigation`).
    Committed(String),
    /// The load under way ended before any page came.
    Failed(LoadFailure),
    /// Stop was pressed. With nothing loading in the webview (WebKit ignores a reload without
    /// a page), the load ends at once, as no end will come.
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
    /// The last load failed before any page came (server down, connection refused): shown
    /// in the error page's place until a page comes, or the page under it moves.
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
    },
    /// Its page replaced the one shown; it loads the rest.
    Committed {
        /// The page shown before the webview's last report moved it while this load
        /// continued: a navigation within the page, unless a new load turns out to have
        /// started there ([`NavInput::Started`], reported after its URL).
        moved_from: Option<String>,
    },
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
                // A load the page or the history started (also one within the page, which
                // WebKit reports as a load when asked for it).
                if started && self.load == Load::Idle {
                    self.load = Load::Provisional { url: url.clone() };
                }
                match &mut self.load {
                    // The page shown stays until the load commits.
                    Load::Provisional { url: target } => {
                        if loading {
                            target.clone_from(&url);
                        }
                    }
                    // `pushState`, `replaceState`, a fragment, Back / Forward within the page.
                    Load::Idle => effects.navigated = self.navigate(url.clone()),
                    Load::Committed { .. } => {
                        let before = self.current.clone();
                        effects.navigated = self.navigate(url.clone());
                        if effects.navigated {
                            self.load = Load::Committed {
                                moved_from: Some(before),
                            };
                        }
                    }
                }
                if ended {
                    // A load that got to its URL without a new page moved within the page
                    // shown; one that failed is reported as such ([`NavInput::Failed`]).
                    if let Load::Provisional { url: target } =
                        std::mem::replace(&mut self.load, Load::Idle)
                        && url == target
                    {
                        effects.navigated = self.navigate(url);
                    }
                    effects.load_ended = true;
                }
            }
            NavInput::Requested(url) => {
                self.load = Load::Provisional {
                    url: self.shown(url),
                };
            }
            NavInput::Reloading => {
                self.load = Load::Provisional {
                    url: self.current.clone(),
                };
            }
            NavInput::Started(url) => {
                let url = self.shown(url);
                // Its URL, reported first, moved the page as if within it: it stays.
                if let Load::Committed {
                    moved_from: Some(before),
                } = &self.load
                    && self.current == url
                {
                    self.current = before.clone();
                    effects.navigated = true;
                }
                self.load = Load::Provisional { url };
            }
            NavInput::Committed(url) => {
                let url = self.shown(url);
                if self.load != Load::Idle {
                    self.load = Load::Committed { moved_from: None };
                }
                self.page = PageState::Shown;
                if !url.is_empty() {
                    self.current = url;
                }
                effects.navigated = true;
            }
            NavInput::Failed(failure) => {
                // An aborted load ends as any other: the webview reports its end.
                if !failure.aborted() {
                    let load = std::mem::replace(&mut self.load, Load::Idle);
                    let url = if failure.url.is_empty() {
                        match load {
                            Load::Provisional { url } => url,
                            Load::Idle | Load::Committed { .. } => self.current.clone(),
                        }
                    } else {
                        self.shown(failure.url)
                    };
                    self.page = PageState::Failed { url };
                }
            }
            NavInput::Stopped => {
                if !self.native.loading {
                    // The webview's reports all came before Stop: it loads nothing, so it will
                    // report no end (a reload WebKit ignored, a load it never started).
                    self.load = Load::Idle;
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

    fn error(url: &str, domain: &str, code: i64) -> NavInput {
        NavInput::Failed(LoadFailure {
            url: url.to_owned(),
            domain: domain.to_owned(),
            code,
        })
    }

    /// `NSURLErrorCannotConnectToHost`: the server is down.
    fn refused(url: &str) -> NavInput {
        error(url, "NSURLErrorDomain", -1004)
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
        assert_eq!(nav.apply(NavInput::Started(page("/a"))), NOTHING);
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
    fn a_load_that_fails_shows_its_failure() {
        let mut nav = tab();
        nav.apply(native(&page("/a"), true));
        nav.apply(NavInput::Started(page("/a")));
        assert_eq!(nav.apply(refused(&page("/a"))), NOTHING);
        assert_eq!(nav.page(), &failed("/a"));
        assert!(!nav.loading());
        // WKWebView's URL goes back to the page shown as the load ends.
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
    fn a_failure_reported_after_the_load_ended_shows_too() {
        let mut nav = tab();
        nav.apply(native(&page("/a"), true));
        assert_eq!(nav.apply(native(HOME, false)), ENDED);
        assert_eq!(nav.page(), &PageState::Shown);
        nav.apply(refused(&page("/a")));
        assert_eq!(nav.page(), &failed("/a"));
    }

    #[test]
    fn a_failure_without_its_url_is_the_load_under_ways() {
        let mut nav = tab();
        nav.apply(NavInput::Requested(page("/a")));
        nav.apply(refused(""));
        assert_eq!(nav.page(), &failed("/a"));
        // Reported once the load ended: the page shown.
        let mut nav = tab();
        nav.apply(refused(""));
        assert_eq!(nav.page(), &failed("/"));
    }

    #[test]
    fn a_load_that_ends_without_a_page_or_a_failure_did_not_fail() {
        let mut nav = tab();
        nav.apply(native(&page("/a"), true));
        assert_eq!(nav.apply(native(HOME, false)), ENDED);
        assert_eq!(nav.page(), &PageState::Shown);
        assert_eq!(nav.current(), HOME);
    }

    #[test]
    fn a_stopped_load_or_one_that_became_a_download_did_not_fail() {
        for aborted in [
            error(&page("/a.md"), "NSURLErrorDomain", -999),
            error(&page("/a.md"), "WebKitErrorDomain", 102),
        ] {
            let mut nav = tab();
            nav.apply(native(&page("/a.md"), true));
            nav.apply(NavInput::Started(page("/a.md")));
            assert_eq!(nav.apply(aborted.clone()), NOTHING);
            assert!(
                nav.loading(),
                "{aborted:?}: it ends with the webview's load"
            );
            assert_eq!(nav.apply(native(HOME, false)), ENDED, "{aborted:?}");
            assert_eq!(nav.page(), &PageState::Shown, "{aborted:?}");
        }
    }

    #[test]
    fn only_a_cancelled_load_or_an_interrupted_frame_load_is_aborted() {
        let failure = |domain: &str, code| LoadFailure {
            url: String::new(),
            domain: domain.to_owned(),
            code,
        };
        assert!(failure("NSURLErrorDomain", -999).aborted());
        assert!(failure("WebKitErrorDomain", 102).aborted());
        assert!(!failure("NSURLErrorDomain", -1004).aborted());
        assert!(!failure("NSURLErrorDomain", 102).aborted());
        assert!(!failure("WebKitErrorDomain", -999).aborted());
    }

    #[test]
    fn a_retry_shows_the_failure_until_a_page_comes_or_it_fails_again() {
        let mut nav = tab();
        nav.apply(native(&page("/a"), true));
        nav.apply(refused(&page("/a")));
        nav.apply(native(HOME, false));
        nav.apply(NavInput::Requested(page("/a")));
        nav.apply(native(&page("/a"), true));
        nav.apply(NavInput::Started(page("/a")));
        assert!(nav.loading());
        assert_eq!(nav.page(), &failed("/a"));
        nav.apply(refused(&page("/a")));
        nav.apply(native(HOME, false));
        assert_eq!(nav.page(), &failed("/a"));
        nav.apply(NavInput::Requested(page("/a")));
        nav.apply(native(&page("/a"), true));
        nav.apply(NavInput::Committed(page("/a")));
        assert_eq!(nav.page(), &PageState::Shown);
    }

    #[test]
    fn a_failure_stays_until_the_next_page_comes() {
        let mut nav = tab();
        nav.apply(native(&page("/a"), true));
        nav.apply(refused(&page("/a")));
        nav.apply(native(HOME, false));
        // A load the page under the failure starts (Electron's blank error page stays until
        // the next page commits).
        nav.apply(native(&page("/b"), true));
        nav.apply(NavInput::Started(page("/b")));
        assert_eq!(nav.page(), &failed("/a"));
        nav.apply(NavInput::Committed(page("/b")));
        assert_eq!(nav.page(), &PageState::Shown);
    }

    #[test]
    fn the_first_page_failing_keeps_its_address() {
        let mut nav = tab();
        nav.apply(native(HOME, true));
        nav.apply(NavInput::Started(HOME.to_owned()));
        nav.apply(refused(HOME));
        // Nothing committed yet: WKWebView has no URL left.
        assert_eq!(nav.apply(native("", false)), ENDED);
        assert_eq!(nav.page(), &failed("/"));
        assert_eq!(nav.current(), HOME);
    }

    #[test]
    fn a_reload_that_fails_shows_its_failure() {
        let mut nav = tab();
        nav.apply(NavInput::Reloading);
        assert!(nav.loading());
        nav.apply(native(HOME, true));
        nav.apply(NavInput::Started(HOME.to_owned()));
        nav.apply(refused(HOME));
        assert_eq!(nav.apply(native(HOME, false)), ENDED);
        assert_eq!(nav.page(), &failed("/"));
        assert_eq!(nav.current(), HOME);
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
    fn stop_once_the_page_committed_or_with_nothing_loading_changes_nothing() {
        let mut nav = tab();
        assert_eq!(nav.apply(NavInput::Stopped), NOTHING);
        assert!(!nav.loading());
        nav.apply(native(&page("/a"), true));
        nav.apply(NavInput::Committed(page("/a")));
        nav.apply(NavInput::Stopped);
        assert!(nav.loading(), "it ends with the webview's load");
        assert_eq!(nav.apply(native(&page("/a"), false)), ENDED);
        assert_eq!(nav.page(), &PageState::Shown);
    }

    #[test]
    fn reload_after_the_first_load_was_stopped_loads_the_page() {
        let mut nav = tab();
        nav.apply(native(HOME, true));
        nav.apply(NavInput::Stopped);
        nav.apply(error(HOME, "NSURLErrorDomain", -999));
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
    fn a_load_the_page_starts_while_it_still_loads_keeps_the_page_until_it_commits() {
        let mut nav = tab();
        nav.apply(native(&page("/slow"), true));
        nav.apply(NavInput::Committed(page("/slow")));
        // A link followed while the page still loads: WKWebView stays loading throughout,
        // and reports the link's URL before the load it starts.
        nav.apply(native(&page("/router"), true));
        assert_eq!(nav.current(), page("/router"), "pushState first");
        assert_eq!(nav.apply(native(&page("/dead"), true)), NAVIGATED);
        assert_eq!(
            nav.apply(NavInput::Started(page("/dead"))),
            NAVIGATED,
            "back to the page shown"
        );
        assert_eq!(nav.current(), page("/router"));
        assert!(nav.loading());
        nav.apply(refused(&page("/dead")));
        assert_eq!(nav.page(), &failed("/dead"));
        assert_eq!(nav.apply(native(&page("/router"), false)), ENDED);
        assert_eq!(nav.page(), &failed("/dead"));
        assert_eq!(nav.current(), page("/router"));
        assert_eq!(nav.reload(), Reload::Retry(page("/dead")));
    }

    #[test]
    fn a_load_started_without_a_move_keeps_the_page() {
        let mut nav = tab();
        nav.apply(native(&page("/slow"), true));
        nav.apply(NavInput::Committed(page("/slow")));
        // `location.reload()` while the page loads: its URL does not change.
        assert_eq!(nav.apply(NavInput::Started(page("/slow"))), NOTHING);
        assert_eq!(nav.current(), page("/slow"));
        assert!(nav.loading());
    }

    #[test]
    fn an_in_page_navigation_clears_a_failure() {
        let mut nav = tab();
        nav.apply(native(&page("/a"), true));
        nav.apply(refused(&page("/a")));
        nav.apply(native(HOME, false));
        // The page under the failure moving (its own script, Back within it).
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
        assert_eq!(nav.apply(NavInput::Started(HOME.to_owned())), NOTHING);
        assert_eq!(nav.current(), page("/slow"));
        nav.apply(refused(HOME));
        nav.apply(native(&page("/slow"), false));
        assert_eq!(nav.page(), &failed("/"));
        assert_eq!(nav.current(), page("/slow"));
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
        nav.apply(NavInput::Started(proxied("/b.md")));
        nav.apply(refused(&proxied("/b.md")));
        nav.apply(native(&proxied("/a.md"), false));
        assert_eq!(nav.page(), &failed("/b.md"));
    }
}
