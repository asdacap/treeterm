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
}
