//! One connection's life as a sans-IO state machine (the logic of Electron's `Connection`
//! class in `connectionManager.ts`).
//!
//! The machine decides; it never waits or does I/O. Inputs carry the current time. It
//! queues [`Effect`]s for its driver (start an attempt, drop the transport, watch heartbeats,
//! spawn/kill port forwards) and [`MachineEvent`]s for watchers, and says when it next needs
//! [`ConnectionMachine::handle_timeout`]. Tests drive it with made-up instants.
//!
//! Reconnect policy (Electron's): a connection is lost when its session watch fails, its ssh
//! tunnel closes, or the watch stays silent for [`HEARTBEAT_TIMEOUT`] (the daemon broadcasts
//! every 15 s). Attempt `n + 1` starts `1 s · 2ⁿ` (at most [`RECONNECT_MAX_DELAY`]) after the
//! loss or after attempt `n` failed, with no limit on attempts. "Reconnect now" skips the
//! wait, "cancel" stops the loop (the connection shows its last error), "force" starts a loop
//! on a healthy connection. Port forwards are restarted after every successful reconnect.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use indexmap::IndexMap;

use crate::settings::SshConnectionConfig;

use super::port_forward::{ForwardEffect, ForwardEvent, PortForward};
use super::process::{ProcessCommand, ProcessEvent};
use super::ssh::TunnelEnd;
use super::types::{
    ConnectPhase, ConnectionError, ConnectionId, ConnectionInfo, ConnectionStatus,
    ConnectionTarget, OutdatedDaemon, PortForwardConfig, PortForwardId, PortForwardInfo,
};

/// Silence on the session watch after which the connection counts as lost.
pub const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(50);
/// Wait before the first reconnect attempt; doubles after each failure.
pub const RECONNECT_BASE_DELAY: Duration = Duration::from_secs(1);
pub const RECONNECT_MAX_DELAY: Duration = Duration::from_secs(30);

/// Wait before the next attempt when `attempts` have already been made since the loss.
pub fn reconnect_delay(attempts: u32) -> Duration {
    let factor = 1u32.checked_shl(attempts).unwrap_or(u32::MAX);
    RECONNECT_BASE_DELAY
        .checked_mul(factor)
        .map_or(RECONNECT_MAX_DELAY, |delay| delay.min(RECONNECT_MAX_DELAY))
}

/// Identifies one connect attempt, so a late result can be told from the current one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AttemptId(u64);

/// What an attempt connects to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttemptTarget {
    /// Start the local daemon if needed and connect.
    Local,
    /// Bootstrap over ssh, tunnel, connect through the tunnel.
    Remote {
        config: Arc<SshConnectionConfig>,
        outdated: OutdatedDaemon,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttemptPlan {
    pub id: AttemptId,
    pub target: AttemptTarget,
}

/// Work for the driver, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Start connecting, replacing (cancelling) any attempt in flight. Report the result
    /// with [`ConnectionMachine::attempt_finished`].
    StartAttempt(AttemptPlan),
    /// Abandon the attempt in flight; its result will never come.
    CancelAttempt,
    /// Drop the current transport (daemon connection and tunnel) and its heartbeat watch.
    DropTransport,
    /// Watch the new transport's session as heartbeat; report through
    /// [`ConnectionMachine::heartbeat`] and [`ConnectionMachine::heartbeat_failed`].
    WatchHeartbeat,
    /// Start a forward's ssh, replacing any previous one for the forward.
    SpawnForward {
        id: PortForwardId,
        generation: u64,
        command: ProcessCommand,
    },
    KillForward {
        id: PortForwardId,
    },
}

/// News for watchers.
#[derive(Debug, Clone)]
pub enum MachineEvent<C> {
    Status(ConnectionInfo<C>),
    ForwardStatus(PortForwardInfo),
    ForwardOutput {
        id: PortForwardId,
        line: String,
    },
    /// The forward is gone; its watchers can be dropped.
    ForwardRemoved(PortForwardId),
}

