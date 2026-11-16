//! The flash a workspace's rows play when it enters an attention state. Port of
//! `WorkspaceAttentionFlash` (`TreePane.tsx`) and `.tree-item-flash` (`index.css`): the
//! state's colour washes over the row to 30% opacity and back, 0.6 s ease-in-out, three
//! times. Like the ding it stays quiet when the unread marker was already showing, and it
//! never plays for idle or working.

use std::time::{Duration, Instant};

use crate::activity::ActivityState;

/// One pulse (`animation: workspace-attention-flash 0.6s ease-in-out 3`).
pub const FLASH_PERIOD: Duration = Duration::from_millis(600);
/// Pulses per flash.
pub const FLASH_REPEATS: u32 = 3;
/// The opacity mid-pulse (`50% { opacity: 0.3 }`).
pub const FLASH_PEAK: f32 = 0.3;

/// One workspace's flash, following its aggregate activity.
#[derive(Debug, Default)]
pub struct AttentionFlash {
    /// The aggregate activity last observed. Activity is runtime only, so it starts idle.
    activity: ActivityState,
    flash: Flash,
}

#[derive(Debug, Default, Clone, Copy)]
enum Flash {
    #[default]
    Quiet,
    Playing {
        state: ActivityState,
        started: Instant,
    },
}

impl AttentionFlash {
    /// The workspace's aggregate activity is `activity` at `now`; `unread_before` is whether
    /// its unread marker showed before this change (the change itself may record attention).
    /// A new state replays the flash, or stops it when it is idle or working or the marker was
    /// showing; the same state again changes nothing, and an unread-only change neither.
    pub fn observe(&mut self, activity: ActivityState, unread_before: bool, now: Instant) {
        if activity == self.activity {
            return;
        }
        self.activity = activity;
        self.flash = match activity {
            ActivityState::Idle | ActivityState::Working => Flash::Quiet,
            _ if unread_before => Flash::Quiet,
            state => Flash::Playing {
                state,
                started: now,
            },
        };
    }

    /// The state whose colour flashes and its opacity at `now`; `None` when nothing flashes
    /// or the flash is over.
    pub fn frame(&self, now: Instant) -> Option<(ActivityState, f32)> {
        match self.flash {
            Flash::Quiet => None,
            Flash::Playing { state, started } => {
                flash_opacity(now.saturating_duration_since(started))
                    .map(|opacity| (state, opacity))
            }
        }
    }
}

/// The flash's opacity `elapsed` into it; `None` once its pulses are over. Each pulse rises
/// from 0 to [`FLASH_PEAK`] and falls back, each half eased in and out (CSS applies the timing
/// function to each keyframe interval).
pub fn flash_opacity(elapsed: Duration) -> Option<f32> {
    if elapsed >= FLASH_PERIOD * FLASH_REPEATS {
        return None;
    }
    let period = FLASH_PERIOD.as_nanos();
    let phase = (elapsed.as_nanos() % period) as f32 / period as f32;
    let half = if phase < 0.5 {
        phase * 2.
    } else {
        (1. - phase) * 2.
    };
    Some(FLASH_PEAK * ease_in_out(half))
}

