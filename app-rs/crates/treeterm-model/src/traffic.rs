//! A rolling download rate for one byte counter (`createTrafficMeterStore.ts`): sampled every
//! [`TRAFFIC_TICK`], notifying only when the rate changes. Counting a chunk never notifies.

use gpui_kit::{Context, Task};
use treeterm_core::traffic::{ByteCounter, RateWindow, TRAFFIC_TICK, TrafficRate};

pub struct TrafficMeter {
    counter: ByteCounter,
    window: RateWindow,
    rate: TrafficRate,
    /// A sample has been taken (`ConnectionTraffic` is Loading until then).
    sampled: bool,
    _tick: Task<()>,
}

impl TrafficMeter {
    /// Starts sampling; stops when dropped.
    pub fn new(counter: ByteCounter, cx: &mut Context<Self>) -> Self {
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
            counter,
            window: RateWindow::default(),
            rate: TrafficRate::default(),
            sampled: false,
            _tick: tick,
        }
    }

    fn sample(&mut self, cx: &mut Context<Self>) {
        let rate = self.window.sample(cx.background_executor().now(), self.counter.total());
        if !self.sampled {
            self.sampled = true;
            cx.notify();
        }
        if rate != self.rate {
            self.rate = rate;
            cx.notify();
        }
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
