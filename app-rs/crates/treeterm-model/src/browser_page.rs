//! A Browser tab's page (`applications/browser/renderer.ts`): a localhost page, shown as
//! it is on this machine and through an ephemeral ssh forward owned by the tab on an ssh
//! session (the page is on the ssh host's localhost).

use gpui_kit::{Context, EventEmitter, Task};

use crate::apps::{forwarded_url, page_url, target_port};
use crate::web_app::{RemoteHost, WebAppEvent, ensure_forward, forward_id_for_tab};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserPhase {
    Forwarding { port: u16 },
    /// `url` opens the page the tab remembers; `home` is the start page (Home). On an ssh
    /// session both go through the forward.
    Ready { url: String, home: String },
    Error(String),
}

/// [`WebAppEvent::LocalPortAssigned`]: the forward's local port, kept in the tab's state so
/// the page's origin survives restarts.
impl EventEmitter<WebAppEvent> for BrowserPageModel {}

pub struct BrowserPageModel {
    /// The instance's start page.
    home: String,
    /// The path the tab remembers (`path`).
    path: String,
    tab_id: String,
    local_port: Option<u16>,
    remote: Option<RemoteHost>,
    phase: BrowserPhase,
    _run: Option<Task<()>>,
}

impl BrowserPageModel {
    pub fn new(
        home: String,
        path: String,
        tab_id: String,
        local_port: Option<u16>,
        remote: Option<RemoteHost>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut model = Self {
            home,
            path,
            tab_id,
            local_port,
            remote,
            phase: BrowserPhase::Forwarding { port: 0 },
            _run: None,
        };
        model.start(cx);
        model
    }

    pub fn phase(&self) -> &BrowserPhase {
        &self.phase
    }

    /// Run the start-up again (after an error).
    pub fn retry(&mut self, cx: &mut Context<Self>) {
        self.start(cx);
    }

    /// "Restart" on the Port Forward sub-tab: a new forward, then the page again.
    pub fn restart_forward(&mut self, cx: &mut Context<Self>) {
        self.remove_forward(cx);
        self.start(cx);
    }

    /// The tab closed: drop its forward.
    pub fn close(&mut self, cx: &mut Context<Self>) {
        self._run = None;
        self.remove_forward(cx);
    }

    fn remove_forward(&self, cx: &mut Context<Self>) {
        if let Some(remote) = &self.remote
            && let Some(app) = remote.app.upgrade()
        {
            app.read(cx).manager().remove_port_forward(&forward_id_for_tab(&self.tab_id));
        }
    }

    fn start(&mut self, cx: &mut Context<Self>) {
        let url = page_url(&self.home, &self.path);
        let Some(remote) = self.remote.clone() else {
            self.phase = BrowserPhase::Ready { url, home: self.home.clone() };
            cx.notify();
            return;
        };
        let port = match target_port(&self.home) {
            Ok(port) => port,
            Err(error) => {
                self.phase = BrowserPhase::Error(error);
                cx.notify();
                return;
            }
        };
        self.phase = BrowserPhase::Forwarding { port };
        cx.notify();
        let (tab_id, preferred, home) = (self.tab_id.clone(), self.local_port, self.home.clone());
        let executor = cx.background_executor().clone();
        self._run = Some(cx.spawn(async move |this, cx| {
            let ready = async {
                let local = ensure_forward(&remote, &tab_id, port, preferred, &executor, cx).await?;
                Ok::<_, String>((local, forwarded_url(&url, local)?, forwarded_url(&home, local)?))
            }
            .await;
            match this.update(cx, |model, cx| {
                model.phase = match ready {
                    Ok((local, url, home)) => {
                        model.local_port = Some(local);
                        cx.emit(WebAppEvent::LocalPortAssigned(local));
                        BrowserPhase::Ready { url, home }
                    }
                    Err(message) => BrowserPhase::Error(message),
                };
                cx.notify();
            }) {
                Ok(()) | Err(_) => {} // Err: the tab closed
            }
        }));
    }
}