/// Quadratic ease-in-out, close to CSS `ease-in-out` (`cubic-bezier(0.42, 0, 0.58, 1)`).
fn ease_in_out(t: f32) -> f32 {
    if t < 0.5 {
        2. * t * t
    } else {
        1. - (2. - 2. * t).powi(2) / 2.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(millis: u64) -> Duration {
        Duration::from_millis(millis)
    }

    fn close(actual: Option<f32>, expected: f32) -> bool {
        actual.is_some_and(|actual| (actual - expected).abs() < 1e-4)
    }

    #[test]
    fn entering_an_attention_state_flashes_it_unless_the_marker_was_showing() {
        let t0 = Instant::now();
        let mut flash = AttentionFlash::default();
        flash.observe(ActivityState::Working, false, t0);
        assert_eq!(flash.frame(t0), None, "working does not flash");
        flash.observe(ActivityState::Completed, false, t0 + ms(10));
        assert_eq!(
            flash.frame(t0 + ms(10)),
            Some((ActivityState::Completed, 0.))
        );
        let (state, opacity) = flash.frame(t0 + ms(310)).unwrap();
        assert_eq!(state, ActivityState::Completed);
        assert!((opacity - FLASH_PEAK).abs() < 1e-4);

        // Finished while the marker was already showing: quiet, like the ding.
        flash.observe(ActivityState::Working, true, t0 + ms(400));
        flash.observe(ActivityState::Completed, true, t0 + ms(500));
        assert_eq!(flash.frame(t0 + ms(800)), None);
        // An unread-only change re-arms nothing; the next state change decides again.
        flash.observe(ActivityState::Completed, false, t0 + ms(900));
        assert_eq!(flash.frame(t0 + ms(1200)), None);
        flash.observe(ActivityState::Error, false, t0 + ms(1000));
        assert_eq!(flash.frame(t0 + ms(1000)), Some((ActivityState::Error, 0.)));
    }

    #[test]
    fn a_new_attention_state_restarts_the_flash_and_the_same_one_does_not() {
        let t0 = Instant::now();
        let mut flash = AttentionFlash::default();
        flash.observe(ActivityState::PermissionRequest, false, t0);
        // The same state again (the marker now showing) keeps the flash going.
        flash.observe(ActivityState::PermissionRequest, true, t0 + ms(1000));
        assert!(flash.frame(t0 + ms(1500)).is_some());
        assert_eq!(flash.frame(t0 + ms(1800)), None, "three pulses, then over");
        // Another attention state starts again from its own beginning, in its colour.
        flash.observe(ActivityState::UserInputRequired, false, t0 + ms(1700));
        assert!(close(
            flash.frame(t0 + ms(2000)).map(|(_, o)| o),
            FLASH_PEAK
        ));
        assert_eq!(
            flash.frame(t0 + ms(2000)).map(|(state, _)| state),
            Some(ActivityState::UserInputRequired)
        );
    }

    #[test]
    fn going_idle_or_working_stops_a_flash_at_once() {
        let t0 = Instant::now();
        for quiet in [ActivityState::Idle, ActivityState::Working] {
            let mut flash = AttentionFlash::default();
            flash.observe(ActivityState::ApplicationError, false, t0);
            assert!(flash.frame(t0 + ms(100)).is_some());
            flash.observe(quiet, false, t0 + ms(200));
            assert_eq!(flash.frame(t0 + ms(300)), None, "{quiet:?}");
        }
        assert_eq!(
            AttentionFlash::default().frame(t0),
            None,
            "a new workspace is idle"
        );
    }

    #[test]
    fn each_pulse_rises_to_thirty_percent_and_falls_back_three_times() {
        for start in [0, 600, 1200] {
            assert!(close(flash_opacity(ms(start)), 0.), "{start}");
            assert!(close(flash_opacity(ms(start + 300)), FLASH_PEAK), "{start}");
            // Eased: a quarter of the way through a half is an eighth of the way up.
            assert!(
                close(flash_opacity(ms(start + 75)), FLASH_PEAK / 8.),
                "{start}"
            );
            assert!(
                close(flash_opacity(ms(start + 525)), FLASH_PEAK / 8.),
                "{start}"
            );
            assert!(
                close(flash_opacity(ms(start + 225)), FLASH_PEAK * 7. / 8.),
                "{start}"
            );
        }
        assert!(flash_opacity(ms(1799)).is_some_and(|o| o < 0.001));
        assert_eq!(flash_opacity(ms(1800)), None);
        assert_eq!(flash_opacity(ms(60_000)), None);
    }

    #[test]
    fn a_frame_before_the_flash_started_reads_as_its_start() {
        let t0 = Instant::now() + ms(1000);
        let mut flash = AttentionFlash::default();
        flash.observe(ActivityState::SafePermissionRequested, false, t0);
        assert_eq!(
            flash.frame(t0 - ms(500)),
            Some((ActivityState::SafePermissionRequested, 0.))
        );
    }
}
