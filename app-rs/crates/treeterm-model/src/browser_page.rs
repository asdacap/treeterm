//! A Browser tab's page (`applications/browser/renderer.ts`): a localhost page, shown as
//! it is on this machine and through an ephemeral ssh forward owned by the tab on an ssh
//! session (the page is on the ssh host's localhost).

use gpui_kit::{Context, EventEmitter, Task};
use serde_json::Value;

use crate::apps::{forwarded_url, parse_local_url, target_port};
use crate::web_app::{RemoteHost, WebAppEvent, ensure_forward, forward_id_for_tab, saved_port};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserPhase {
    Forwarding { port: u16 },
    /// `home` is the start page (Home), through the forward on an ssh session. The page
    /// opens on the one the tab remembers (`WorkspaceStore::page_url`).
    Ready { home: String },
    Error(String),
}

/// [`WebAppEvent::LocalPortAssigned`]: the forward's local port, kept in the tab's state so
/// the page's origin survives restarts.
impl EventEmitter<WebAppEvent> for BrowserPageModel {}

pub struct BrowserPageModel {
    /// The instance's start page.
    home: String,
    tab_id: String,
    local_port: Option<u16>,
    remote: Option<RemoteHost>,
    phase: BrowserPhase,
    /// Bumped each time the page becomes Ready from another phase: Electron mounts its
    /// `WebView` only while Ready, so the page opens anew then; Ready again keeps it.
    mount: u64,
    _run: Option<Task<()>>,
}

impl BrowserPageModel {
    pub fn new(home: String, tab_id: String, local_port: Option<u16>, remote: Option<RemoteHost>, cx: &mut Context<Self>) -> Self {
        let mut model = Self {
            home,
            tab_id,
            local_port,
            remote,
            phase: BrowserPhase::Forwarding { port: 0 },
            mount: 0,
            _run: None,
        };
        model.start(cx);
        model
    }

    pub fn phase(&self) -> &BrowserPhase {
        &self.phase
    }

    /// Which opening of the page this is: a new number wants a fresh page.
    pub fn page_mount(&self) -> u64 {
        self.mount
    }

    /// The local end of the ssh forward the next start tries first.
    pub fn local_port(&self) -> Option<u16> {
        self.local_port
    }

    /// The tab's state as it is now, another window's `localPort` included: Electron reads
    /// it at each start.
    pub fn follow_state(&mut self, state: &Value) {
        self.local_port = saved_port(state, "localPort");
    }

    fn set_phase(&mut self, phase: BrowserPhase, cx: &mut Context<Self>) {
        if matches!(phase, BrowserPhase::Ready { .. }) && !matches!(self.phase, BrowserPhase::Ready { .. }) {
            self.mount += 1;
        }
        self.phase = phase;
        cx.notify();
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
        // Checked before anything else, on this machine too: a start page that is not on
        // localhost is an error with Retry, not a page that cannot be shown.
        let home = match parse_local_url(&self.home) {
            Ok(home) => home,
            Err(error) => return self.set_phase(BrowserPhase::Error(error), cx),
        };
        let port = target_port(&home);
        let home = String::from(home);
        let Some(remote) = self.remote.clone() else {
            return self.set_phase(BrowserPhase::Ready { home }, cx);
        };
        self.set_phase(BrowserPhase::Forwarding { port }, cx);
        let (tab_id, preferred) = (self.tab_id.clone(), self.local_port);
        let executor = cx.background_executor().clone();
        self._run = Some(cx.spawn(async move |this, cx| {
            let ready = async {
                let local = ensure_forward(&remote, &tab_id, port, preferred, &executor, cx).await?;
                Ok::<_, String>((local, forwarded_url(&home, local)?))
            }
            .await;
            match this.update(cx, |model, cx| {
                let phase = match ready {
                    Ok((local, home)) => {
                        model.local_port = Some(local);
                        cx.emit(WebAppEvent::LocalPortAssigned(local));
                        BrowserPhase::Ready { home }
                    }
                    Err(message) => BrowserPhase::Error(message),
                };
                model.set_phase(phase, cx);
            }) {
                Ok(()) | Err(_) => {} // Err: the tab closed
            }
        }));
    }
}
