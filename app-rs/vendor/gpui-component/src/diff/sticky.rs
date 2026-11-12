//! TreeTerm patch: the sticky file header (see [`super::Diff::render_sticky_header`]).
//!
//! Drawn over the rows like a CSS `position: sticky; top: 0` header inside its file's
//! section: once the file's own header row starts to scroll out of view, the header of the
//! file at the top of the viewport stays pinned there, and the next file's header pushes
//! it up and out. It is positioned after the list has laid out its rows in the same frame,
//! so it never lags the scroll by a frame.

use std::rc::Rc;

use gpui::{
    AnyElement, App, AvailableSpace, Bounds, ContentMask, Element, ElementId, GlobalElementId,
    InspectorElementId, InteractiveElement as _, IntoElement, LayoutId, ListState,
    ParentElement as _, Pixels, Position, Style, Styled as _, Window, div, point, px, size,
};

use super::{DiffFile, FileRenderer, state::DisplayRow};

pub(super) struct StickyHeader {
    pub(super) list: ListState,
    pub(super) rows: Rc<Vec<DisplayRow>>,
    /// The index of each file's header row in `rows`.
    pub(super) header_rows: Vec<usize>,
    pub(super) files: Vec<DiffFile>,
    pub(super) render: FileRenderer,
}

impl StickyHeader {
    /// The file at the top of the viewport, once its own header row has started to scroll
    /// out of view.
    fn pinned(&self) -> Option<usize> {
        let top = self.list.logical_scroll_top();
        let file = self.rows.get(top.item_ix)?.file();
        let header = *self.header_rows.get(file)?;
        (top.item_ix > header || top.offset_in_item > px(0.)).then_some(file)
    }
}

impl IntoElement for StickyHeader {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for StickyHeader {
    type RequestLayoutState = ();
    type PrepaintState = Option<(AnyElement, ContentMask<Pixels>)>;

    fn id(&self) -> Option<ElementId> {
        Some("diff-sticky-header".into())
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
        // Covers the body, which clips the header pushed above it.
        let mut style = Style::default();
        style.position = Position::Absolute;
        style.inset.top = px(0.).into();
        style.inset.right = px(0.).into();
        style.inset.bottom = px(0.).into();
        style.inset.left = px(0.).into();
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
        let file = self.pinned()?;
        let viewport = self.list.viewport_bounds();
        // Blocks clicks and hovers from the rows beneath, but not the wheel.
        let mut header = div()
            .w(viewport.size.width)
            .block_mouse_except_scroll()
            .child((self.render)(&self.files[file], window, cx))
            .into_any_element();
        let height = header
            .layout_as_root(
                size(
                    AvailableSpace::Definite(viewport.size.width),
                    AvailableSpace::MinContent,
                ),
                window,
                cx,
            )
            .height;
        let next_top = self
            .header_rows
            .get(file + 1)
            .and_then(|ix| self.list.bounds_for_item(*ix))
            .map(|next| next.top());
        // The next file's header pushes it up.
        let top = match next_top {
            Some(next_top) => viewport.top().min(next_top - height),
            None => viewport.top(),
        };
        let mask = ContentMask {
            bounds: bounds.intersect(&viewport),
        };
        window.with_content_mask(Some(mask.clone()), |window| {
            header.prepaint_at(point(viewport.left(), top), window, cx)
        });
        Some((header, mask))
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some((header, mask)) = prepaint {
            window.with_content_mask(Some(mask.clone()), |window| header.paint(window, cx));
        }
    }
}
