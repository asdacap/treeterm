//! Paints a terminal grid: cell backgrounds, selection, text, cursor, IME preedit and the
//! strip the column limit leaves unused.
//!
//! Text is shaped one row segment at a time with every glyph forced onto the cell grid, so
//! fallback fonts cannot drift columns. Wide characters (CJK, emoji) get a segment of their
//! own spanning two cells. GPUI caches shaped lines across frames, so unchanged rows cost
//! a cache lookup.

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::{
    App, Bounds, Element, ElementId, ElementInputHandler, Entity, FocusHandle, Font, FontFallbacks,
    FontFeatures, FontStyle, FontWeight, GlobalElementId, Hsla, InspectorElementId, IntoElement,
    LayoutId, Pixels, Point, Rgba, ShapedLine, SharedString, Size, StrikethroughStyle, Style,
    TextAlign, TextRun, UnderlineStyle, Window, fill, point, px, relative, size,
};
use treeterm_model::terminal::TerminalModel;
use treeterm_term::alacritty_terminal::grid::Dimensions;
use treeterm_term::alacritty_terminal::index::{Column, Line, Point as GridPoint};
use treeterm_term::alacritty_terminal::term::cell::Flags;
use treeterm_term::alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor, Rgb};
use treeterm_term::core::GridSize;
use treeterm_term::fit::propose_dimensions;
use treeterm_term::palette::{Palette, SELECTION_BACKGROUND};

use crate::terminal_view::TerminalView;

/// Space between the grid and the edge of the tab (Electron: `padding: 0.5em`).
pub const PADDING: Pixels = px(7.);

/// What the view needs to map pointer positions to cells; written every prepaint.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Geometry {
    /// Top-left corner of cell (0, 0).
    pub origin: Point<Pixels>,
    pub cell: Size<Pixels>,
    pub cols: usize,
    pub rows: usize,
}

impl Geometry {
    /// The cell under `position`, clamped to the grid, and whether it is the cell's right half.
    pub fn cell_at(&self, position: Point<Pixels>) -> (usize, usize, bool) {
        let x = (position.x - self.origin.x) / self.cell.width;
        let y = (position.y - self.origin.y) / self.cell.height;
        let column = (x.max(0.) as usize).min(self.cols.saturating_sub(1));
        let line = (y.max(0.) as usize).min(self.rows.saturating_sub(1));
        (line, column, x.fract() >= 0.5)
    }

    pub fn cell_bounds(&self, line: usize, column: usize) -> Bounds<Pixels> {
        Bounds::new(
            point(
                self.origin.x + self.cell.width * column as f32,
                self.origin.y + self.cell.height * line as f32,
            ),
            self.cell,
        )
    }
}

/// Font settings resolved for painting.
#[derive(Clone)]
pub struct TermFont {
    pub family: SharedString,
    pub fallbacks: Vec<String>,
    pub size: Pixels,
}

impl TermFont {
    fn font(&self, bold: bool, italic: bool) -> Font {
        Font {
            family: self.family.clone(),
            // Ligatures would merge glyphs and shift every column after them.
            features: FontFeatures(std::sync::Arc::new(vec![
                ("calt".into(), 0),
                ("liga".into(), 0),
            ])),
            fallbacks: (!self.fallbacks.is_empty())
                .then(|| FontFallbacks::from_fonts(self.fallbacks.clone())),
            weight: if bold { FontWeight::BOLD } else { FontWeight::NORMAL },
            style: if italic { FontStyle::Italic } else { FontStyle::Normal },
        }
    }

    /// Cell size: one advance wide, ascent + descent tall (xterm.js with line height 1).
    pub fn cell_size(&self, window: &Window) -> Size<Pixels> {
        let text = window.text_system();
        let font_id = text.resolve_font(&self.font(false, false));
        let width = text
            .advance(font_id, self.size, 'm')
            .map(|advance| advance.width)
            .unwrap_or(self.size * 0.6);
        // GPUI reports descent below the baseline as a negative offset.
        let height = (text.ascent(font_id, self.size) + text.descent(font_id, self.size).abs()).ceil();
        size(width, height)
    }
}

pub struct TerminalElement {
    model: Entity<TerminalModel>,
    view: Entity<TerminalView>,
    focus: FocusHandle,
    font: TermFont,
    palette: Palette,
    geometry: Rc<Cell<Option<Geometry>>>,
    /// Blink phase: false hides a blinking cursor.
    cursor_on: bool,
    focused: bool,
    marked_text: Option<String>,
}

