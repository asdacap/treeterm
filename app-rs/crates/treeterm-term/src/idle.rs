//! Quiet/active detection from viewport snapshots. Port of `utils/idleDetector.ts`.
//!
//! Sans-IO: the owner passes the time in and schedules a timer for [`IdleDetector::deadline`],
//! calling [`IdleDetector::on_timer`] when it fires. Consumers feed rendered viewport
//! snapshots; a frame identical to the last one repainted nothing visible and is not
//! activity. A short burst that goes quiet on the screen it started from (a blip) reports
//! nothing, so it neither rings nor triggers a reclassification.

use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdleEdge {
    /// The last viewport seen before the timer ran out.
    pub snapshot: String,
    /// The timeout that was armed for this quiet period.
    pub idle_timeout: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdleOutput {
    /// Quiet → active, at most once per burst, with the frame that confirmed the burst. A
    /// burst is confirmed when it is still changing one idle timeout after it began, or when
    /// it goes quiet on a screen other than the one it started from. The first burst after
    /// creation is assumed to be the attach replay and is only reported by the first rule.
    Activity(String),
    /// No viewport change for the armed timeout, after a burst that changed the screen.
    Idle(IdleEdge),
}

#[derive(Debug, Clone, Copy)]
struct Armed {
    deadline: Instant,
    timeout: Duration,
}

#[derive(Debug)]
pub struct IdleDetector {
    timer: Option<Armed>,
    last_snapshot: String,
    /// No resting state is known until the first burst resolves. That burst is usually the
    /// attach replay or initial paint, not work starting, so a short one is not reported;
    /// otherwise every tab of a freshly opened session would go Working → Idle and ring.
    settled: bool,
    burst_baseline: String,
    burst_started_at: Instant,
    reported: bool,
    force_edge: bool,
    last_timeout: Duration,
}

impl IdleDetector {
    /// `initial_snapshot` is what is on screen at creation; the first differing snapshot is
    /// the first activity.
    pub fn new(initial_snapshot: String, idle_timeout: Duration, now: Instant) -> Self {
        Self {
            timer: None,
            last_snapshot: initial_snapshot.clone(),
            settled: false,
            burst_baseline: initial_snapshot,
            burst_started_at: now,
            reported: false,
            force_edge: false,
            last_timeout: idle_timeout,
        }
    }

    /// When the owner must call [`Self::on_timer`].
    pub fn deadline(&self) -> Option<Instant> {
        self.timer.map(|armed| armed.deadline)
    }

    /// `idle_timeout` is read every time the timer is (re)armed so settings and the
    /// workspace's unread state apply live.
    pub fn process_snapshot(&mut self, snapshot: String, now: Instant, idle_timeout: Duration) -> Vec<IdleOutput> {
        let mut out = Vec::new();
        if snapshot == self.last_snapshot {
            return out;
        }
        if self.timer.take().is_some() {
            if !self.reported && now.duration_since(self.burst_started_at) >= idle_timeout {
                self.report(&snapshot, &mut out);
            }
        } else {
            self.burst_baseline = std::mem::take(&mut self.last_snapshot);
            self.burst_started_at = now;
            self.reported = false;
        }
        self.last_snapshot = snapshot;
        // Captured at arming so the edge reports the timeout that actually elapsed.
        self.last_timeout = idle_timeout;
        self.timer = Some(Armed {
            deadline: now + idle_timeout,
            timeout: idle_timeout,
        });
        out
    }

    /// The timer scheduled for [`Self::deadline`] fired. Early or stale calls do nothing.
    pub fn on_timer(&mut self, now: Instant) -> Vec<IdleOutput> {
        let mut out = Vec::new();
        let Some(armed) = self.timer else {
            return out;
        };
        if now < armed.deadline {
            return out;
        }
        self.timer = None;
        let was_settled = self.settled;
        self.settled = true;
        let forced = std::mem::take(&mut self.force_edge);
        if !self.reported {
            if self.last_snapshot == self.burst_baseline {
                // A blip is not activity, but a forced edge still owes its consumer an idle.
                if !forced {
                    return out;
                }
            } else if was_settled {
                let snapshot = self.last_snapshot.clone();
                self.report(&snapshot, &mut out);
            }
        }
        out.push(IdleOutput::Idle(IdleEdge {
            snapshot: self.last_snapshot.clone(),
            idle_timeout: armed.timeout,
        }));
        out
    }

    /// Make the next quiet edge report idle even if the burst ends back on its starting
    /// screen. For a consumer that dropped an idle edge's work (a stale response) and relies
    /// on the next edge to redo it. With no burst running, the edge is reported now.
    pub fn request_idle_edge(&mut self) -> Vec<IdleOutput> {
        if self.timer.is_some() {
            self.force_edge = true;
            return Vec::new();
        }
        vec![IdleOutput::Idle(IdleEdge {
            snapshot: self.last_snapshot.clone(),
            idle_timeout: self.last_timeout,
        })]
    }

    /// Cancel a pending idle edge. Safe to repeat.
    pub fn cancel(&mut self) {
        self.timer = None;
        self.force_edge = false;
    }

    fn report(&mut self, snapshot: &str, out: &mut Vec<IdleOutput>) {
        self.reported = true;
        out.push(IdleOutput::Activity(snapshot.to_owned()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: Duration = Duration::from_millis(1);

    /// Fake clock + detector, mirroring the vitest harness (fake timers, fixed timeout).
    struct Harness {
        detector: IdleDetector,
        now: Instant,
        timeout: Duration,
        activity: Vec<String>,
        idle: Vec<IdleEdge>,
        /// Order of outputs, `true` = activity.
        order: Vec<bool>,
    }

    impl Harness {
        fn new() -> Self {
            let now = Instant::now();
            Self {
                detector: IdleDetector::new(String::new(), 500 * MS, now),
                now,
                timeout: 500 * MS,
                activity: Vec::new(),
                idle: Vec::new(),
                order: Vec::new(),
            }
        }

        fn record(&mut self, outputs: Vec<IdleOutput>) {
            for output in outputs {
                match output {
                    IdleOutput::Activity(snapshot) => {
                        self.order.push(true);
                        self.activity.push(snapshot);
                    }
                    IdleOutput::Idle(edge) => {
                        self.order.push(false);
                        self.idle.push(edge);
                    }
                }
            }
        }

        fn snap(&mut self, snapshot: &str) {
            let outputs = self.detector.process_snapshot(snapshot.to_owned(), self.now, self.timeout);
            self.record(outputs);
        }

        fn advance(&mut self, by: Duration) {
            let target = self.now + by;
            while let Some(deadline) = self.detector.deadline().filter(|d| *d <= target) {
                self.now = deadline;
                let outputs = self.detector.on_timer(deadline);
                self.record(outputs);
            }
            self.now = target;
        }

        fn request_edge(&mut self) {
            let outputs = self.detector.request_idle_edge();
            self.record(outputs);
        }

        /// Runs the silent first burst through to its idle so a resting state is known.
        fn settle(&mut self) {
            self.snap("boot");
            self.advance(500 * MS);
            assert!(self.activity.is_empty());
            assert_eq!(self.idle.len(), 1);
            self.idle.clear();
            self.order.clear();
        }

        fn edge(snapshot: &str, ms: u64) -> IdleEdge {
            IdleEdge {
                snapshot: snapshot.to_owned(),
                idle_timeout: Duration::from_millis(ms),
            }
        }
    }

    #[test]
    fn does_not_report_the_first_burst_as_activity_because_no_resting_state_is_known() {
        let mut h = Harness::new();
        h.snap("replay");
        h.snap("more replay");
        assert!(h.activity.is_empty());
        h.advance(500 * MS);
        assert_eq!(h.idle.len(), 1);
        assert!(h.activity.is_empty());
        h.snap("real work");
        h.advance(500 * MS);
        assert_eq!(h.activity, vec!["real work"]);
    }

    #[test]
    fn reports_a_first_burst_that_keeps_changing_past_one_idle_timeout_as_activity() {
        let mut h = Harness::new();
        h.snap("spinner 1");
        h.advance(499 * MS);
        h.snap("spinner 2");
        assert!(h.activity.is_empty());
        h.advance(MS);
        h.snap("spinner 3");
        assert_eq!(h.activity, vec!["spinner 3"]);
        h.snap("spinner 4");
        assert_eq!(h.activity.len(), 1);
        h.advance(500 * MS);
        assert_eq!(h.idle, vec![Harness::edge("spinner 4", 500)]);
        h.snap("next job");
        h.advance(500 * MS);
        assert_eq!(h.activity.len(), 2);
    }

    #[test]
    fn reports_a_short_burst_that_changed_the_screen_as_activity_then_idle_when_it_goes_quiet() {
        let mut h = Harness::new();
        h.settle();
        h.snap("one");
        h.snap("two");
        assert!(h.activity.is_empty());
        h.advance(500 * MS);
        assert_eq!(h.activity, vec!["two"]);
        assert_eq!(h.idle, vec![Harness::edge("two", 500)]);
        assert_eq!(h.order, vec![true, false]);
    }

    #[test]
    fn reports_a_long_burst_while_it_is_still_changing_once() {
        let mut h = Harness::new();
        h.settle();
        h.snap("spinner 1");
        h.advance(499 * MS);
        h.snap("spinner 2");
        assert!(h.activity.is_empty());
        h.advance(MS);
        h.snap("spinner 3");
        assert_eq!(h.activity, vec!["spinner 3"]);
        h.snap("spinner 4");
        h.advance(500 * MS);
        assert_eq!(h.activity.len(), 1);
        assert_eq!(h.idle, vec![Harness::edge("spinner 4", 500)]);
    }

    #[test]
    fn ignores_a_blip_that_goes_quiet_on_the_screen_it_started_from() {
        let mut h = Harness::new();
        h.settle();
        h.snap("flash");
        h.advance(100 * MS);
        h.snap("boot");
        h.advance(5000 * MS);
        assert!(h.activity.is_empty());
        assert!(h.idle.is_empty());
        // The next burst is measured from the screen the blip left behind.
        h.snap("work");
        h.advance(500 * MS);
        assert_eq!(h.activity, vec!["work"]);
        assert_eq!(h.idle.len(), 1);
    }

    #[test]
    fn still_reports_a_long_burst_that_ends_back_on_the_screen_it_started_from() {
        let mut h = Harness::new();
        h.settle();
        h.snap("spinner 1");
        h.advance(300 * MS);
        h.snap("spinner 2");
        h.advance(300 * MS);
        h.snap("boot");
        assert_eq!(h.activity, vec!["boot"]);
        h.advance(500 * MS);
        assert_eq!(h.idle, vec![Harness::edge("boot", 500)]);
    }

    #[test]
    fn ignores_a_first_burst_that_returns_to_the_initial_screen_leaving_the_detector_settled() {
        let mut h = Harness::new();
        h.snap("flash");
        h.snap("");
        h.advance(500 * MS);
        assert!(h.idle.is_empty());
        h.snap("work");
        h.advance(500 * MS);
        assert_eq!(h.activity, vec!["work"]);
    }

    #[test]
    fn emits_nothing_until_a_snapshot_differs_from_the_initial_one() {
        let mut h = Harness::new();
        h.snap("");
        h.advance(5000 * MS);
        assert!(h.activity.is_empty());
        assert!(h.idle.is_empty());
    }

    #[test]
    fn re_arms_the_idle_timer_on_each_visible_change() {
        let mut h = Harness::new();
        h.snap("one");
        h.advance(300 * MS);
        h.snap("two");
        h.advance(300 * MS);
        assert!(h.idle.is_empty());
        h.advance(200 * MS);
        assert_eq!(h.idle.len(), 1);
    }

    #[test]
    fn ignores_identical_snapshots_so_repeated_repaints_still_idle() {
        let mut h = Harness::new();
        h.settle();
        h.snap("same screen");
        for _ in 0..5 {
            h.advance(100 * MS);
            h.snap("same screen");
        }
        assert_eq!(h.activity.len(), 1);
        assert_eq!(h.idle.len(), 1);
    }

    #[test]
    fn reads_the_timeout_when_arming_so_a_change_mid_burst_is_honoured() {
        let mut h = Harness::new();
        h.snap("one");
        h.timeout = 2000 * MS;
        h.snap("two");
        h.advance(1999 * MS);
        assert!(h.idle.is_empty());
        h.advance(MS);
        assert_eq!(h.idle, vec![Harness::edge("two", 2000)]);
    }

    #[test]
    fn cancel_drops_a_pending_idle_and_is_safe_to_repeat() {
        let mut h = Harness::new();
        h.snap("one");
        h.detector.cancel();
        h.detector.cancel();
        h.advance(5000 * MS);
        assert!(h.idle.is_empty());
    }

    #[test]
    fn a_requested_edge_fires_idle_for_a_blip_back_to_the_starting_screen_without_activity() {
        let mut h = Harness::new();
        h.settle();
        h.snap("spinner");
        h.request_edge();
        h.snap("boot");
        h.advance(500 * MS);
        assert!(h.activity.is_empty());
        assert_eq!(h.idle, vec![Harness::edge("boot", 500)]);
        // One-shot: the next blip is silent again.
        h.snap("spinner");
        h.snap("boot");
        h.advance(500 * MS);
        assert_eq!(h.idle.len(), 1);
    }

    #[test]
    fn a_requested_edge_with_no_burst_running_fires_idle_immediately() {
        let mut h = Harness::new();
        h.settle();
        h.request_edge();
        assert_eq!(h.idle, vec![Harness::edge("boot", 500)]);
    }

    #[test]
    fn early_timer_calls_do_nothing() {
        let mut h = Harness::new();
        h.settle();
        h.snap("work");
        let early = h.now + 100 * MS;
        assert!(h.detector.on_timer(early).is_empty());
        assert!(h.detector.deadline().is_some());
    }
}
