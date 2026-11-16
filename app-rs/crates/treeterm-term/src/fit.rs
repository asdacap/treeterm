//! Sizing a terminal to its container. Port of `terminal/proposeDimensions.ts` and the
//! column limit in `BaseTerminal.tsx`.
//!
//! The result is only a proposal: the daemon owns the size and echoes back the one it
//! applied, and only then does the grid change.

use crate::core::GridSize;

/// The cell grid that fills a `width` × `height` area, or `None` when the answer would be
/// meaningless (no font measured yet, or the area has no layout).
pub fn propose_dimensions(width: f32, height: f32, cell_width: f32, cell_height: f32) -> Option<GridSize> {
    if cell_width <= 0.0 || cell_height <= 0.0 || width <= 0.0 || height <= 0.0 {
        return None;
    }
    let cols = (width / cell_width).floor().clamp(2.0, f32::from(u16::MAX)) as u16;
    let rows = (height / cell_height).floor().clamp(1.0, f32::from(u16::MAX)) as u16;
    Some(GridSize { cols, rows })
}

/// Whether a tab's columns are capped at `settings.terminal.maxCols`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WidthLimit {
    Capped(u16),
    /// The tab switched the limit off in its status bar.
    Off,
}

impl WidthLimit {
    /// Apply the cap before requesting, so the daemon's echo matches the request and the
    /// size-mismatch badge stays hidden.
    pub fn apply(self, proposed: GridSize) -> GridSize {
        match self {
            WidthLimit::Capped(max) => GridSize {
                cols: proposed.cols.min(max.max(2)),
                rows: proposed.rows,
            },
            WidthLimit::Off => proposed,
        }
    }

    /// Where the strip the cap leaves unused begins (px from the grid's left edge), or
    /// `None` when there is none worth marking. Sub-cell leftovers from rounding are not
    /// worth marking, and neither is anything while the limit is off.
    pub fn unused_strip(self, container_width: f32, cols: u16, cell_width: f32) -> Option<f32> {
        match self {
            WidthLimit::Off => None,
            WidthLimit::Capped(_) => {
                let grid_width = f32::from(cols) * cell_width;
                (container_width - grid_width >= cell_width).then_some(grid_width)
            }
        }
    }
}

/// What one view of a PTY asked the daemon for. Port of `applyFit` and `requestedSize` in
/// `BaseTerminal.tsx`.
///
/// Any attachment may resize the PTY, and the daemon applies the last request. A view asks
/// again only when its own fit changes (its container, font or width limit), never because
/// the size is not the one it asked for: two views of one PTY at different sizes would
/// otherwise take it from each other on every frame. A size another attachment applied is
/// kept, and [`FitState::mismatch`] flags it.
#[derive(Debug, Default)]
pub struct FitState {
    /// The size the view called for last time.
    last_fit: Option<GridSize>,
    /// The size the view last asked for. Kept after the daemon's echo, so a later size from
    /// another attachment is a mismatch.
    last_requested: Option<GridSize>,
}

impl FitState {
    /// The size to ask the daemon for, if any: none while the view's fit is unchanged, or
    /// when the grid (`current`) already has it.
    pub fn propose(&mut self, wanted: GridSize, current: GridSize) -> Option<GridSize> {
        if self.last_fit == Some(wanted) {
            return None;
        }
        self.last_fit = Some(wanted);
        if wanted == current {
            return None;
        }
        self.last_requested = Some(wanted);
        Some(wanted)
    }

    /// `(requested, actual)` when the size the daemon reported is not the one last asked for.
    pub fn mismatch(&self, actual: GridSize) -> Option<(GridSize, GridSize)> {
        self.last_requested
            .filter(|requested| *requested != actual)
            .map(|requested| (requested, actual))
    }

    /// Fit again without mounting again, as Electron's width-limit switch does: the next fit
    /// is compared with the grid even if it is unchanged, and the last request still stands.
    pub fn forget_fit(&mut self) {
        self.last_fit = None;
    }