impl TerminalElement {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        model: Entity<TerminalModel>,
        view: Entity<TerminalView>,
        focus: FocusHandle,
        font: TermFont,
        palette: Palette,
        geometry: Rc<Cell<Option<Geometry>>>,
        cursor_on: bool,
        focused: bool,
        marked_text: Option<String>,
    ) -> Self {
        Self {
            model,
            view,
            focus,
            font,
            palette,
            geometry,
            cursor_on,
            focused,
            marked_text,
        }
    }
}

pub struct Frame {
    geometry: Geometry,
    background: Hsla,
    rects: Vec<(Bounds<Pixels>, Hsla)>,
    lines: Vec<(Point<Pixels>, ShapedLine)>,
    cursor: Option<CursorPaint>,
    preedit: Option<(Bounds<Pixels>, ShapedLine)>,
    unused_strip: Option<Bounds<Pixels>>,
    border: Hsla,
}

struct CursorPaint {
    bounds: Bounds<Pixels>,
    shape: CursorShape,
    color: Hsla,
    /// The character under a block cursor, redrawn in the background colour.
    glyph: Option<(Point<Pixels>, ShapedLine)>,
}

fn hsla(color: Rgb) -> Hsla {
    Rgba {
        r: f32::from(color.r) / 255.,
        g: f32::from(color.g) / 255.,
        b: f32::from(color.b) / 255.,
        a: 1.,
    }
    .into()
}

/// One run of identically styled text within a segment.
struct Run {
    len: usize,
    color: Hsla,
    bold: bool,
    italic: bool,
    underline: Option<UnderlineStyle>,
    strike: bool,
}

/// Text that shapes as one line starting at `column`.
struct Segment {
    column: usize,
    /// Grid cells covered so far.
    cells: usize,
    text: String,
    runs: Vec<Run>,
    /// Glyphs are forced one per cell; wide characters are left to their natural width.
    force: bool,
}

impl IntoElement for TerminalElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TerminalElement {
    type RequestLayoutState = ();
    type PrepaintState = Frame;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let style = Style {
            size: size(relative(1.).into(), relative(1.).into()),
            ..Style::default()
        };
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let cell = self.font.cell_size(window);
        let inner = Bounds::new(
            point(bounds.origin.x + PADDING, bounds.origin.y + PADDING),
            size(
                (bounds.size.width - PADDING * 2.).max(px(0.)),
                (bounds.size.height - PADDING * 2.).max(px(0.)),
            ),
        );

        // Propose a size that fits; the daemon decides and echoes it back.
        let (width_limit, live) = {
            let model = self.model.read(cx);
            (
                model.width_limit(),
                !matches!(model.overlay(), treeterm_model::terminal::Overlay::Loading),
            )
        };
        if live
            && let Some(proposed) = propose_dimensions(
                f32::from(inner.size.width),
                f32::from(inner.size.height),
                f32::from(cell.width),
                f32::from(cell.height),
            )
        {
            let wanted: GridSize = width_limit.apply(proposed);
            self.model.update(cx, |model, _| {
                model.set_cell_size(f32::from(cell.width), f32::from(cell.height));
                model.request_size(wanted);
            });
        }

        let model = self.model.read(cx);
        let term = model.term().clone();
        let term = term.lock();
        let content = term.renderable_content();
        let cols = term.columns();
        let rows = term.screen_lines();
        let geometry = Geometry {
            origin: inner.origin,
            cell,
            cols,
            rows,
        };
        self.geometry.set(Some(geometry));

        let overrides = content.colors;
        let background_rgb = self.palette.background();
        let background = hsla(background_rgb);
        let default_fg = self.palette.resolve(overrides, Color::Named(NamedColor::Foreground));
        let selection = content.selection;
        let display_offset = content.display_offset;
        let mut rects: Vec<(Bounds<Pixels>, Hsla)> = Vec::new();
        let mut lines = Vec::new();
        let text_system = window.text_system().clone();

