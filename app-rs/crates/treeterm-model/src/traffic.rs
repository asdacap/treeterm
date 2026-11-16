//! Rolling download rates (`createTrafficMeterStore.ts`): a connection's, and each of its
//! workspaces' terminals'. Sampled every [`TRAFFIC_TICK`], notifying only when the rate
//! changes. Counting a chunk never notifies.

use std::collections::HashMap;

use gpui_kit::{AppContext as _, Context, Entity, Task};
use treeterm_core::traffic::{ByteCounter, RateWindow, TRAFFIC_TICK, TrafficRate};

/// What a meter counts, which decides when it starts sampling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeterKind {
    /// Every byte received from a connection's daemon: sampled from the start, as Electron
    /// polls every listed connection (its badge reads "↓ …" until then).
    Connection,
    /// The PTY bytes of a workspace's terminals: no sample until some arrive, so the first
    /// sample (rate 0) absorbs the first burst, as Electron samples only the workspaces it
    /// recorded bytes for (`publishWorkspaces`).
    Workspace,
}

pub struct TrafficMeter {
    kind: MeterKind,
    counter: ByteCounter,
    /// What the counters attached before `counter` received.
    base: u64,
    window: RateWindow,
    rate: TrafficRate,
    /// A sample has been taken (`ConnectionTraffic` is Loading until then).
    sampled: bool,
    _tick: Task<()>,
}

impl TrafficMeter {
    /// Starts sampling; stops when dropped.
    pub fn new(kind: MeterKind, counter: ByteCounter, cx: &mut Context<Self>) -> Self {
        let executor = cx.background_executor().clone();
        let tick = cx.spawn(async move |this, cx| {
            loop {
                executor.timer(TRAFFIC_TICK).await;
                if this.update(cx, |meter, cx| meter.sample(cx)).is_err() {
                    return;
                }
            }
        });
        Self {
            kind,
            counter,
            base: 0,
            window: RateWindow::default(),
            rate: TrafficRate::default(),
            sampled: false,
            _tick: tick,
        }
    }

    fn sample(&mut self, cx: &mut Context<Self>) {
        let total = self.base + self.counter.total();
        match self.kind {
            MeterKind::Workspace if !self.sampled && total == 0 => return,
            MeterKind::Workspace | MeterKind::Connection => {}
        }
        let rate = self.window.sample(cx.background_executor().now(), total);
        if !self.sampled {
            self.sampled = true;
            cx.notify();
        }
        if rate != self.rate {
            self.rate = rate;
            cx.notify();
        }
    }

    /// Count `counter` from now on, on top of what the earlier ones received (a connection's
    /// next generation). The rate window and the first sample are kept.
    pub fn attach(&mut self, counter: ByteCounter) {
        self.base += self.counter.total();
        self.counter = counter;
    }

    /// What receivers add to.
    pub fn counter(&self) -> &ByteCounter {
        &self.counter
    }

    pub fn rate(&self) -> TrafficRate {
        self.rate
    }

    pub fn is_sampled(&self) -> bool {
        self.sampled
    }
}

/// A connection's download meters, kept as long as the connection is listed: across its
/// reconnects and a failed connection replaced by a new connect (Electron keeps one
/// `ByteCounter` per connection id in Main, and one meter in the app store that outlives
/// session stores). One for the connection, and one per workspace until it is removed.
pub struct ConnectionTraffic {
    connection: Entity<TrafficMeter>,
    workspaces: HashMap<String, Entity<TrafficMeter>>,
}

