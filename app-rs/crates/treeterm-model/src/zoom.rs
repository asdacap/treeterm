//! View > Actual Size, Zoom In and Zoom Out. Electron's menu roles move Chromium's page zoom
//! level by half a level (`webContents.zoomLevel ± 0.5`, back to 0 for Actual Size), and
//! Chromium draws the page 1.2^level times its size, kept within 25%–500%. The level is not
//! bounded: past a limit the page stays at it, and zooming back takes as many steps. One level
//! holds for every window (Chromium zooms by origin) until the app quits.
//!
//! GPUI has no page zoom (its scale factor is the display's, and the views size themselves
//! in absolute pixels), so the app scales only terminal text by [`Zoom::factor`].

use gpui_kit::Global;

/// Chromium's page zoom limits (`kMinimumBrowserZoomFactor`, `kMaximumBrowserZoomFactor`).
const MIN_FACTOR: f32 = 0.25;
const MAX_FACTOR: f32 = 5.0;

/// The zoom level, counted in Electron's half-level steps from Actual Size.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Zoom {
    steps: i32,
}

impl Global for Zoom {}

impl Zoom {
    pub fn zoom_in(&mut self) {
        self.steps = self.steps.saturating_add(1);
    }

    pub fn zoom_out(&mut self) {
        self.steps = self.steps.saturating_sub(1);
    }

    pub fn reset(&mut self) {
        self.steps = 0;
    }

    /// How many times its size text is drawn: 1.2^level, within Chromium's limits.
    pub fn factor(&self) -> f32 {
        1.2f32
            .powf(self.steps as f32 / 2.)
            .clamp(MIN_FACTOR, MAX_FACTOR)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zoomed(steps: i32) -> Zoom {
        let mut zoom = Zoom::default();
        for _ in 0..steps.abs() {
            if steps > 0 {
                zoom.zoom_in()
            } else {
                zoom.zoom_out()
            }
        }
        zoom
    }

    #[test]
    fn actual_size_draws_text_at_its_size() {
        assert_eq!(Zoom::default().factor(), 1.);
    }

    fn assert_near(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 1e-6,
            "{actual} is not {expected}"
        );
    }

    #[test]
    fn each_step_is_half_a_chromium_level() {
        assert_near(zoomed(1).factor(), 1.2f32.sqrt());
        assert_near(zoomed(2).factor(), 1.2);
        assert_near(zoomed(-2).factor(), 1. / 1.2);

        let mut zoom = zoomed(1);
        zoom.zoom_out();
        assert_eq!(
            zoom.factor(),
            1.,
            "Zoom In then Zoom Out is back to actual size"
        );
    }

    #[test]
    fn actual_size_resets_any_level() {
        for steps in [5, -5, 40] {
            let mut zoom = zoomed(steps);
            zoom.reset();
            assert_eq!(zoom, Zoom::default());
        }
    }

    /// Electron after 30 Zoom Ins is at level 15, drawn at 500%; it takes 13 Zoom Outs to
    /// come under 500% (level 8.5), and the same past 25%.
    #[test]
    fn the_factor_stays_within_chromiums_limits_while_the_level_runs_on() {
        let mut zoom = zoomed(30);
        assert_eq!(zoom.factor(), 5.);
        for _ in 0..12 {
            zoom.zoom_out();
        }
        assert_eq!(zoom.factor(), 5., "level 9 is past 500%");
        zoom.zoom_out();
        assert_near(zoom.factor(), 1.2f32.powf(8.5));

        let mut zoom = zoomed(-30);
        assert_eq!(zoom.factor(), 0.25);
        for _ in 0..14 {
            zoom.zoom_in();
        }
        assert_eq!(zoom.factor(), 0.25, "level -8 is past 25%");
        zoom.zoom_in();
        assert_near(zoom.factor(), 1.2f32.powf(-7.5));
    }
}