        for row in 0..rows {
            let line = Line(row as i32 - display_offset as i32);
            let grid_row = &term.grid()[line];
            let y = inner.origin.y + cell.height * row as f32;
            let mut segments: Vec<Segment> = Vec::new();
            let mut bg_run: Option<(usize, Rgb)> = None;
            let flush_bg = |rects: &mut Vec<(Bounds<Pixels>, Hsla)>, run: Option<(usize, Rgb)>, end: usize| {
                if let Some((start, color)) = run {
                    rects.push((
                        Bounds::new(
                            point(inner.origin.x + cell.width * start as f32, y),
                            size(cell.width * (end - start) as f32, cell.height),
                        ),
                        hsla(color),
                    ));
                }
            };

            // Last column with something to draw, so trailing blanks cost nothing.
            let mut last_text = 0;
            for column in 0..cols {
                let c = grid_row[Column(column)].c;
                if c != ' ' && c != '\0' {
                    last_text = column + 1;
                }
            }

            for column in 0..cols {
                let cell_data = &grid_row[Column(column)];
                let flags = cell_data.flags;
                let selected = selection
                    .is_some_and(|range| range.contains(GridPoint::new(line, Column(column))));
                let mut fg = self.palette.foreground(overrides, cell_data.fg, flags);
                let mut bg = self.palette.resolve(overrides, cell_data.bg);
                if flags.contains(Flags::INVERSE) {
                    std::mem::swap(&mut fg, &mut bg);
                }
                if selected {
                    bg = SELECTION_BACKGROUND;
                }
                if flags.contains(Flags::HIDDEN) {
                    fg = bg;
                }

                // Backgrounds: merge equal neighbours; the default is painted once for all.
                let wanted = (bg != background_rgb).then_some(bg);
                match (bg_run, wanted) {
                    (Some((_, current)), Some(color)) if current == color => {}
                    (run, wanted) => {
                        flush_bg(&mut rects, run, column);
                        bg_run = wanted.map(|color| (column, color));
                    }
                }

                if flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
                    continue;
                }
                let underline = flags.intersects(Flags::ALL_UNDERLINES).then(|| UnderlineStyle {
                    thickness: px(1.),
                    color: Some(hsla(fg)),
                    wavy: flags.contains(Flags::UNDERCURL),
                });
                let strike = flags.contains(Flags::STRIKEOUT);
                let blank = (cell_data.c == ' ' || cell_data.c == '\0') && underline.is_none() && !strike;
                if column >= last_text && blank {
                    continue;
                }
                let mut text = String::new();
                text.push(if cell_data.c == '\0' { ' ' } else { cell_data.c });
                if let Some(zerowidth) = cell_data.zerowidth() {
                    text.extend(zerowidth.iter());
                }
                let run = Run {
                    len: text.len(),
                    color: hsla(fg),
                    bold: flags.contains(Flags::BOLD),
                    italic: flags.contains(Flags::ITALIC),
                    underline,
                    strike,
                };
                let wide = flags.contains(Flags::WIDE_CHAR);
                let extend = !wide
                    && segments.last().is_some_and(|segment| {
                        segment.force && segment.column + segment.cells == column
                    });
                if extend {
                    let segment = segments.last_mut().expect("checked above");
                    segment.cells += 1;
                    segment.text.push_str(&text);
                    match segment.runs.last_mut() {
                        Some(last)
                            if last.color == run.color
                                && last.bold == run.bold
                                && last.italic == run.italic
                                && last.underline == run.underline
                                && last.strike == run.strike =>
                        {
                            last.len += run.len;
                        }
                        Some(_) | None => segment.runs.push(run),
                    }
                } else {
                    segments.push(Segment {
                        column,
                        cells: if wide { 2 } else { 1 },
                        text,
                        runs: vec![run],
                        force: !wide,
                    });
                }
            }
            flush_bg(&mut rects, bg_run, cols);