/// Result of [`ConnectionMachine::attempt_finished`].
#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttemptVerdict {
    /// The result was applied; on success the driver keeps the transport.
    Applied,
    /// Not the attempt in flight (cancelled or replaced): drop its transport.
    Stale,
}

/// The connection is not remote.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("Connection is not remote: {0}")]
pub struct NotRemote(pub ConnectionId);

#[derive(Debug, Clone)]
enum State<C> {
    Connecting {
        phase: ConnectPhase,
        attempt: AttemptId,
    },
    Connected {
        daemon: C,
        heartbeat_deadline: Instant,
    },
    Reconnecting {
        /// Attempts started since the connection was lost.
        attempts: u32,
        error: ConnectionError,
        step: Step,
    },
    Failed {
        error: ConnectionError,
    },
    Disconnected,
}

#[derive(Debug, Clone, Copy)]
enum Step {
    /// Backing off until the next attempt.
    Waiting {
        until: Instant,
    },
    InFlight(AttemptId),
}

pub struct ConnectionMachine<C> {
    id: ConnectionId,
    target: ConnectionTarget,
    /// "Connect anyway" survives reconnects; "refresh" never replays (it would kill the
    /// remote's processes again).
    reconnect_outdated: OutdatedDaemon,
    state: State<C>,
    next_attempt: u64,
    forwards: IndexMap<PortForwardId, PortForward>,
    effects: VecDeque<Effect>,
    events: VecDeque<MachineEvent<C>>,
}

impl<C: Clone> ConnectionMachine<C> {
    /// A connection whose first attempt starts now.
    pub fn start(id: ConnectionId, target: ConnectionTarget, outdated: OutdatedDaemon) -> Self {
        let phase = match target {
            ConnectionTarget::Local => ConnectPhase::Daemon,
            ConnectionTarget::Remote(_) => ConnectPhase::Bootstrap,
        };
        let reconnect_outdated = match outdated {
            OutdatedDaemon::Accept => OutdatedDaemon::Accept,
            OutdatedDaemon::Report | OutdatedDaemon::Refresh => OutdatedDaemon::Report,
        };
        let mut machine = Self {
            id,
            target,
            reconnect_outdated,
            state: State::Disconnected,
            next_attempt: 0,
            forwards: IndexMap::new(),
            effects: VecDeque::new(),
            events: VecDeque::new(),
        };
        let attempt = machine.begin_attempt(outdated);
        machine.state = State::Connecting { phase, attempt };
        machine.emit_status();
        machine
    }

    pub fn id(&self) -> &ConnectionId {
        &self.id
    }

    pub fn target(&self) -> &ConnectionTarget {
        &self.target
    }

    pub fn status(&self) -> ConnectionStatus<C> {
        match &self.state {
            State::Connecting { phase, .. } => ConnectionStatus::Connecting { phase: *phase },
            State::Connected { daemon, .. } => ConnectionStatus::Connected {
                daemon: daemon.clone(),
            },
            State::Reconnecting {
                attempts, error, ..
            } => ConnectionStatus::Reconnecting {
                attempt: *attempts,
                error: error.clone(),
            },
            State::Failed { error } => ConnectionStatus::Error {
                error: error.clone(),
            },
            State::Disconnected => ConnectionStatus::Disconnected,
        }
    }

    pub fn info(&self) -> ConnectionInfo<C> {
        ConnectionInfo {
            id: self.id.clone(),
            target: self.target.clone(),
            status: self.status(),
        }
    }

    // --- attempts ----------------------------------------------------------------------------

    /// The first attempt reached `phase`.
    pub fn connect_phase(&mut self, attempt: AttemptId, phase: ConnectPhase) {
        if let State::Connecting {
            phase: current,
            attempt: in_flight,
        } = &mut self.state
            && *in_flight == attempt
            && *current != phase
        {
            *current = phase;
            self.emit_status();
        }
    }