impl ConnectionTraffic {
    /// Nothing received yet: each connection generation attaches its counter.
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            connection: cx.new(|cx| TrafficMeter::new(MeterKind::Connection, ByteCounter::default(), cx)),
            workspaces: HashMap::new(),
        }
    }

    /// A new connection generation is up: what its daemon socket receives counts on from the
    /// connection's total so far. What the last one still receives is not counted.
    pub fn attach_generation(&mut self, received: ByteCounter, cx: &mut Context<Self>) {
        self.connection.update(cx, |meter, _| meter.attach(received));
    }

    /// The connection's meter (`ConnectionTrafficBadge`).
    pub fn connection(&self) -> &Entity<TrafficMeter> {
        &self.connection
    }

    /// Workspace `id`'s meter (`WorkspaceTrafficBadge`), made on first use.
    pub fn workspace(&mut self, id: &str, cx: &mut Context<Self>) -> Entity<TrafficMeter> {
        self.workspaces
            .entry(id.to_owned())
            .or_insert_with(|| cx.new(|cx| TrafficMeter::new(MeterKind::Workspace, ByteCounter::default(), cx)))
            .clone()
    }

    /// Workspace `id` was removed: its total and samples go (`removeWorkspace`). If it comes
    /// back it starts from 0.
    pub fn remove_workspace(&mut self, id: &str) {
        self.workspaces.remove(id);
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::TestAppContext;

    use super::*;

    fn tick(cx: &mut TestAppContext) {
        cx.executor().advance_clock(TRAFFIC_TICK);
        cx.run_until_parked();
    }

    fn rate(cx: &mut TestAppContext, meter: &Entity<TrafficMeter>) -> TrafficRate {
        meter.read_with(cx, |m, _| m.rate())
    }

    #[gpui_kit::test]
    fn a_connection_meter_is_sampled_from_the_start(cx: &mut TestAppContext) {
        let meter = cx.new(|cx| TrafficMeter::new(MeterKind::Connection, ByteCounter::default(), cx));
        assert!(!meter.read_with(cx, |m, _| m.is_sampled()));

        tick(cx);

        assert!(meter.read_with(cx, |m, _| m.is_sampled()));
        assert_eq!(rate(cx, &meter), TrafficRate::default());
    }

    #[gpui_kit::test]
    fn an_attached_counter_counts_on_from_the_total_and_keeps_the_rate_window(cx: &mut TestAppContext) {
        let first = ByteCounter::default();
        let meter = cx.new(|cx| TrafficMeter::new(MeterKind::Connection, first.clone(), cx));
        first.add(100);
        tick(cx);
        assert_eq!(rate(cx, &meter).total_bytes, 100);

        let second = ByteCounter::default();
        meter.update(cx, |m, _| m.attach(second.clone()));
        first.add(1_000); // the old generation's socket, being dropped
        second.add(50);
        tick(cx);

        assert!(meter.read_with(cx, |m, _| m.is_sampled()));
        assert_eq!(rate(cx, &meter).total_bytes, 150);
        // Not restarted: the window still holds the first sample, 1 s before.
        assert_eq!(rate(cx, &meter).bytes_per_sec, 50.0);
    }

    #[gpui_kit::test]
    fn a_workspace_meter_takes_no_sample_until_bytes_arrive_then_absorbs_the_first_burst(cx: &mut TestAppContext) {
        let counter = ByteCounter::default();
        let meter = cx.new(|cx| TrafficMeter::new(MeterKind::Workspace, counter.clone(), cx));
        for _ in 0..3 {
            tick(cx);
        }
        assert!(!meter.read_with(cx, |m, _| m.is_sampled()));
        assert_eq!(rate(cx, &meter), TrafficRate::default());

        counter.add(4_000);
        tick(cx);
        assert_eq!(rate(cx, &meter), TrafficRate { total_bytes: 4_000, bytes_per_sec: 0.0 });

        counter.add(1_000);
        tick(cx);
        assert_eq!(rate(cx, &meter), TrafficRate { total_bytes: 5_000, bytes_per_sec: 1_000.0 });
    }

    #[gpui_kit::test]
    fn a_workspace_meter_is_kept_by_id_until_the_workspace_is_removed(cx: &mut TestAppContext) {
        let traffic = cx.new(ConnectionTraffic::new);
        let meter = traffic.update(cx, |t, cx| t.workspace("a", cx));
        meter.read_with(cx, |m, _| m.counter().add(10));

        assert_eq!(traffic.update(cx, |t, cx| t.workspace("a", cx)), meter);
        assert_ne!(traffic.update(cx, |t, cx| t.workspace("b", cx)), meter);

        traffic.update(cx, |t, _| t.remove_workspace("a"));
        let back = traffic.update(cx, |t, cx| t.workspace("a", cx));
        assert_ne!(back, meter);
        assert_eq!(back.read_with(cx, |m, _| m.counter().total()), 0);
    }

    #[gpui_kit::test]
    fn each_generation_counts_on_from_the_connections_total(cx: &mut TestAppContext) {
        let traffic = cx.new(ConnectionTraffic::new);
        let meter = traffic.read_with(cx, |t, _| t.connection().clone());
        for received in [300, 200] {
            let generation = ByteCounter::default();
            traffic.update(cx, |t, cx| t.attach_generation(generation.clone(), cx));
            generation.add(received);
        }
        tick(cx);

        assert_eq!(traffic.read_with(cx, |t, _| t.connection().clone()), meter);
        assert_eq!(rate(cx, &meter).total_bytes, 500);
    }
}