            for segment in segments {
                let runs: Vec<TextRun> = segment
                    .runs
                    .iter()
                    .map(|run| TextRun {
                        len: run.len,
                        font: self.font.font(run.bold, run.italic),
                        color: run.color,
                        background_color: None,
                        underline: run.underline,
                        strikethrough: run.strike.then(|| StrikethroughStyle {
                            thickness: px(1.),
                            color: Some(run.color),
                        }),
                    })
                    .collect();
                let shaped = text_system.shape_line(
                    segment.text.into(),
                    self.font.size,
                    &runs,
                    segment.force.then_some(cell.width),
                );
                let x = inner.origin.x + cell.width * segment.column as f32;
                lines.push((point(x, y), shaped));
            }
        }

        // Cursor, in viewport coordinates.
        let cursor_point = content.cursor.point;
        let cursor_row = cursor_point.line.0 + display_offset as i32;
        let cursor_visible = (0..rows as i32).contains(&cursor_row);
        let cursor = if cursor_visible && content.cursor.shape != CursorShape::Hidden {
            let row = cursor_row as usize;
            let column = cursor_point.column.0;
            let cell_data = &term.grid()[cursor_point];
            let wide = cell_data.flags.contains(Flags::WIDE_CHAR);
            let mut bounds = geometry.cell_bounds(row, column);
            if wide {
                bounds.size.width = cell.width * 2.;
            }
            let shape = if !self.focused {
                CursorShape::HollowBlock
            } else {
                content.cursor.shape
            };
            let blinking = self.focused && term.cursor_style().blinking;
            let shown = !blinking || self.cursor_on;
            let color = hsla(self.palette.resolve(overrides, Color::Named(NamedColor::Cursor)));
            let glyph = (shape == CursorShape::Block && shown && cell_data.c != ' ' && cell_data.c != '\0')
                .then(|| {
                    let text: String = std::iter::once(cell_data.c)
                        .chain(cell_data.zerowidth().into_iter().flatten().copied())
                        .collect();
                    let run = TextRun {
                        len: text.len(),
                        font: self.font.font(cell_data.flags.contains(Flags::BOLD), cell_data.flags.contains(Flags::ITALIC)),
                        color: background,
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    };
                    let shaped = text_system.shape_line(text.into(), self.font.size, &[run], None);
                    (bounds.origin, shaped)
                });
            shown.then_some(CursorPaint {
                bounds,
                shape,
                color,
                glyph,
            })
        } else {
            None
        };

        // IME preedit sits on the cursor, over whatever the grid shows there.
        let preedit = self.marked_text.as_ref().filter(|text| !text.is_empty()).map(|text| {
            let row = cursor_row.clamp(0, rows as i32 - 1) as usize;
            let origin = geometry.cell_bounds(row, cursor_point.column.0).origin;
            let run = TextRun {
                len: text.len(),
                font: self.font.font(false, false),
                color: hsla(default_fg),
                background_color: None,
                underline: Some(UnderlineStyle {
                    thickness: px(1.),
                    color: Some(hsla(default_fg)),
                    wavy: false,
                }),
                strikethrough: None,
            };
            let shaped = text_system.shape_line(text.clone().into(), self.font.size, &[run], None);
            let bounds = Bounds::new(origin, size(shaped.width.max(cell.width), cell.height));
            (bounds, shaped)
        });

        let unused_strip = width_limit
            .unused_strip(f32::from(inner.size.width), cols as u16, f32::from(cell.width))
            .map(|left| {
                let x = inner.origin.x + px(left);
                Bounds::new(
                    point(x, bounds.origin.y),
                    size(bounds.origin.x + bounds.size.width - x, bounds.size.height),
                )
            });

        Frame {
            geometry,
            background,
            rects,
            lines,
            cursor,
            preedit,
            unused_strip,
            border: crate::theme::colors(cx).border,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        frame: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        window.handle_input(
            &self.focus,
            ElementInputHandler::new(bounds, self.view.clone()),
            cx,
        );
        window.with_content_mask(Some(gpui_kit::ContentMask { bounds }), |window| {
            window.paint_quad(fill(bounds, frame.background));
            for (rect, color) in &frame.rects {
                window.paint_quad(fill(*rect, *color));
            }
            let line_height = frame.geometry.cell.height;
            for (origin, line) in &frame.lines {
                if let Err(error) = line.paint(*origin, line_height, TextAlign::Left, None, window, cx) {
                    tracing::warn!(%error, "terminal line paint failed");
                }
            }
            if let Some(cursor) = &frame.cursor {
                paint_cursor(cursor, line_height, window, cx);
            }
            if let Some((rect, line)) = &frame.preedit {
                window.paint_quad(fill(*rect, frame.background));
                if let Err(error) = line.paint(rect.origin, line_height, TextAlign::Left, None, window, cx) {
                    tracing::warn!(%error, "preedit paint failed");
                }
            }
            if let Some(strip) = frame.unused_strip {
                let mut shade = frame.border;
                shade.a = 0.25;
                window.paint_quad(fill(strip, shade));
                window.paint_quad(fill(
                    Bounds::new(strip.origin, size(px(1.), strip.size.height)),
                    frame.border,
                ));
            }
        });
    }
}

fn paint_cursor(cursor: &CursorPaint, line_height: Pixels, window: &mut Window, cx: &mut App) {
    let b = cursor.bounds;
    match cursor.shape {
        CursorShape::Block => {
            window.paint_quad(fill(b, cursor.color));
            if let Some((origin, glyph)) = &cursor.glyph
                && let Err(error) = glyph.paint(*origin, line_height, TextAlign::Left, None, window, cx)
            {
                tracing::warn!(%error, "cursor glyph paint failed");
            }
        }
        CursorShape::Beam => {
            window.paint_quad(fill(Bounds::new(b.origin, size(px(2.), b.size.height)), cursor.color));
        }
        CursorShape::Underline => {
            window.paint_quad(fill(
                Bounds::new(point(b.origin.x, b.origin.y + b.size.height - px(2.)), size(b.size.width, px(2.))),
                cursor.color,
            ));
        }
        CursorShape::HollowBlock => {
            window.paint_quad(gpui_kit::outline(b, cursor.color, gpui_kit::BorderStyle::Solid));
        }
        CursorShape::Hidden => {}
    }
}