    /// An attempt ended with a new daemon connection or an error.
    pub fn attempt_finished(
        &mut self,
        attempt: AttemptId,
        result: Result<C, ConnectionError>,
        now: Instant,
    ) -> AttemptVerdict {
        let in_flight = match &self.state {
            State::Connecting { attempt: id, .. } => *id == attempt,
            State::Reconnecting {
                step: Step::InFlight(id),
                ..
            } => *id == attempt,
            State::Reconnecting {
                step: Step::Waiting { .. },
                ..
            }
            | State::Connected { .. }
            | State::Failed { .. }
            | State::Disconnected => false,
        };
        if !in_flight {
            return AttemptVerdict::Stale;
        }
        let reconnecting = matches!(self.state, State::Reconnecting { .. });
        match result {
            Ok(daemon) => {
                self.state = State::Connected {
                    daemon,
                    heartbeat_deadline: now + HEARTBEAT_TIMEOUT,
                };
                self.effects.push_back(Effect::WatchHeartbeat);
                self.emit_status();
                if reconnecting {
                    // The forwards ride their own ssh sessions, which died with the network
                    // (or are about to): rebuild them all.
                    self.restart_forwards(now);
                }
            }
            Err(error) => {
                if let State::Reconnecting {
                    attempts,
                    error: last,
                    step,
                } = &mut self.state
                {
                    tracing::warn!(connection = %self.id, attempt = *attempts, %error, "reconnect attempt failed");
                    *step = Step::Waiting {
                        until: now + reconnect_delay(*attempts),
                    };
                    *last = error;
                } else {
                    tracing::error!(connection = %self.id, %error, "connect failed");
                    self.state = State::Failed { error };
                }
                self.emit_status();
            }
        }
        AttemptVerdict::Applied
    }

    // --- liveness ----------------------------------------------------------------------------

    /// The session watch delivered something: the connection is alive.
    pub fn heartbeat(&mut self, now: Instant) {
        if let State::Connected {
            heartbeat_deadline, ..
        } = &mut self.state
        {
            *heartbeat_deadline = now + HEARTBEAT_TIMEOUT;
        }
    }

    /// The session watch failed or ended.
    pub fn heartbeat_failed(&mut self, reason: String, now: Instant) {
        if matches!(self.state, State::Connected { .. }) {
            self.start_reconnect(ConnectionError::SessionWatchFailed(reason), now);
        }
    }

    /// The tunnel's ssh process ended.
    pub fn tunnel_closed(&mut self, end: TunnelEnd, now: Instant) {
        if matches!(self.state, State::Connected { .. }) {
            self.start_reconnect(ConnectionError::TunnelClosed(end), now);
        }
    }

    // --- time --------------------------------------------------------------------------------

    /// The earliest instant [`Self::handle_timeout`] has something to do.
    pub fn poll_timeout(&self) -> Option<Instant> {
        let own = match &self.state {
            State::Connected {
                heartbeat_deadline, ..
            } => Some(*heartbeat_deadline),
            State::Reconnecting {
                step: Step::Waiting { until },
                ..
            } => Some(*until),
            State::Reconnecting {
                step: Step::InFlight(_),
                ..
            }
            | State::Connecting { .. }
            | State::Failed { .. }
            | State::Disconnected => None,
        };
        own.into_iter()
            .chain(self.forwards.values().filter_map(PortForward::poll_timeout))
            .min()
    }

    pub fn handle_timeout(&mut self, now: Instant) {
        match &self.state {
            State::Connected {
                heartbeat_deadline, ..
            } if now >= *heartbeat_deadline => {
                self.start_reconnect(ConnectionError::HeartbeatLost, now);
            }
            State::Reconnecting {
                step: Step::Waiting { until },
                ..
            } if now >= *until => self.attempt_reconnect(),
            State::Connected { .. }
            | State::Reconnecting { .. }
            | State::Connecting { .. }
            | State::Failed { .. }
            | State::Disconnected => {}
        }
        for forward in self.forwards.values_mut() {
            forward.handle_timeout(now);
        }
        self.collect_forward_events();
    }