    /// Start over, as a terminal mounting again in Electron: the next fit is proposed even if
    /// it is unchanged, and nothing counts as requested.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_the_area_with_whole_cells() {
        assert_eq!(
            propose_dimensions(805.0, 410.0, 8.0, 17.0),
            Some(GridSize { cols: 100, rows: 24 })
        );
    }

    #[test]
    fn never_proposes_fewer_than_two_columns_or_one_row() {
        assert_eq!(
            propose_dimensions(3.0, 3.0, 8.0, 17.0),
            Some(GridSize { cols: 2, rows: 1 })
        );
    }

    #[test]
    fn has_no_answer_without_a_font_or_a_layout() {
        assert_eq!(propose_dimensions(800.0, 400.0, 0.0, 17.0), None);
        assert_eq!(propose_dimensions(0.0, 400.0, 8.0, 17.0), None);
        assert_eq!(propose_dimensions(800.0, 0.0, 8.0, 17.0), None);
    }

    #[test]
    fn the_width_limit_caps_columns_unless_off() {
        let wide = GridSize { cols: 200, rows: 50 };
        assert_eq!(WidthLimit::Capped(160).apply(wide), GridSize { cols: 160, rows: 50 });
        assert_eq!(WidthLimit::Capped(300).apply(wide), wide);
        assert_eq!(WidthLimit::Off.apply(wide), wide);
    }

    #[test]
    fn the_unused_strip_is_marked_only_when_a_cell_or_more_is_left() {
        assert_eq!(WidthLimit::Capped(160).unused_strip(1600.0, 160, 8.0), Some(1280.0));
        assert_eq!(WidthLimit::Capped(160).unused_strip(1285.0, 160, 8.0), None);
        assert_eq!(WidthLimit::Off.unused_strip(1600.0, 160, 8.0), None);
    }

    const SMALL: GridSize = GridSize { cols: 80, rows: 24 };
    const LARGE: GridSize = GridSize { cols: 100, rows: 30 };
    const OTHER: GridSize = GridSize { cols: 90, rows: 20 };

    #[test]
    fn an_unchanged_fit_is_proposed_once() {
        let mut fit = FitState::default();
        assert_eq!(fit.propose(LARGE, SMALL), Some(LARGE));
        assert_eq!(fit.propose(LARGE, SMALL), None, "not again while the echo is on its way");
        assert_eq!(fit.propose(LARGE, OTHER), None, "nor to undo another attachment's size");
        assert_eq!(fit.propose(SMALL, OTHER), Some(SMALL), "a new fit is");
    }

    #[test]
    fn a_fit_the_grid_already_has_is_not_requested() {
        let mut fit = FitState::default();
        assert_eq!(fit.propose(SMALL, SMALL), None);
        assert_eq!(fit.mismatch(OTHER), None, "nothing was requested");
        assert_eq!(fit.propose(LARGE, SMALL), Some(LARGE));
        assert_eq!(fit.propose(OTHER, OTHER), None);
        assert_eq!(
            fit.mismatch(OTHER),
            Some((LARGE, OTHER)),
            "the last request stands until another is sent"
        );
    }

    #[test]
    fn a_size_other_than_the_last_request_is_a_mismatch() {
        let mut fit = FitState::default();
        assert_eq!(fit.propose(LARGE, SMALL), Some(LARGE));
        assert_eq!(fit.mismatch(LARGE), None);
        assert_eq!(fit.mismatch(OTHER), Some((LARGE, OTHER)));
        assert_eq!(fit.mismatch(LARGE), None, "kept after the echo");
    }

    #[test]
    fn a_forgotten_fit_is_compared_with_the_grid_again_and_keeps_the_request() {
        let mut fit = FitState::default();
        assert_eq!(fit.propose(LARGE, SMALL), Some(LARGE));
        fit.forget_fit();
        assert_eq!(fit.propose(LARGE, OTHER), Some(LARGE), "taken back from another attachment");
        fit.forget_fit();
        assert_eq!(fit.propose(LARGE, LARGE), None, "the grid has it already");
        assert_eq!(fit.mismatch(OTHER), Some((LARGE, OTHER)), "the last request still stands");
    }

    #[test]
    fn a_reset_proposes_the_same_fit_again_and_forgets_the_request() {
        let mut fit = FitState::default();
        assert_eq!(fit.propose(LARGE, SMALL), Some(LARGE));
        fit.reset();
        assert_eq!(fit.mismatch(OTHER), None);
        assert_eq!(fit.propose(LARGE, OTHER), Some(LARGE));
    }
}
