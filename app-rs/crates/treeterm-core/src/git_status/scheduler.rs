//! When git status refreshes run (the cooldown logic of `createGitControllerStore`), sans
//! IO: the owner tells it what happened and when, and does what it answers.
//!
//! - A request with nothing running and the cooldown over starts a run at once.
//! - Requests while a run is going, or within the cooldown of the last start, coalesce into
//!   one more run, started when the cooldown ends (and never while one is running).
//! - Every request gets a [`Ticket`], completed by the run that covers it.

use std::time::{Duration, Instant};

/// The least time between the starts of two runs.
pub const REFRESH_COOLDOWN: Duration = Duration::from_secs(3);

/// One request, completed when a run that started after it finishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Ticket(u64);

/// What the owner must do next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use = "the scheduler only decides; the owner must start the run or the timer"]
pub enum Next {
    Nothing,
    /// Start a refresh now; report it with [`RefreshScheduler::run_finished`].
    StartRun,
    /// Call [`RefreshScheduler::timer_fired`] at this instant (replacing any earlier timer;
    /// a stale one firing is ignored).
    WakeAt(Instant),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Run {
    Idle,
    Running,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Timer {
    Disarmed,
    Armed(Instant),
}

#[derive(Debug, Clone)]
pub struct RefreshScheduler {
    cooldown: Duration,
    /// When the last run started; none yet counts as long ago.
    last_start: Option<Instant>,
    run: Run,
    timer: Timer,
    /// Waiting for the next run.
    queued: Vec<Ticket>,
    /// Covered by the run going on.
    active: Vec<Ticket>,
    next_ticket: u64,
}

impl Default for RefreshScheduler {
    fn default() -> Self {
        Self::new(REFRESH_COOLDOWN)
    }
}

impl RefreshScheduler {
    pub fn new(cooldown: Duration) -> Self {
        Self {
            cooldown,
            last_start: None,
            run: Run::Idle,
            timer: Timer::Disarmed,
            queued: Vec::new(),
            active: Vec::new(),
            next_ticket: 0,
        }
    }

    /// `gitRefreshing`.
    pub fn is_refreshing(&self) -> bool {
        self.run == Run::Running
    }

    fn cooled_down(&self, now: Instant) -> bool {
        self.last_start
            .is_none_or(|start| now.saturating_duration_since(start) >= self.cooldown)
    }

    fn start_run(&mut self, now: Instant) -> Next {
        self.timer = Timer::Disarmed;
        self.last_start = Some(now);
        self.run = Run::Running;
        self.active = std::mem::take(&mut self.queued);
        Next::StartRun
    }

    /// Wake when the cooldown ends, unless a wake-up is already due.
    fn schedule_queued(&mut self, now: Instant) -> Next {
        if let Timer::Armed(_) = self.timer {
            return Next::Nothing;
        }
        let at = self
            .last_start
            .map_or(now, |start| (start + self.cooldown).max(now));
        self.timer = Timer::Armed(at);
        Next::WakeAt(at)
    }

    /// Ask for a refresh (`refreshGit`).
    pub fn request(&mut self, now: Instant) -> (Ticket, Next) {
        let ticket = Ticket(self.next_ticket);
        self.next_ticket += 1;
        self.queued.push(ticket);
        let next = match self.run {
            Run::Running => Next::Nothing,
            Run::Idle if self.cooled_down(now) => self.start_run(now),
            Run::Idle => self.schedule_queued(now),
        };
        (ticket, next)
    }

    /// The timer asked for by [`Next::WakeAt`] went off.
    pub fn timer_fired(&mut self, now: Instant) -> Next {
        match self.timer {
            Timer::Armed(at) if now >= at => {
                self.timer = Timer::Disarmed;
                match self.run {
                    Run::Idle if !self.queued.is_empty() => self.start_run(now),
                    Run::Idle | Run::Running => Next::Nothing,
                }
            }
            Timer::Armed(_) | Timer::Disarmed => Next::Nothing,
        }
    }

    /// The run started by [`Next::StartRun`] finished. Returns the tickets it completed.
    ///
    /// # Panics
    /// When no run is going (the owner reported a run it did not start).
    pub fn run_finished(&mut self, now: Instant) -> (Vec<Ticket>, Next) {
        assert_eq!(self.run, Run::Running, "run_finished without a run");
        self.run = Run::Idle;
        let completed = std::mem::take(&mut self.active);
        let next = if self.queued.is_empty() {
            Next::Nothing
        } else {
            self.schedule_queued(now)
        };
        (completed, next)
    }

    /// Drop the queued requests and the pending wake-up (`dispose`); a run going on still
    /// finishes. Returns the tickets that will never complete.
    pub fn dispose(&mut self) -> Vec<Ticket> {
        self.timer = Timer::Disarmed;
        std::mem::take(&mut self.queued)
    }
}