    // --- user actions ------------------------------------------------------------------------

    /// Retry a failed connection (Electron's `reconnect`: only from the error state).
    pub fn reconnect(&mut self, now: Instant) {
        if let State::Failed { error } = &self.state {
            let error = error.clone();
            self.start_reconnect(error, now);
        }
    }

    /// Skip the backoff wait. No-op while an attempt is in flight.
    pub fn reconnect_now(&mut self) {
        if let State::Reconnecting {
            step: Step::Waiting { .. },
            ..
        } = self.state
        {
            self.attempt_reconnect();
        }
    }

    /// Start a reconnect loop whatever the state (except while one runs).
    pub fn force_reconnect(&mut self, now: Instant) {
        match &self.state {
            State::Reconnecting { .. } | State::Disconnected => {}
            State::Failed { error } => {
                let error = error.clone();
                self.start_reconnect(error, now);
            }
            State::Connecting { .. } | State::Connected { .. } => {
                self.start_reconnect(ConnectionError::ReconnectRequested, now);
            }
        }
    }

    /// Stop reconnecting; the connection shows the last error.
    pub fn cancel_reconnect(&mut self) {
        let State::Reconnecting { error, step, .. } = &self.state else {
            return;
        };
        if let Step::InFlight(_) = step {
            self.effects.push_back(Effect::CancelAttempt);
        }
        let error = match error {
            ConnectionError::ReconnectRequested => ConnectionError::ReconnectCancelled,
            other => other.clone(),
        };
        self.state = State::Failed { error };
        self.emit_status();
    }

    /// Close the connection for good: stop attempts, the transport and every forward.
    pub fn disconnect(&mut self) {
        if matches!(self.state, State::Disconnected) {
            return;
        }
        self.effects.push_back(Effect::CancelAttempt);
        self.effects.push_back(Effect::DropTransport);
        let ids: Vec<PortForwardId> = self.forwards.keys().cloned().collect();
        for id in ids {
            self.remove_port_forward(&id);
        }
        self.state = State::Disconnected;
        self.emit_status();
    }

    // --- port forwards -----------------------------------------------------------------------

    /// Start forwarding `config` through the connection's host. Replaces a forward with the
    /// same id.
    pub fn add_port_forward(
        &mut self,
        config: PortForwardConfig,
        now: Instant,
    ) -> Result<PortForwardInfo, NotRemote> {
        let ConnectionTarget::Remote(ssh) = &self.target else {
            return Err(NotRemote(self.id.clone()));
        };
        let id = config.id.clone();
        let mut forward = PortForward::new(ssh, config);
        let effect = forward.start(now);
        if let Some(mut replaced) = self.forwards.insert(id.clone(), forward)
            && let Some(kill) = replaced.stop()
        {
            self.apply_forward_effect(&id, kill);
        }
        self.apply_forward_effect(&id, effect);
        self.collect_forward_events();
        Ok(self.forwards[&id].info())
    }

    /// Stop and forget a forward. False if it is not this connection's.
    pub fn remove_port_forward(&mut self, id: &PortForwardId) -> bool {
        let Some(mut forward) = self.forwards.shift_remove(id) else {
            return false;
        };
        if let Some(kill) = forward.stop() {
            self.apply_forward_effect(id, kill);
        }
        self.drain_forward(id, &mut forward);
        self.events
            .push_back(MachineEvent::ForwardRemoved(id.clone()));
        true
    }

    pub fn port_forward(&self, id: &PortForwardId) -> Option<&PortForward> {
        self.forwards.get(id)
    }

    pub fn port_forwards(&self) -> Vec<PortForwardInfo> {
        self.forwards.values().map(PortForward::info).collect()
    }

    pub fn port_forward_event(&mut self, id: &PortForwardId, generation: u64, event: ProcessEvent) {
        if let Some(forward) = self.forwards.get_mut(id) {
            forward.on_process_event(generation, event);
            self.collect_forward_events();
        }
    }

