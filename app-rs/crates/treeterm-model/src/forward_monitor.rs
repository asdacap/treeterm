//! A tab's own ssh port forward as its Port Forward sub-tab shows it (`PortForwardView` /
//! `ForwardLog` in `PageViews.tsx`): the forward's status and its ssh output, from the
//! scrollback on.
//!
//! The forward is created by the tab's page (Web App or Browser) once it starts, and
//! replaced by Restart; the monitor follows its page and watches again whenever the forward
//! it watched is gone.

use futures::StreamExt as _;
use gpui_kit::{Context, Entity, Subscription, Task, WeakEntity};
use treeterm_core::connection::{PortForwardId, PortForwardInfo, PortForwardStatus};

use crate::app_store::AppStore;

/// Lines of ssh output kept.
const MAX_LINES: usize = 1000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForwardState {
    /// Not started yet (or replaced and not back yet).
    Missing,
    Live {
        info: Box<PortForwardInfo>,
        /// The ssh process's output; a new process (status back to Connecting) starts over.
        log: Vec<String>,
    },
}

pub struct ForwardMonitor {
    app: WeakEntity<AppStore>,
    id: PortForwardId,
    state: ForwardState,
    _status: Option<Task<()>>,
    _output: Option<Task<()>>,
    _page: Subscription,
}

impl ForwardMonitor {
    /// Watches `id`, trying again whenever `page` (the tab's page, which owns the forward)
    /// changes while there is nothing to watch.
    pub fn new<T: 'static>(app: WeakEntity<AppStore>, id: PortForwardId, page: &Entity<T>, cx: &mut Context<Self>) -> Self {
        let observe = cx.observe(page, |monitor, _, cx| monitor.watch(cx));
        let mut monitor = Self {
            app,
            id,
            state: ForwardState::Missing,
            _status: None,
            _output: None,
            _page: observe,
        };
        monitor.watch(cx);
        monitor
    }

    pub fn state(&self) -> &ForwardState {
        &self.state
    }

    /// Start watching the forward if it exists and is not watched yet.
    fn watch(&mut self, cx: &mut Context<Self>) {
        if let ForwardState::Live { .. } = self.state {
            return;
        }
        let Some(app) = self.app.upgrade() else {
            return;
        };
        let Ok(watch) = app.read(cx).manager().watch_port_forward_status(&self.id) else {
            return; // not created yet: the page notifies once it is
        };
        self.state = ForwardState::Live { info: Box::new(watch.initial), log: Vec::new() };
        let mut updates = watch.updates;
        self._status = Some(cx.spawn(async move |this, cx| {
            while let Some(info) = updates.next().await {
                if this.update(cx, |monitor, cx| monitor.on_status(info, cx)).is_err() {
                    return;
                }
            }
            // The forward was removed (closed or restarted).
            match this.update(cx, |monitor, cx| {
                monitor.state = ForwardState::Missing;
                monitor._output = None;
                cx.notify();
                monitor.watch(cx);
            }) {
                Ok(()) | Err(_) => {} // Err: the tab closed
            }
        }));
        self.watch_output(cx);
        cx.notify();
    }

    fn on_status(&mut self, next: PortForwardInfo, cx: &mut Context<Self>) {
        let ForwardState::Live { info, .. } = &mut self.state else {
            return;
        };
        let restarted = next.status == PortForwardStatus::Connecting && info.status != PortForwardStatus::Connecting;
        **info = next;
        if restarted {
            self.watch_output(cx);
        }
        cx.notify();
    }

    /// Follow the ssh output from its scrollback.
    fn watch_output(&mut self, cx: &mut Context<Self>) {
        let Some(app) = self.app.upgrade() else {
            return;
        };
        let watch = app.read(cx).manager().watch_port_forward_output(&self.id);
        let ForwardState::Live { log, .. } = &mut self.state else {
            return;
        };
        match watch {
            Ok(watch) => {
                *log = watch.initial;
                let mut lines = watch.updates;
                self._output = Some(cx.spawn(async move |this, cx| {
                    while let Some(line) = lines.next().await {
                        let pushed = this.update(cx, |monitor, cx| {
                            if let ForwardState::Live { log, .. } = &mut monitor.state {
                                log.push(line);
                                let excess = log.len().saturating_sub(MAX_LINES);
                                log.drain(..excess);
                                cx.notify();
                            }
                        });
                        if pushed.is_err() {
                            return;
                        }
                    }
                }));
            }
            Err(error) => {
                *log = vec![format!("Failed to watch output: {error}")];
                self._output = None;
            }
        }
    }
}