    pub fn port_forward_spawn_failed(
        &mut self,
        id: &PortForwardId,
        generation: u64,
        message: String,
    ) {
        if let Some(forward) = self.forwards.get_mut(id) {
            forward.on_spawn_failed(generation, message);
            self.collect_forward_events();
        }
    }

    // --- outputs -----------------------------------------------------------------------------

    pub fn poll_effect(&mut self) -> Option<Effect> {
        self.effects.pop_front()
    }

    pub fn has_effects(&self) -> bool {
        !self.effects.is_empty()
    }

    pub fn poll_event(&mut self) -> Option<MachineEvent<C>> {
        self.events.pop_front()
    }

    // --- internals ---------------------------------------------------------------------------

    fn start_reconnect(&mut self, error: ConnectionError, now: Instant) {
        if matches!(self.state, State::Reconnecting { .. } | State::Disconnected) {
            return;
        }
        tracing::warn!(connection = %self.id, %error, "connection lost; reconnecting");
        if let State::Connecting { .. } = self.state {
            self.effects.push_back(Effect::CancelAttempt);
        }
        self.effects.push_back(Effect::DropTransport);
        self.state = State::Reconnecting {
            attempts: 0,
            error,
            step: Step::Waiting {
                until: now + reconnect_delay(0),
            },
        };
        self.emit_status();
    }

    fn attempt_reconnect(&mut self) {
        let attempt = self.begin_attempt(self.reconnect_outdated);
        if let State::Reconnecting { attempts, step, .. } = &mut self.state {
            *attempts += 1;
            *step = Step::InFlight(attempt);
        }
        self.emit_status();
    }

    fn begin_attempt(&mut self, outdated: OutdatedDaemon) -> AttemptId {
        self.next_attempt += 1;
        let id = AttemptId(self.next_attempt);
        let target = match &self.target {
            ConnectionTarget::Local => AttemptTarget::Local,
            ConnectionTarget::Remote(config) => AttemptTarget::Remote {
                config: config.clone(),
                outdated,
            },
        };
        self.effects
            .push_back(Effect::StartAttempt(AttemptPlan { id, target }));
        id
    }

    fn restart_forwards(&mut self, now: Instant) {
        let ids: Vec<PortForwardId> = self.forwards.keys().cloned().collect();
        for id in ids {
            let effects = self.forwards[&id].restart(now);
            for effect in effects {
                self.apply_forward_effect(&id, effect);
            }
        }
        self.collect_forward_events();
    }

    fn apply_forward_effect(&mut self, id: &PortForwardId, effect: ForwardEffect) {
        self.effects.push_back(match effect {
            ForwardEffect::Spawn {
                generation,
                command,
            } => Effect::SpawnForward {
                id: id.clone(),
                generation,
                command,
            },
            ForwardEffect::Kill => Effect::KillForward { id: id.clone() },
        });
    }

    fn collect_forward_events(&mut self) {
        let mut events = Vec::new();
        for (id, forward) in &mut self.forwards {
            while let Some(event) = forward.poll_event() {
                events.push(forward_event(id, event));
            }
        }
        self.events.extend(events);
    }

    fn drain_forward(&mut self, id: &PortForwardId, forward: &mut PortForward) {
        while let Some(event) = forward.poll_event() {
            self.events.push_back(forward_event(id, event));
        }
    }

    fn emit_status(&mut self) {
        let info = self.info();
        self.events.push_back(MachineEvent::Status(info));
    }
}

fn forward_event<C>(id: &PortForwardId, event: ForwardEvent) -> MachineEvent<C> {
    match event {
        ForwardEvent::Status(info) => MachineEvent::ForwardStatus(info),
        ForwardEvent::Output(line) => MachineEvent::ForwardOutput {
            id: id.clone(),
            line,
        },
    }
}

#[cfg(test)]
#[path = "machine_tests.rs"]
mod tests;
